//! 画师分类对照：Kaloscope 2.0 与 Camie Tagger v2。
//! 分类结果只给画师候选及分数，不将分类概率当作向量相似度或混合画风权重。
//! 预处理来源：Kaloscope ONNX Space 的 timm 配置（crop_pct=.9、bicubic），
//! Camie 官方 onnx_inference.py（512 等比缩放、ImageNet 均值色补边、refined logits）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use image::{imageops, DynamicImage, Rgb, RgbImage};
use ort::{session::{builder::GraphOptimizationLevel, Session}, value::Tensor};
use parking_lot::Mutex;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use super::catalog::ModelFile;
use super::download::{download_checked, DownloadEmitter};
use super::onnx_runtime;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

struct ClassifierSpec {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    image_size: u32,
    files: &'static [ModelFile],
}

const CLASSIFIERS: &[ClassifierSpec] = &[
    ClassifierSpec {
        id: "kaloscope-2.0", label: "Kaloscope 2.0", image_size: 448,
        description: "画师分类，39,261 类；使用社区转换的 ONNX 权重，CPU 推理。分数是单标签 softmax 概率。",
        files: &[
            ModelFile { name: "kaloscope_2-0.onnx", url: "https://huggingface.co/DraconicDragon/Kaloscope-onnx/resolve/9943edc1c2baaa6b496e1e71be8f161cb7be8252/v2.0/kaloscope_2-0.onnx", bytes: 733886867, sha256: Some("b50ab39b8986a2da9c4070226fb7f1009a34896ddf3c316156a84c64b0433219") },
            ModelFile { name: "class_mapping.csv", url: "https://huggingface.co/DraconicDragon/Kaloscope-onnx/resolve/9943edc1c2baaa6b496e1e71be8f161cb7be8252/v2.0/class_mapping.csv", bytes: 750182, sha256: Some("45aa78dacd9751de1c7a7293237845072c093dc915b72f1d4b5597ea2ff92cd4") },
            ModelFile { name: "config.json", url: "https://huggingface.co/DraconicDragon/Kaloscope-onnx/resolve/9943edc1c2baaa6b496e1e71be8f161cb7be8252/v2.0/config.json", bytes: 38, sha256: Some("1ddd819555414a56bef4803b159d37d15e116817e5cb53487007798146511223") },
        ],
    },
    ClassifierSpec {
        id: "camie-tagger-v2", label: "Camie Tagger v2", image_size: 512,
        description: "官方二次元多标签模型，仅展示其中 7,007 个画师类别；CPU 推理。分数是独立 sigmoid 概率。",
        files: &[
            ModelFile { name: "camie-tagger-v2.onnx", url: "https://huggingface.co/Camais03/camie-tagger-v2/resolve/7d40c1b85b86ab4f607b2caf26b1b50c99db743e/camie-tagger-v2.onnx", bytes: 788983561, sha256: Some("ab0aaf253e3d546090001bec9bebc776c354ab6800f442ab9167af87b4a953ac") },
            ModelFile { name: "camie-tagger-v2-metadata.json", url: "https://huggingface.co/Camais03/camie-tagger-v2/resolve/7d40c1b85b86ab4f607b2caf26b1b50c99db743e/camie-tagger-v2-metadata.json", bytes: 7771946, sha256: Some("de9f962eb0fd86b7e30d0af4e8c7990205200d70e955d8ecae60f87d14eae66b") },
            ModelFile { name: "config.json", url: "https://huggingface.co/Camais03/camie-tagger-v2/resolve/7d40c1b85b86ab4f607b2caf26b1b50c99db743e/config.json", bytes: 527, sha256: Some("4fcb331e5b98c7b99649114a26ef677ed1aaeff0623da4693a74bce1ff99a433") },
        ],
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub model: String,
    pub label: String,
    pub description: String,
    pub download_size_bytes: u64,
    pub present: bool,
    pub dir: String,
    pub runtime_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtistHit {
    pub artist: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub model: String,
    pub label: String,
    pub device: String,
    pub query_ms: i64,
    pub hits: Vec<ArtistHit>,
}

fn spec(id: &str) -> AppResult<&'static ClassifierSpec> {
    CLASSIFIERS.iter().find(|s| s.id == id).ok_or_else(|| AppError::bad_request(format!("未知画师分类模型: {id}")))
}

fn model_dir(data_dir: &Path, spec: &ClassifierSpec) -> PathBuf { data_dir.join("models").join(spec.id) }

fn present(dir: &Path, spec: &ClassifierSpec) -> bool {
    spec.files.iter().all(|f| std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.bytes))
}

fn status(data_dir: &Path, spec: &ClassifierSpec) -> Status {
    let dir = model_dir(data_dir, spec);
    let runtime_ready = onnx_runtime::is_ready(data_dir);
    Status {
        model: spec.id.into(), label: spec.label.into(), description: spec.description.into(),
        download_size_bytes: spec.files.iter().map(|f| f.bytes).sum::<u64>() + if runtime_ready { 0 } else { onnx_runtime::download_size_bytes() },
        present: present(&dir, spec), dir: dir.to_string_lossy().into_owned(), runtime_ready,
    }
}

pub fn catalog(data_dir: &Path) -> Vec<Status> { CLASSIFIERS.iter().map(|s| status(data_dir, s)).collect() }

pub async fn download(http: &reqwest::Client, data_dir: &Path, id: &str, cancel: &CancellationToken, on_event: DownloadEmitter<'_>) -> AppResult<Status> {
    let spec = spec(id)?;
    let dir = model_dir(data_dir, spec);
    for file in spec.files { download_checked(http, file, &dir, cancel, on_event).await?; }
    onnx_runtime::ensure_downloaded(http, data_dir, cancel, on_event).await?;
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    Ok(status(data_dir, spec))
}

/// RGB、CHW、float32，保持与各模型公开预处理一致的尺寸、裁剪/补边和 ImageNet 标准化。
fn preprocess(image: &DynamicImage, spec: &ClassifierSpec) -> AppResult<Vec<f32>> {
    let (w, h) = (image.width(), image.height());
    if w == 0 || h == 0 { return Err(AppError::bad_request("查询图片尺寸为空")); }
    let rgb = image.to_rgb8();
    let size = spec.image_size;
    let square = if spec.id == "kaloscope-2.0" {
        // timm crop_pct=.9；短边缩到497再中心裁448，避免将非正方图直接拉伸。
        let shorter = (size as f64 / 0.9).floor() as u32;
        let (nw, nh) = if w <= h { (shorter, (h as u64 * shorter as u64 / w as u64) as u32) }
            else { ((w as u64 * shorter as u64 / h as u64) as u32, shorter) };
        if nw as u64 * nh as u64 > 100_000_000 { return Err(AppError::bad_request("图片长宽比过大，无法用于画师分类")); }
        let resized = imageops::resize(&rgb, nw, nh, imageops::FilterType::CatmullRom);
        // torchvision CenterCrop 使用 Python round（ties-to-even）。
        let offset = |length: u32| { let d = length - size; let floor = d / 2; if d % 2 == 1 && floor % 2 == 1 { floor + 1 } else { floor } };
        imageops::crop_imm(&resized, offset(nw), offset(nh), size, size).to_image()
    } else {
        let (nw, nh) = if w > h { (size, ((h as u64 * size as u64 / w as u64) as u32).max(1)) }
            else { (((w as u64 * size as u64 / h as u64) as u32).max(1), size) };
        let resized = imageops::resize(&rgb, nw, nh, imageops::FilterType::Lanczos3);
        let mut padded = RgbImage::from_pixel(size, size, Rgb([124, 116, 104]));
        imageops::replace(&mut padded, &resized, ((size - nw) / 2) as i64, ((size - nh) / 2) as i64);
        padded
    };
    let plane = (size * size) as usize;
    let mut tensor = vec![0.0; plane * 3];
    for (i, pixel) in square.pixels().enumerate() {
        for channel in 0..3 { tensor[channel * plane + i] = (pixel[channel] as f32 / 255.0 - MEAN[channel]) / STD[channel]; }
    }
    Ok(tensor)
}

struct LabelMapping { output_len: usize, artists: Vec<(usize, String)> }

fn load_labels(dir: &Path, spec: &ClassifierSpec) -> AppResult<LabelMapping> {
    if spec.id == "kaloscope-2.0" {
        let mut reader = csv::Reader::from_path(dir.join("class_mapping.csv")).map_err(|e| AppError::new("model", format!("画师映射读取失败: {e}")))?;
        #[derive(serde::Deserialize)] struct Row { class_id: usize, class_name: String }
        let mut artists = Vec::new();
        for row in reader.deserialize::<Row>() {
            let row = row.map_err(|e| AppError::new("model", format!("画师映射格式错误: {e}")))?;
            artists.push((row.class_id, row.class_name));
        }
        artists.sort_by_key(|(id, _)| *id);
        if artists.is_empty() || artists.iter().enumerate().any(|(i, (id, name))| i != *id || name.trim().is_empty()) {
            return Err(AppError::new("model", "Kaloscope 画师映射有缺失或重复类别"));
        }
        Ok(LabelMapping { output_len: artists.len(), artists })
    } else {
        let meta: serde_json::Value = serde_json::from_reader(std::fs::File::open(dir.join("camie-tagger-v2-metadata.json"))?)?;
        let dataset = &meta["dataset_info"];
        let count = dataset["total_tags"].as_u64().ok_or_else(|| AppError::new("model", "Camie 元数据缺少 total_tags"))? as usize;
        let tags = dataset["tag_mapping"]["idx_to_tag"].as_object().ok_or_else(|| AppError::new("model", "Camie 元数据缺少 idx_to_tag"))?;
        let categories = &dataset["tag_mapping"]["tag_to_category"];
        let mut artists = Vec::new();
        for (id, tag) in tags {
            let name = tag.as_str().ok_or_else(|| AppError::new("model", "Camie 标签名称格式错误"))?;
            let idx: usize = id.parse().map_err(|_| AppError::new("model", "Camie 标签索引格式错误"))?;
            if idx >= count { return Err(AppError::new("model", "Camie 标签索引超出类别数量")); }
            if categories[name].as_str() == Some("artist") { artists.push((idx, name.to_string())); }
        }
        if artists.is_empty() || tags.len() != count { return Err(AppError::new("model", "Camie 画师映射不完整")); }
        artists.sort_by_key(|(id, _)| *id);
        Ok(LabelMapping { output_len: count, artists })
    }
}

fn artist_scores(logits: &[f32], labels: &LabelMapping, softmax: bool, top_k: usize) -> AppResult<Vec<ArtistHit>> {
    if logits.len() != labels.output_len || logits.iter().any(|x| !x.is_finite()) {
        return Err(AppError::new("model", "分类输出维度不匹配或包含非有限数值"));
    }
    let maximum = if softmax { logits.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64 } else { 0.0 };
    let denominator = if softmax { logits.iter().map(|x| (*x as f64 - maximum).exp()).sum::<f64>() } else { 1.0 };
    let mut hits: Vec<ArtistHit> = labels.artists.iter().map(|(idx, name)| {
        let x = logits[*idx] as f64;
        let score = if softmax { (x - maximum).exp() / denominator }
            else if x >= 0.0 { 1.0 / (1.0 + (-x).exp()) } else { let e = x.exp(); e / (1.0 + e) };
        ArtistHit { artist: name.clone(), score }
    }).collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.artist.cmp(&b.artist)));
    hits.truncate(top_k);
    Ok(hits)
}

struct CachedSession { model_dir: PathBuf, session: Session, labels: LabelMapping }
static SESSION: OnceLock<Mutex<Option<CachedSession>>> = OnceLock::new();

/// 在启动向量模型前释放分类模型；分类自身也只缓存最后使用的一种模型。
pub fn unload() { if let Some(cache) = SESSION.get() { *cache.lock() = None; } }

/// 同步 CPU 推理，调用方放到 spawn_blocking；top_k 最多50，仅展示画师类别。
pub fn classify(data_dir: &Path, id: &str, image: &DynamicImage, top_k: i64) -> AppResult<Classification> {
    let started = Instant::now();
    let spec = spec(id)?;
    let dir = model_dir(data_dir, spec);
    if !present(&dir, spec) { return Err(AppError::bad_request(format!("{} 尚未下载完整，请先下载模型", spec.label))); }
    let input = preprocess(image, spec)?;
    onnx_runtime::initialize(data_dir)?;
    let mut cache = SESSION.get_or_init(|| Mutex::new(None)).lock();
    if cache.as_ref().is_some_and(|c| c.model_dir != dir) { *cache = None; }
    if cache.is_none() {
        let labels = load_labels(&dir, spec)?;
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(4);
        let session = Session::builder().and_then(|b| b.with_intra_threads(threads))
            .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
            .and_then(|b| b.commit_from_file(dir.join(spec.files[0].name)))
            .map_err(|e| AppError::new("model", format!("{} 加载失败: {e}", spec.label)))?;
        *cache = Some(CachedSession { model_dir: dir, session, labels });
    }
    let cached = cache.as_mut().expect("classifier cache populated");
    let tensor = Tensor::from_array(([1usize, 3, spec.image_size as usize, spec.image_size as usize], input.into_boxed_slice()))
        .map_err(|e| AppError::new("model", format!("分类输入失败: {e}")))?;
    let outputs = cached.session.run(ort::inputs![tensor]).map_err(|e| AppError::new("model", format!("{} 推理失败: {e}", spec.label)))?;
    // Camie 发布图的第2个输出是 refined_predictions，不可误用第1个 initial_predictions。
    let output_index = if spec.id == "camie-tagger-v2" { 1 } else { 0 };
    if outputs.len() <= output_index { return Err(AppError::new("model", "分类模型缺少预期输出")); }
    let (shape, logits) = outputs[output_index].try_extract_tensor::<f32>().map_err(|e| AppError::new("model", format!("分类输出读取失败: {e}")))?;
    if shape.len() != 2 || shape[0] != 1 { return Err(AppError::new("model", format!("分类输出形状错误: {shape:?}"))); }
    let hits = artist_scores(logits, &cached.labels, spec.id == "kaloscope-2.0", if top_k <= 0 { 10 } else { top_k.min(50) as usize })?;
    Ok(Classification { model: spec.id.into(), label: spec.label.into(), device: "cpu".into(), query_ms: started.elapsed().as_millis() as i64, hits })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessing_preserves_aspect_ratio_with_model_specific_crop_or_padding() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(120, 240, Rgb([255, 0, 0])));
        let kalo = preprocess(&image, spec("kaloscope-2.0").unwrap()).unwrap();
        assert_eq!(kalo.len(), 3 * 448 * 448);
        assert!((kalo[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-6);
        let camie = preprocess(&image, spec("camie-tagger-v2").unwrap()).unwrap();
        assert_eq!(camie.len(), 3 * 512 * 512);
        assert!((camie[0] - (124.0 / 255.0 - MEAN[0]) / STD[0]).abs() < 1e-6, "Camie补边用均值色");
        assert!((camie[256 * 512 + 256] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-6, "图像居中");
    }

    #[test]
    fn output_probabilities_filter_artists_without_changing_probability_semantics() {
        let labels = LabelMapping { output_len: 4, artists: vec![(1, "a".into()), (3, "b".into())] };
        let scores = artist_scores(&[1000.0, 0.0, -1.0, 2.0], &labels, false, 5).unwrap();
        assert_eq!(scores.iter().map(|h| h.artist.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!((scores[1].score - 0.5).abs() < 1e-12);
        assert!((scores[0].score - 0.8807970779).abs() < 1e-9);
        let all = LabelMapping { output_len: 2, artists: vec![(0, "a".into()), (1, "b".into())] };
        let scores = artist_scores(&[1000.0, 1001.0], &all, true, 2).unwrap();
        assert!((scores.iter().map(|h| h.score).sum::<f64>() - 1.0).abs() < 1e-12, "稳定softmax且不溢出");
        assert!(artist_scores(&[f32::NAN, 1.0], &all, true, 2).is_err());
        assert!(artist_scores(&[f32::INFINITY, 1.0], &all, false, 2).is_err());
        assert!(artist_scores(&[f32::NEG_INFINITY, 1.0], &all, false, 2).is_err());
        assert!(artist_scores(&[1.0], &all, true, 2).is_err());
    }

    #[test]
    fn classifier_ids_are_allowlisted_and_catalog_needs_full_download() {
        let dir = tempfile::tempdir().unwrap();
        assert!(spec("../weights").is_err());
        let statuses = catalog(dir.path());
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|s| !s.present && !s.runtime_ready));
    }

    /// 用离线缓存运行真正的两个ONNX模型。环境变量缺省跳过，不在常规测试中联网或下载权重。
    /// CLASSIFIER_TEST_DATA_DIR 指向含 models/<id>/ 和已安装runtime的数据目录。
    #[test]
    fn official_classifier_models_cpu_smoke_opt_in() {
        let Ok(data_dir) = std::env::var("CLASSIFIER_TEST_DATA_DIR") else { return; };
        let data_dir = Path::new(&data_dir);
        let reference_dir = data_dir.join("reference");
        for spec in CLASSIFIERS {
            let labels = load_labels(&model_dir(data_dir, spec), spec).unwrap();
            assert_eq!(labels.artists.len(), if spec.id == "kaloscope-2.0" { 39261 } else { 7007 });
            let mut results = Vec::new();
            for index in 0..2 {
                let image = image::open(reference_dir.join(format!("input-{index}.png"))).unwrap();
                let result = classify(data_dir, spec.id, &image, 5).unwrap();
                assert_eq!(result.hits.len(), 5);
                assert!(result.hits.iter().all(|h| h.score.is_finite() && (0.0..=1.0).contains(&h.score)));
                assert!(result.hits.windows(2).all(|h| h[0].score >= h[1].score));
                assert_eq!(result.device, "cpu");
                eprintln!("{} input {index}: {}", spec.id, serde_json::to_string(&result).unwrap());
                results.push(result);
                // Pillow参考预处理与Rust重采样仅容许插值量化差异，不容许通道/补边/归一参数错误。
                let reference = std::fs::read(reference_dir.join(format!("{}-{index}-input.f32", spec.id))).unwrap();
                let expected = reference.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
                let actual = preprocess(&image, spec).unwrap();
                assert_eq!(actual.len(), expected.len());
                let mean_error = actual.iter().zip(&expected).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / actual.len() as f64;
                eprintln!("{} preprocessing mean absolute error: {mean_error}", spec.id);
                assert!(mean_error < 0.005, "{}预处理偏离Pillow参考: {mean_error}", spec.id);
            }
            assert_ne!(serde_json::to_value(&results[0].hits).unwrap(), serde_json::to_value(&results[1].hits).unwrap());
            unload();
        }
    }
}
