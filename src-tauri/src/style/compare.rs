//! Local image-pair comparison (CONTRACT §16). No library index, mean, network or writes.
//! Input bytes are frozen once; every encoder sees the same images and its native preprocessing.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use image::{DynamicImage, ImageReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::models::{StyleDevice, StyleDevicePreference};

use super::{catalog, model, search, StyleState};

pub const MAX_IMAGES: usize = 32;
pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const MAX_ASPECT_RATIO: u64 = 64;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareImageInput {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareRequest {
    pub images: Vec<CompareImageInput>,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComparedImage {
    pub id: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairScore {
    pub left_id: String,
    pub right_id: String,
    pub cosine: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelComparison {
    pub model: String,
    pub label: String,
    /// Expected catalog revision, not an assertion that local weights were re-hashed.
    pub model_version: String,
    pub preprocessing: String,
    pub device: Option<StyleDevice>,
    pub fallback_reason: Option<String>,
    pub load_ms: u64,
    pub encode_ms: u64,
    pub total_ms: u64,
    pub pairs: Vec<PairScore>,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareResult {
    pub images: Vec<ComparedImage>,
    pub results: Vec<ModelComparison>,
    pub elapsed_ms: u64,
    pub centered: bool,
}

struct FrozenImage {
    info: ComparedImage,
    bytes: Vec<u8>,
}

fn check_cancel(cancel: &CancellationToken) -> AppResult<()> {
    if cancel.is_cancelled() {
        Err(AppError::cancelled())
    } else {
        Ok(())
    }
}

fn millis(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn validate_request(req: &CompareRequest) -> AppResult<()> {
    if !(2..=MAX_IMAGES).contains(&req.images.len()) {
        return Err(AppError::bad_request("画风比较需要 2–32 张本地图片"));
    }
    if req.models.is_empty() || req.models.len() > catalog::MODELS.len() {
        return Err(AppError::bad_request("请选择 1–3 个画风编码器"));
    }
    let mut models = HashSet::new();
    for id in &req.models {
        catalog::get(id)?;
        if !models.insert(id) {
            return Err(AppError::bad_request("画风编码器不能重复"));
        }
    }
    let mut ids = HashSet::new();
    for input in &req.images {
        if input.id.trim().is_empty() || input.id.len() > 128 || !ids.insert(&input.id) {
            return Err(AppError::bad_request(
                "图片 ID 必须非空、唯一且不超过 128 字节",
            ));
        }
        if !Path::new(&input.path).is_absolute() {
            return Err(AppError::bad_request("图片路径必须为本地绝对路径"));
        }
    }
    Ok(())
}

fn reader(bytes: &[u8]) -> AppResult<ImageReader<Cursor<&[u8]>>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| AppError::bad_request(format!("无法识别图片：{e}")))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

fn validate_dimensions(width: u32, height: u32) -> AppResult<()> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
        || u64::from(width.max(height)) > u64::from(width.min(height)) * MAX_ASPECT_RATIO
    {
        return Err(AppError::bad_request(
            "图片超出比较限制：最多 32M 像素、长边 16384、长宽比 64:1",
        ));
    }
    Ok(())
}

fn decode(bytes: &[u8]) -> AppResult<DynamicImage> {
    reader(bytes)?
        .decode()
        .map_err(|e| AppError::bad_request(format!("读取图片失败：{e}")))
}

fn freeze_images(
    inputs: &[CompareImageInput],
    cancel: &CancellationToken,
) -> AppResult<Vec<FrozenImage>> {
    let mut paths = HashSet::new();
    let mut total_bytes = 0u64;
    let mut images = Vec::with_capacity(inputs.len());
    for input in inputs {
        check_cancel(cancel)?;
        let path = Path::new(&input.path)
            .canonicalize()
            .map_err(|e| AppError::bad_request(format!("图片 {} 无法读取：{e}", input.id)))?;
        if !paths.insert(path.clone()) {
            return Err(AppError::bad_request(
                "同一路径的图片不能重复加入比较（含符号链接）",
            ));
        }
        // Check before open as well: opening a FIFO can block before its metadata is read.
        if !path.is_file() {
            return Err(AppError::bad_request("比较图片必须为普通文件"));
        }
        let file = std::fs::File::open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
            return Err(AppError::bad_request(
                "每张比较图片必须为不超过 32 MiB 的普通文件",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes)?;
        total_bytes += bytes.len() as u64;
        if bytes.len() as u64 > MAX_IMAGE_BYTES || total_bytes > MAX_TOTAL_BYTES {
            return Err(AppError::bad_request(
                "比较图片最多每张 32 MiB、合计 128 MiB",
            ));
        }
        let (width, height) = reader(&bytes)?
            .into_dimensions()
            .map_err(|e| AppError::bad_request(format!("图片 {} 无效：{e}", input.id)))?;
        validate_dimensions(width, height)?;
        // Reject damaged image bodies before loading any potentially large model.
        drop(decode(&bytes)?);
        let sha256 = hex::encode(Sha256::digest(&bytes));
        images.push(FrozenImage {
            info: ComparedImage {
                id: input.id.clone(),
                path: path.to_string_lossy().into_owned(),
                width,
                height,
                sha256,
            },
            bytes,
        });
    }
    check_cancel(cancel)?;
    Ok(images)
}

fn preprocessing(id: &str, size: usize) -> String {
    match id {
        "clip-vit-base-patch32" => {
            format!("RGB；短边双线性缩放 {size}；中心裁剪 {size}×{size}；CLIP mean/std；L2")
        }
        "csd-vit-large-patch14" => {
            format!("RGB；短边 bicubic 缩放 {size}；中心裁剪 {size}×{size}；CLIP mean/std；style projection；L2")
        }
        "megastyle-siglip-so400m" => {
            format!("RGB；整图 bicubic 缩放 {size}×{size}；归一化 [-1,1]；attention pooling；L2")
        }
        _ => "未知预处理".into(),
    }
}

fn expected_version(spec: &catalog::ModelSpec, dir: &Path) -> String {
    if spec.id == model::MODEL_NAME
        && model::weights_path(dir)
            .is_some_and(|p| p.file_name().is_some_and(|n| n == model::SAFETENSORS_FILE))
    {
        return "local-unverified:model.safetensors".into();
    }
    let revision = spec
        .files
        .iter()
        .find(|f| f.sha256.is_some())
        .and_then(|f| f.url.split("/resolve/").nth(1))
        .and_then(|tail| tail.split('/').next())
        .unwrap_or("unknown");
    format!("catalog:{revision}; local-unverified")
}

fn empty_model_result(spec: &catalog::ModelSpec, dir: &Path) -> ModelComparison {
    ModelComparison {
        model: spec.id.into(),
        label: spec.label.into(),
        model_version: expected_version(spec, dir),
        preprocessing: preprocessing(
            spec.id,
            if spec.id == "megastyle-siglip-so400m" {
                384
            } else {
                224
            },
        ),
        device: None,
        fallback_reason: None,
        load_ms: 0,
        encode_ms: 0,
        total_ms: 0,
        pairs: Vec::new(),
        error: None,
    }
}

fn pair_scores(
    images: &[FrozenImage],
    rows: &[Vec<f32>],
    dimension: usize,
) -> AppResult<Vec<PairScore>> {
    if rows.len() != images.len()
        || rows.iter().any(|row| {
            row.len() != dimension
                || row.iter().any(|value| !value.is_finite())
                || row
                    .iter()
                    .map(|value| f64::from(*value).powi(2))
                    .sum::<f64>()
                    <= 0.0
        })
    {
        return Err(AppError::new(
            "model",
            "编码器返回无效向量，无法计算画风相似度",
        ));
    }
    let mut pairs = Vec::with_capacity(images.len() * images.len().saturating_sub(1) / 2);
    for left in 0..images.len() {
        for right in (left + 1)..images.len() {
            pairs.push(PairScore {
                left_id: images[left].info.id.clone(),
                right_id: images[right].info.id.clone(),
                cosine: search::cosine(&rows[left], &rows[right]),
            });
        }
    }
    Ok(pairs)
}

/// Testable encoding boundary; batches obey both the catalog and decoded-pixel budget.
fn encode_pairs(
    images: &[FrozenImage],
    batch_size: usize,
    dimension: usize,
    cancel: &CancellationToken,
    mut embed: impl FnMut(&[DynamicImage]) -> AppResult<Vec<Vec<f32>>>,
) -> AppResult<Vec<PairScore>> {
    let mut rows = Vec::with_capacity(images.len());
    let mut start = 0;
    while start < images.len() {
        check_cancel(cancel)?;
        let mut end = start;
        let mut pixels = 0;
        while end < images.len() && end - start < batch_size.max(1) {
            let info = &images[end].info;
            let next = u64::from(info.width) * u64::from(info.height);
            if end > start && pixels + next > MAX_IMAGE_PIXELS {
                break;
            }
            pixels += next;
            end += 1;
        }
        let decoded = images[start..end]
            .iter()
            .map(|image| decode(&image.bytes))
            .collect::<AppResult<Vec<_>>>()?;
        let encoded = embed(&decoded)?;
        if encoded.len() != decoded.len() {
            return Err(AppError::new("model", "编码器返回的图片数量不匹配"));
        }
        rows.extend(encoded);
        check_cancel(cancel)?;
        start = end;
    }
    pair_scores(images, &rows, dimension)
}

/// Caller must await completion rather than abort this future: a running Candle operation
/// cannot be preempted. Cancellation is checked around each model/batch while holding the lock.
pub async fn compare_with(
    data_dir: PathBuf,
    style: Arc<StyleState>,
    preference: StyleDevicePreference,
    req: CompareRequest,
    cancel: CancellationToken,
) -> AppResult<CompareResult> {
    let started = Instant::now();
    validate_request(&req)?;
    check_cancel(&cancel)?;
    let prepare_cancel = cancel.clone();
    let images = Arc::new(
        tokio::task::spawn_blocking(move || freeze_images(&req.images, &prepare_cancel))
            .await
            .map_err(|e| AppError::new("internal", e.to_string()))??,
    );
    let _inference = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
        guard = style.inference_lock.lock() => guard,
    };
    let mut results = Vec::with_capacity(req.models.len());
    for id in req.models {
        check_cancel(&cancel)?;
        let spec = catalog::get(&id)?;
        let dir = model::model_dir(&data_dir, spec.id);
        let mut result = empty_model_result(spec, &dir);
        let model_started = Instant::now();
        if !spec.present(&dir) {
            result.error = Some(AppError::new(
                "model_not_downloaded",
                format!(
                    "{} 未下载完整，请到设置页下载模型后重试；无需建立索引",
                    spec.label
                ),
            ));
        } else {
            let loading = Instant::now();
            let loaded = style.encoder(&dir, preference).await;
            result.load_ms = millis(loading);
            check_cancel(&cancel)?;
            match loaded {
                Err(error) => result.error = Some(error),
                Ok(encoder) => {
                    result.device = Some(encoder.device_kind());
                    result.preprocessing = preprocessing(spec.id, encoder.image_size());
                    result.fallback_reason = encoder.fallback_reason().map(str::to_owned);
                    let encoding = Instant::now();
                    let inputs = images.clone();
                    let encode_cancel = cancel.clone();
                    let encoded = tokio::task::spawn_blocking(move || {
                        encode_pairs(
                            &inputs,
                            spec.batch_size,
                            spec.dim,
                            &encode_cancel,
                            |batch| encoder.embed_batch(batch),
                        )
                    })
                    .await
                    .map_err(|e| AppError::new("internal", e.to_string()));
                    result.encode_ms = millis(encoding);
                    check_cancel(&cancel)?;
                    match encoded.and_then(|result| result) {
                        Ok(pairs) => result.pairs = pairs,
                        Err(error) => result.error = Some(error),
                    }
                }
            }
        }
        result.total_ms = millis(model_started);
        results.push(result);
    }
    check_cancel(&cancel)?;
    Ok(CompareResult {
        images: images.iter().map(|image| image.info.clone()).collect(),
        results,
        elapsed_ms: millis(started),
        centered: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures(dir: &Path, count: usize) -> Vec<CompareImageInput> {
        (0..count)
            .map(|i| {
                let path = dir.join(format!("{i}.png"));
                image::RgbImage::from_pixel(12, 8, image::Rgb([i as u8 * 50, 40, 90]))
                    .save(&path)
                    .unwrap();
                CompareImageInput {
                    id: format!("image-{i}"),
                    path: path.to_string_lossy().into_owned(),
                }
            })
            .collect()
    }

    fn request(images: Vec<CompareImageInput>) -> CompareRequest {
        CompareRequest {
            images,
            models: vec![model::MODEL_NAME.into()],
        }
    }

    #[test]
    fn rejects_invalid_request_and_image_boundaries() {
        let temp = tempfile::tempdir().unwrap();
        let images = fixtures(temp.path(), 2);
        assert!(validate_request(&request(images.clone())).is_ok());
        for count in [0, 1, 33] {
            let mut req = request(images.clone());
            req.images.resize(count, req.images[0].clone());
            assert!(validate_request(&req).is_err());
        }
        let mut req = request(images.clone());
        req.images[1].id = req.images[0].id.clone();
        assert!(validate_request(&req).is_err());
        for models in [
            vec![],
            vec!["../weights".into()],
            vec![model::MODEL_NAME.into(); 2],
        ] {
            let mut req = request(images.clone());
            req.models = models;
            assert!(validate_request(&req).is_err());
        }
        let mut req = request(images);
        req.images[0].path = "relative.png".into();
        assert!(validate_request(&req).is_err());
        for (w, h) in [(0, 1), (16385, 1), (8192, 8192), (4096, 1)] {
            assert!(validate_dimensions(w, h).is_err());
        }
        assert!(validate_dimensions(4096, 4096).is_ok());
    }

    #[test]
    fn freeze_rejects_duplicate_paths_and_bad_bodies_and_preserves_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let images = fixtures(temp.path(), 2);
        let token = CancellationToken::new();
        let frozen = freeze_images(&images, &token).unwrap();
        assert_eq!(frozen[0].info.width, 12);
        assert_eq!(
            frozen[0].info.sha256,
            hex::encode(Sha256::digest(&frozen[0].bytes))
        );
        let mut duplicate = images.clone();
        duplicate[1].path = duplicate[0].path.clone();
        assert!(freeze_images(&duplicate, &token).is_err());
        #[cfg(unix)]
        {
            let link = temp.path().join("alias.png");
            std::os::unix::fs::symlink(&images[0].path, &link).unwrap();
            duplicate[1].path = link.to_string_lossy().into_owned();
            assert!(freeze_images(&duplicate, &token).is_err());
        }
        std::fs::write(&images[0].path, b"not an image").unwrap();
        assert!(freeze_images(&images, &token).is_err());
        assert!(decode(&frozen[0].bytes).is_ok());
    }

    #[test]
    fn fake_encoder_pairs_are_complete_ordered_and_not_centered() {
        let temp = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let images = freeze_images(&fixtures(temp.path(), 3), &token).unwrap();
        let mut offset = 0;
        let pairs = encode_pairs(&images, 2, 2, &token, |batch| {
            let rows = [vec![1., 0.], vec![0., 1.], vec![-1., 0.]];
            let result = rows[offset..offset + batch.len()].to_vec();
            offset += batch.len();
            Ok(result)
        })
        .unwrap();
        assert_eq!(pairs.len(), 3);
        assert_eq!(
            pairs
                .iter()
                .map(|p| (p.left_id.as_str(), p.right_id.as_str(), p.cosine))
                .collect::<Vec<_>>(),
            vec![
                ("image-0", "image-1", 0.),
                ("image-0", "image-2", -1.),
                ("image-1", "image-2", 0.)
            ]
        );
        let duplicates = encode_pairs(&images, 2, 2, &token, |batch| {
            Ok(vec![vec![2., 3.]; batch.len()])
        })
        .unwrap();
        assert!(duplicates
            .iter()
            .all(|pair| (pair.cosine - 1.).abs() < 1e-12));
    }

    #[test]
    fn invalid_vectors_fail_instead_of_becoming_zero_similarity() {
        let temp = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let images = freeze_images(&fixtures(temp.path(), 2), &token).unwrap();
        for row in [
            vec![],
            vec![1.],
            vec![0., 0.],
            vec![f32::NAN, 1.],
            vec![f32::INFINITY, 1.],
        ] {
            assert!(encode_pairs(&images, 2, 2, &token, |batch| Ok(vec![
                row.clone();
                batch.len()
            ]))
            .is_err());
        }
        assert!(encode_pairs(&images, 2, 2, &token, |_| Ok(vec![vec![1., 0.]])).is_err());
    }

    #[test]
    fn cancellation_stops_after_current_batch() {
        let temp = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let images = freeze_images(&fixtures(temp.path(), 3), &token).unwrap();
        let mut calls = 0;
        let result = encode_pairs(&images, 1, 2, &token, |batch| {
            calls += 1;
            token.cancel();
            Ok(vec![vec![1., 0.]; batch.len()])
        });
        assert_eq!(calls, 1);
        assert_eq!(result.unwrap_err().code, "cancelled");
    }

    #[tokio::test]
    async fn missing_models_return_individual_errors_without_downloading_or_database() {
        let temp = tempfile::tempdir().unwrap();
        let mut req = request(fixtures(temp.path(), 2));
        req.models = catalog::MODELS
            .iter()
            .map(|model| model.id.into())
            .collect();
        let result = compare_with(
            temp.path().into(),
            Arc::new(StyleState::new()),
            StyleDevicePreference::Cpu,
            req,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!result.centered);
        assert_eq!(result.results.len(), 3);
        assert!(result
            .results
            .iter()
            .all(
                |model| model.error.as_ref().unwrap().code == "model_not_downloaded"
                    && model.pairs.is_empty()
                    && model.device.is_none()
            ));
        assert!(!temp.path().join("models").exists());
        assert!(!temp.path().join("library.db").exists());
    }

    #[tokio::test]
    async fn available_encoder_succeeds_when_other_models_are_missing() {
        use candle_transformers::models::clip::vision_model::ClipVisionConfig;
        let temp = tempfile::tempdir().unwrap();
        let dir = model::model_dir(temp.path(), model::MODEL_NAME);
        std::fs::create_dir_all(&dir).unwrap();
        // Only the cached tiny random encoder is used; these files are never loaded as weights.
        std::fs::write(dir.join(model::CONFIG_FILE), b"{}").unwrap();
        std::fs::write(dir.join(model::WEIGHTS_FILE), b"synthetic fixture").unwrap();
        let cfg = ClipVisionConfig {
            embed_dim: 8,
            intermediate_size: 16,
            num_hidden_layers: 1,
            num_attention_heads: 2,
            projection_dim: 512,
            image_size: 8,
            patch_size: 4,
            ..ClipVisionConfig::vit_base_patch32()
        };
        let encoder = Arc::new(model::Encoder::random(cfg).unwrap());
        let style = Arc::new(StyleState::new());
        *style.encoder.lock() = Some((dir, StyleDevicePreference::Cpu, encoder));
        let mut req = request(fixtures(temp.path(), 3));
        req.models.push("megastyle-siglip-so400m".into());
        let result = compare_with(
            temp.path().into(),
            style,
            StyleDevicePreference::Cpu,
            req,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let success = &result.results[0];
        assert!(success.error.is_none(), "{:?}", success.error);
        assert_eq!(success.device, Some(StyleDevice::Cpu));
        assert_eq!(success.pairs.len(), 3);
        assert!(success
            .pairs
            .iter()
            .all(|pair| pair.cosine.is_finite() && (-1.0..=1.0).contains(&pair.cosine)));
        assert_eq!(
            result.results[1].error.as_ref().unwrap().code,
            "model_not_downloaded"
        );
        assert!(result.results[1].pairs.is_empty());
        assert!(!temp.path().join("library.db").exists());
    }

    #[tokio::test]
    async fn cancellation_while_waiting_does_not_wait_for_other_inference() {
        let temp = tempfile::tempdir().unwrap();
        let style = Arc::new(StyleState::new());
        let _held = style.inference_lock.lock().await;
        let token = CancellationToken::new();
        let request = request(fixtures(temp.path(), 2));
        let comparison = compare_with(
            temp.path().into(),
            style.clone(),
            StyleDevicePreference::Cpu,
            request,
            token.clone(),
        );
        let cancel = async {
            tokio::task::yield_now().await;
            token.cancel();
        };
        let (result, ()) = tokio::join!(comparison, cancel);
        assert_eq!(result.unwrap_err().code, "cancelled");
    }

    #[test]
    fn preprocessing_and_version_are_explicit_and_not_means_or_claimed_verification() {
        let temp = tempfile::tempdir().unwrap();
        for spec in catalog::MODELS {
            assert!(expected_version(spec, temp.path()).starts_with("catalog:"));
            assert!(expected_version(spec, temp.path()).contains("local-unverified"));
        }
        assert!(preprocessing(model::MODEL_NAME, 224).contains("中心裁剪"));
        assert!(preprocessing("csd-vit-large-patch14", 224).contains("中心裁剪"));
        assert!(preprocessing("megastyle-siglip-so400m", 384).contains("整图"));
        std::fs::write(temp.path().join(model::SAFETENSORS_FILE), b"manual").unwrap();
        assert_eq!(
            expected_version(catalog::get(model::MODEL_NAME).unwrap(), temp.path()),
            "local-unverified:model.safetensors"
        );
    }
}
