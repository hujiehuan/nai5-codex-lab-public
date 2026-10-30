//! CLIP ViT-B/32 图像塔（CONTRACT §12.0 / §12.3）：candle F32 推理，支持 CPU 与 macOS Metal。
//! 权重目录 `<data_dir>/models/clip-vit-base-patch32/` 默认下载 HF `openai/clip-vit-base-patch32` 的原文件：
//! `config.json`（只读 `vision_config` 与 `projection_dim` 定形状，缺键按 candle 的 `ClipVisionConfig::vit_base_patch32()`）与
//! `pytorch_model.bin`（键名 `vision_model.*` / `visual_projection.weight`，`VarBuilder::from_pth`，纯 Rust 读取）。
//! 兼容已手动放入的 `model.safetensors`；官方 main 不提供此格式，不能用它作为默认下载文件。
//! 只建图像塔 + 投影，不建文本塔（`candle_transformers::models::clip::ClipModel::new` 会把文本塔也建出来，白占一半内存与加载时间；
//! 图像塔与 `get_image_features` 的算法完全一样：`ClipVisionTransformer` 的 pooled（CLS 经 post_layernorm）→ `visual_projection`）。
//! 预处理（§12.3）：解码后短边缩放到 224（Triangle / 双线性）、中心裁 224×224、RGB、CLIP 的 mean/std 归一；输出 512 维 L2 归一。
//! 测试用随机权重（`VarBuilder::from_varmap`，各层按 candle 的初始化提示随机；`VarBuilder::zeros` 全零权重会让输出恒为零向量、
//! 归一后 NaN，验不了形状之外的东西）实例化模型，设了 `CLIP_MODEL_DIR` 才跑真权重。

use std::path::{Path, PathBuf};

use candle_core::{DType, Device, Module, Tensor};
use candle_nn::{Linear, VarBuilder};
use candle_transformers::models::clip::vision_model::{ClipVisionConfig, ClipVisionTransformer};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::{StyleDevice, StyleDevicePreference};

/// 缺省模型名（`app_state('style_model')` 缺省值，也是权重子目录名）。
pub const MODEL_NAME: &str = "clip-vit-base-patch32";
/// 投影维度（§12.0：512 维）。
pub const DIM: usize = 512;
/// 输入边长。
pub const IMAGE_SIZE: usize = 224;
/// CLIP 的归一化常数（RGB 顺序）。
pub const CLIP_MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
pub const CLIP_STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];
pub const CONFIG_FILE: &str = "config.json";
pub const WEIGHTS_FILE: &str = "pytorch_model.bin";
pub const SAFETENSORS_FILE: &str = "model.safetensors";
/// 要下载 / 要在场的两个文件（§12.0）。
pub const MODEL_FILES: [&str; 2] = [CONFIG_FILE, WEIGHTS_FILE];
/// 一批多少张（§12.3「批量 16 张一批」）。
pub const BATCH_SIZE: usize = 16;

/// `<data_dir>/models/<model>`。
pub fn model_dir(data_dir: &Path, model: &str) -> PathBuf {
    data_dir.join("models").join(model)
}

fn nonempty_file(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
}

/// 优先官方 PyTorch 权重，其次兼容已有 safetensors；空文件和 `.part` 不算完成。
pub fn weights_path(dir: &Path) -> Option<PathBuf> {
    [WEIGHTS_FILE, SAFETENSORS_FILE].iter().map(|f| dir.join(f)).find(|p| nonempty_file(p))
}

/// 配置与至少一种权重都已落地且非空（`.part` 不算）。
pub fn is_present(dir: &Path) -> bool {
    nonempty_file(&dir.join(CONFIG_FILE)) && weights_path(dir).is_some()
}

/// 已落地文件的总字节数（`.part` 不算）。
pub fn size_bytes(dir: &Path) -> u64 {
    std::iter::once(dir.join(CONFIG_FILE)).chain(weights_path(dir)).filter_map(|p| std::fs::metadata(p).ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

fn cfg_num(v: Option<&Value>, key: &str, default: usize) -> usize {
    v.and_then(|c| c.get(key)).and_then(Value::as_u64).map(|n| n as usize).unwrap_or(default)
}

/// HF `config.json` → 图像塔形状；缺键按 candle 内置的 ViT-B/32 缺省，`projection_dim` 先看顶层再看 `vision_config`。
pub fn vision_config_from_json(v: &Value) -> ClipVisionConfig {
    let base = ClipVisionConfig::vit_base_patch32();
    let vc = v.get("vision_config");
    ClipVisionConfig {
        embed_dim: cfg_num(vc, "hidden_size", base.embed_dim),
        intermediate_size: cfg_num(vc, "intermediate_size", base.intermediate_size),
        num_hidden_layers: cfg_num(vc, "num_hidden_layers", base.num_hidden_layers),
        num_attention_heads: cfg_num(vc, "num_attention_heads", base.num_attention_heads),
        projection_dim: v.get("projection_dim").and_then(Value::as_u64).map(|n| n as usize).unwrap_or_else(|| cfg_num(vc, "projection_dim", base.projection_dim)),
        num_channels: cfg_num(vc, "num_channels", base.num_channels),
        image_size: cfg_num(vc, "image_size", base.image_size),
        patch_size: cfg_num(vc, "patch_size", base.patch_size),
        ..base
    }
}

/// 自动模式只有在 Metal 加载及预热都成功后才使用 GPU；强制 Metal 的错误直接返回。
/// 注入加载器让无 GPU 的测试环境也能覆盖回退规则。
fn load_preferred<T>(
    preference: StyleDevicePreference,
    metal: impl FnOnce() -> AppResult<T>,
    cpu: impl FnOnce() -> AppResult<T>,
) -> AppResult<(T, Option<String>)> {
    match preference {
        StyleDevicePreference::Cpu => cpu().map(|encoder| (encoder, None)),
        StyleDevicePreference::Metal => metal().map(|encoder| (encoder, None)),
        StyleDevicePreference::Auto => match metal() {
            Ok(encoder) => Ok((encoder, None)),
            Err(metal_error) => {
                let reason = format!("Metal 未启用，已改用 CPU：{}", metal_error.message);
                let encoder = cpu().map_err(|cpu_error| AppError::new(&cpu_error.code, format!("{}；Metal 尝试也失败：{}", cpu_error.message, metal_error.message)))?;
                Ok((encoder, Some(reason)))
            }
        },
    }
}

/// Candle 0.11 的 Metal 初始化会直接使用 macOS 15 的 residency set，且空设备列表会越界。
/// 必须在进入 Candle 前拒绝这两种情况，错误才能交给自动模式正常回退。
#[cfg(any(target_os = "macos", test))]
fn check_metal_requirements(supported_os: bool, has_device: impl FnOnce() -> bool) -> AppResult<()> {
    if !supported_os {
        return Err(AppError::new("model", "本版本 Metal 加速需要 macOS 15 或更新版本，请选择自动或 CPU 模式"));
    }
    if !has_device() {
        return Err(AppError::new("model", "未检测到可用的 Metal GPU，请选择自动或 CPU 模式"));
    }
    Ok(())
}

/// 图像塔 + 投影。`Tensor` 是 Send + Sync，可以放进 `Arc` 跨 `spawn_blocking` 共用。
enum Backend {
    Clip { vision: ClipVisionTransformer, proj: Linear, cfg: ClipVisionConfig },
    Csd(super::csd::CsdEncoder),
    MegaStyle(super::megastyle::MegaStyleEncoder),
}

pub struct Encoder {
    backend: Backend,
    device: Device,
    fallback_reason: Option<String>,
}

impl Encoder {
    /// 保留显式 CPU 加载入口，供基线测试使用。
    pub fn load(dir: &Path) -> AppResult<Encoder> {
        Self::load_on_device(dir, Device::Cpu)
    }

    /// 按设置加载（阻塞：按需读取图像塔张量，约 350 MB；调用方放 `spawn_blocking`）。
    pub fn load_with_preference(dir: &Path, preference: StyleDevicePreference) -> AppResult<Encoder> {
        let (mut encoder, fallback_reason) = load_preferred(preference, || Self::load_metal(dir), || Self::load(dir))?;
        encoder.fallback_reason = fallback_reason;
        Ok(encoder)
    }

    #[cfg(target_os = "macos")]
    fn load_metal(dir: &Path) -> AppResult<Encoder> {
        check_metal_requirements(objc2::available!(macos = 15.0), || !objc2_metal::MTLCopyAllDevices().is_empty())?;
        let device = Device::new_metal(0).map_err(|e| AppError::new("model", format!("无法初始化 Metal：{e}")))?;
        let encoder = Self::load_on_device(dir, device).map_err(|e| AppError::new("model", format!("Metal 加载画风模型失败：{}", e.message)))?;
        // 完整走一遍批量图像塔并读取结果：能创建 Metal 设备并不代表所有算子都能运行。
        // 两张图片覆盖批量矩阵乘法；只在新加载模型时预热，后续索引复用此实例。
        let probe = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(encoder.image_size() as u32, encoder.image_size() as u32, image::Rgb([127, 127, 127])));
        let rows = encoder.embed_batch(&[probe.clone(), probe]).map_err(|e| AppError::new("model", format!("Metal 试算失败：{}", e.message)))?;
        if rows.len() != 2 || rows.iter().any(|row| {
            let squared_norm = row.iter().map(|v| v * v).sum::<f32>();
            row.len() != encoder.dim() || row.iter().any(|v| !v.is_finite()) || !squared_norm.is_finite() || (squared_norm - 1.0).abs() > 1e-3
        }) {
            return Err(AppError::new("model", "Metal 试算未得到有效的画风向量"));
        }
        Ok(encoder)
    }

    #[cfg(not(target_os = "macos"))]
    fn load_metal(_dir: &Path) -> AppResult<Encoder> {
        Err(AppError::bad_request("当前系统不支持 Metal，请选择自动或 CPU；Metal 加速需要 macOS"))
    }

    fn load_on_device(dir: &Path, device: Device) -> AppResult<Encoder> {
        let backend = match dir.file_name().and_then(|s| s.to_str()) {
            Some("csd-vit-large-patch14") => Some(Backend::Csd(super::csd::CsdEncoder::load(dir, &device)?)),
            Some("megastyle-siglip-so400m") => Some(Backend::MegaStyle(super::megastyle::MegaStyleEncoder::load(dir, &device)?)),
            _ => None,
        };
        if let Some(backend) = backend { return Ok(Encoder { backend, device, fallback_reason: None }); }
        let cfg_path = dir.join(CONFIG_FILE);
        if !is_present(dir) {
            return Err(AppError::bad_request(format!("画风模型未下载：{} 需要非空 {} 和 {}（或 {}）", dir.display(), CONFIG_FILE, WEIGHTS_FILE, SAFETENSORS_FILE)));
        }
        let weights = weights_path(dir).ok_or_else(|| AppError::bad_request("画风模型权重文件已被移走，请重新下载"))?;
        let raw = std::fs::read_to_string(&cfg_path)?;
        let v: Value = serde_json::from_str(&raw).map_err(|e| AppError::new("model", format!("{}: {e}", cfg_path.display())))?;
        let cfg = vision_config_from_json(&v);
        let vb = if weights.file_name().and_then(|f| f.to_str()) == Some(WEIGHTS_FILE) {
            VarBuilder::from_pth(&weights, DType::F32, &device)?
        } else {
            // SAFETY：mmap 只读；文件在加载期间被改写属于用户自己动模型目录，与 candle 示例同样的前提。
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device) }?
        };
        Self::build(vb, cfg)
    }

    /// 随机权重（测试用）：各层按 candle 的初始化提示（Kaiming / randn / 常数）随机，输出不退化。
    pub fn random(cfg: ClipVisionConfig) -> AppResult<Encoder> {
        let varmap = candle_nn::VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &Device::Cpu);
        Self::build(vb, cfg)
    }

    fn build(vb: VarBuilder, cfg: ClipVisionConfig) -> AppResult<Encoder> {
        let device = vb.device().clone();
        let vision = ClipVisionTransformer::new(vb.pp("vision_model"), &cfg)?;
        let proj = candle_nn::linear_no_bias(cfg.embed_dim, cfg.projection_dim, vb.pp("visual_projection"))?;
        Ok(Encoder { backend: Backend::Clip { vision, proj, cfg }, device, fallback_reason: None })
    }

    pub fn device_kind(&self) -> StyleDevice {
        if self.device.is_metal() { StyleDevice::Metal } else { StyleDevice::Cpu }
    }

    pub fn fallback_reason(&self) -> Option<&str> {
        self.fallback_reason.as_deref()
    }

    pub fn dim(&self) -> usize {
        match &self.backend { Backend::Clip { cfg, .. } => cfg.projection_dim, Backend::Csd(e) => e.dim(), Backend::MegaStyle(e) => e.dim() }
    }

    pub fn image_size(&self) -> usize {
        match &self.backend { Backend::Clip { cfg, .. } => cfg.image_size, Backend::Csd(e) => e.image_size(), Backend::MegaStyle(e) => e.image_size() }
    }

    /// 一批图片 → 每张一个 L2 归一的向量（`dim()` 维）。空批返回空。阻塞，调用方放 `spawn_blocking`。
    pub fn embed_batch(&self, images: &[DynamicImage]) -> AppResult<Vec<Vec<f32>>> {
        if images.is_empty() {
            return Ok(vec![]);
        }
        match &self.backend {
            Backend::Clip { vision, proj, cfg } => {
                let x = pixel_tensor_on_device(images, cfg.image_size, &self.device)?;
                let feats = vision.forward(&x)?.apply(proj)?;
                let rows = feats.to_vec2::<f32>()?;
                Ok(rows.iter().map(|r| l2_normalize(r)).collect())
            },
            Backend::Csd(e) => e.embed_batch(images),
            Backend::MegaStyle(e) => e.embed_batch(images),
        }
    }
}

/// §12.3 预处理：短边缩放到 `size`（另一边等比、至少 `size`，Triangle 滤波），中心裁 `size×size`，RGB，CLIP mean/std 归一；
/// 返回 CHW 排布的 `3 × size × size` 个 f32。
pub fn preprocess(img: &DynamicImage, size: u32) -> Vec<f32> {
    let (w, h) = img.dimensions();
    let (w, h) = (w.max(1), h.max(1));
    let (nw, nh) = if w <= h {
        (size, ((h as f64 * size as f64 / w as f64).round() as u32).max(size))
    } else {
        (((w as f64 * size as f64 / h as f64).round() as u32).max(size), size)
    };
    let resized = img.resize_exact(nw, nh, FilterType::Triangle);
    let x0 = (nw - size) / 2;
    let y0 = (nh - size) / 2;
    let rgb = resized.crop_imm(x0, y0, size, size).to_rgb8();
    let n = (size * size) as usize;
    let mut out = vec![0f32; 3 * n];
    for (i, px) in rgb.pixels().enumerate() {
        for c in 0..3 {
            out[c * n + i] = (px[c] as f32 / 255.0 - CLIP_MEAN[c]) / CLIP_STD[c];
        }
    }
    out
}

/// 一批图片 → `[N, 3, size, size]` 张量。
pub fn pixel_tensor(images: &[DynamicImage], size: usize) -> AppResult<Tensor> {
    pixel_tensor_on_device(images, size, &Device::Cpu)
}

fn pixel_tensor_on_device(images: &[DynamicImage], size: usize, device: &Device) -> AppResult<Tensor> {
    let per = 3 * size * size;
    let mut data = Vec::with_capacity(images.len() * per);
    for img in images {
        data.extend(preprocess(img, size as u32));
    }
    Ok(Tensor::from_vec(data, (images.len(), 3, size, size), device)?)
}

/// L2 归一；范数为 0（全零权重）时原样返回零向量而不是 NaN。
pub fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= 1e-12 || !norm.is_finite() {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 128])))
    }

    #[test]
    fn automatic_device_uses_metal_or_reports_cpu_fallback() {
        let (device, reason) = load_preferred(StyleDevicePreference::Auto, || Ok("metal"), || panic!("Metal 可用时不加载 CPU")).unwrap();
        assert_eq!(device, "metal");
        assert!(reason.is_none());

        let (device, reason) = load_preferred(StyleDevicePreference::Auto, || Err(AppError::new("model", "试算算子不可用")), || Ok("cpu")).unwrap();
        assert_eq!(device, "cpu");
        assert!(reason.unwrap().contains("试算算子不可用"));

        let error = load_preferred::<()>(StyleDevicePreference::Auto, || Err(AppError::new("model", "GPU 内存不足")), || Err(AppError::io("权重不可读"))).unwrap_err();
        assert_eq!(error.code, "io", "CPU 加载也失败时保留真正的错误，不能报告已回退成功");
        assert!(error.message.contains("权重不可读") && error.message.contains("GPU 内存不足"));
    }

    #[test]
    fn explicit_devices_do_not_silently_switch_backend() {
        let (device, reason) = load_preferred(StyleDevicePreference::Cpu, || panic!("显式 CPU 不探测 Metal"), || Ok("cpu")).unwrap();
        assert_eq!(device, "cpu");
        assert!(reason.is_none());

        let error = load_preferred::<()>(StyleDevicePreference::Metal, || Err(AppError::new("model", "Metal 试算失败")), || panic!("显式 Metal 不回退 CPU")).unwrap_err();
        assert_eq!(error.message, "Metal 试算失败");
    }

    #[test]
    fn metal_preflight_rejects_old_os_before_inventory_and_empty_devices() {
        let error = check_metal_requirements(false, || panic!("旧 macOS 不调用 Metal API")).unwrap_err();
        assert!(error.message.contains("macOS 15"));
        let error = check_metal_requirements(true, || false).unwrap_err();
        assert!(error.message.contains("未检测到") && error.message.contains("CPU"));
        assert!(check_metal_requirements(true, || true).is_ok());

        let (device, reason) = load_preferred(StyleDevicePreference::Auto, || check_metal_requirements(true, || false).map(|_| "metal"), || Ok("cpu")).unwrap();
        assert_eq!(device, "cpu", "空设备列表正常回退，不能进入 Candle 的越界初始化");
        assert!(reason.unwrap().contains("未检测到"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn metal_on_other_platforms_explains_how_to_recover() {
        let dir = tempfile::tempdir().unwrap();
        let error = Encoder::load_with_preference(dir.path(), StyleDevicePreference::Metal).err().unwrap();
        assert!(error.message.contains("macOS") && error.message.contains("CPU"));
    }

    #[test]
    fn detects_official_and_legacy_weights_without_counting_parts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        std::fs::write(path.join(CONFIG_FILE), b"{}").unwrap();
        std::fs::write(path.join(format!("{WEIGHTS_FILE}.part")), b"partial").unwrap();
        std::fs::write(path.join(WEIGHTS_FILE), b"").unwrap();
        assert!(!is_present(path));
        assert_eq!(size_bytes(path), 2);
        assert!(Encoder::load(path).is_err());

        std::fs::write(path.join(SAFETENSORS_FILE), b"legacy").unwrap();
        assert!(is_present(path));
        assert_eq!(weights_path(path), Some(path.join(SAFETENSORS_FILE)));
        assert_eq!(size_bytes(path), 2 + 6);

        std::fs::write(path.join(WEIGHTS_FILE), b"official").unwrap();
        assert!(is_present(path));
        assert_eq!(weights_path(path), Some(path.join(WEIGHTS_FILE)));
        assert_eq!(size_bytes(path), 2 + 8, "只计算当前使用的权重");
        assert!(Encoder::load(path).is_err(), "假 bin 权重返回加载错误，不 panic");

        std::fs::write(path.join(CONFIG_FILE), b"").unwrap();
        assert!(!is_present(path), "空配置不算完成");
    }

    /// §12.3：横图 / 竖图 / 小图都出 3×224×224；归一化后纯白像素落在 (1-mean)/std、纯黑落在 -mean/std；张量形状 [N,3,224,224]。
    #[test]
    fn preprocess_gives_224_chw_with_clip_normalisation() {
        for (w, h) in [(640, 480), (300, 900), (50, 40), (224, 224)] {
            let v = preprocess(&gradient(w, h), 224);
            assert_eq!(v.len(), 3 * 224 * 224, "{w}x{h}");
            assert!(v.iter().all(|x| x.is_finite()));
        }
        let white = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(10, 20, image::Rgb([255, 255, 255])));
        let v = preprocess(&white, 224);
        let n = 224 * 224;
        for c in 0..3 {
            let expect = (1.0 - CLIP_MEAN[c]) / CLIP_STD[c];
            assert!((v[c * n] - expect).abs() < 1e-5 && (v[c * n + n - 1] - expect).abs() < 1e-5, "通道 {c}");
        }
        let black = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(30, 10, image::Rgb([0, 0, 0])));
        let v = preprocess(&black, 224);
        for c in 0..3 {
            assert!((v[c * n + 7] + CLIP_MEAN[c] / CLIP_STD[c]).abs() < 1e-5);
        }
        // 中心裁：横向渐变图裁掉左右两侧 → 第一行中点附近的 R 通道约为中间灰度
        let wide = gradient(448, 224);
        let v = preprocess(&wide, 224);
        let r_left = v[0];
        let r_right = v[223];
        assert!(r_left < r_right, "裁剪后仍保留左右渐变方向");
        let t = pixel_tensor(&[gradient(64, 64), white], 224).unwrap();
        assert_eq!(t.dims(), &[2, 3, 224, 224]);
        assert_eq!(l2_normalize(&[3.0, 4.0]), vec![0.6, 0.8]);
        assert_eq!(l2_normalize(&[0.0, 0.0]), vec![0.0, 0.0], "零向量不产生 NaN");
    }

    /// §12.3：随机权重实例化整个图像塔 + 投影（真实 ViT-B/32 形状），两张图一批 → 两个 512 维单位向量，且两张不同图向量不同；
    /// `config.json` 的形状解析：缺键回缺省、顶层 projection_dim 优先。
    #[test]
    fn random_weight_model_embeds_batch_to_unit_vectors() {
        let cfg = vision_config_from_json(&serde_json::json!({ "projection_dim": 512, "vision_config": { "hidden_size": 768, "patch_size": 32, "image_size": 224 } }));
        assert_eq!((cfg.embed_dim, cfg.patch_size, cfg.image_size, cfg.projection_dim, cfg.num_hidden_layers), (768, 32, 224, 512, 12));
        let small = vision_config_from_json(&serde_json::json!({ "vision_config": { "projection_dim": 64, "num_hidden_layers": 2 } }));
        assert_eq!((small.projection_dim, small.num_hidden_layers, small.embed_dim), (64, 2, 768));
        let enc = Encoder::random(cfg).unwrap();
        assert_eq!((enc.dim(), enc.image_size()), (DIM, IMAGE_SIZE));
        assert_eq!(enc.device_kind(), StyleDevice::Cpu);
        assert!(enc.fallback_reason().is_none());
        let a = gradient(320, 200);
        let b = DynamicImage::ImageRgb8(image::RgbImage::from_fn(200, 320, |x, y| image::Rgb([((x * y) % 251) as u8, (y % 256) as u8, (x % 256) as u8])));
        let vecs = enc.embed_batch(&[a.clone(), b]).unwrap();
        assert_eq!(vecs.len(), 2);
        for v in &vecs {
            assert_eq!(v.len(), 512);
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-4, "L2 归一: {norm}");
            assert!(v.iter().all(|x| x.is_finite()));
        }
        let cos: f32 = vecs[0].iter().zip(&vecs[1]).map(|(x, y)| x * y).sum();
        assert!(cos < 0.9999, "两张不同图的向量应不同: cos={cos}");
        // 同一张图单独算与批量算一致（batch 维不串扰）
        let single = enc.embed_batch(&[a]).unwrap();
        let cos_same: f32 = single[0].iter().zip(&vecs[0]).map(|(x, y)| x * y).sum();
        assert!(cos_same > 0.999, "单张与批量结果一致: {cos_same}");
        assert!(enc.embed_batch(&[]).unwrap().is_empty());
    }

    /// 真权重健全性（§12.3）：设了 `CLIP_MODEL_DIR` 才跑——同一张图与自身相似度 > 0.99、不同图 < 0.99、维度 512、单位范数。
    #[test]
    fn real_weights_self_similarity_when_env_set() {
        let Ok(dir) = std::env::var("CLIP_MODEL_DIR") else { return };
        if dir.trim().is_empty() {
            return;
        }
        let dir = PathBuf::from(dir);
        assert!(is_present(&dir), "CLIP_MODEL_DIR 里应有非空 config.json 与 pytorch_model.bin（或 model.safetensors）");
        let enc = Encoder::load(&dir).unwrap();
        assert_eq!(enc.dim(), DIM);
        let a = gradient(400, 300);
        let b = DynamicImage::ImageRgb8(image::RgbImage::from_fn(300, 400, |x, y| image::Rgb([((x + y) % 256) as u8, 30, ((x * 3) % 256) as u8])));
        let v1 = enc.embed_batch(&[a.clone(), b]).unwrap();
        let v2 = enc.embed_batch(&[a]).unwrap();
        let cos = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
        assert!(cos(&v1[0], &v2[0]) > 0.99, "同图自相似 {}", cos(&v1[0], &v2[0]));
        assert!(cos(&v1[0], &v1[1]) < 0.99, "异图相似 {}", cos(&v1[0], &v1[1]));
        assert!((cos(&v1[0], &v1[0]) - 1.0).abs() < 1e-3);
        eprintln!("real weights ok: self={:.4} other={:.4} size={} bytes", cos(&v1[0], &v2[0]), cos(&v1[0], &v1[1]), size_bytes(&dir));
    }

    /// Mac 真机回归：`CLIP_METAL_SMOKE=1 CLIP_MODEL_DIR=... cargo test real_weights_metal_matches_cpu -- --nocapture`。
    /// 直接比较同一组图的 CPU / Metal F32 向量；未启用时不要求 CI 配备 GPU / 模型权重。
    #[cfg(target_os = "macos")]
    #[test]
    fn real_weights_metal_matches_cpu_when_enabled() {
        if std::env::var("CLIP_METAL_SMOKE").as_deref() != Ok("1") {
            return;
        }
        let dir = PathBuf::from(std::env::var("CLIP_MODEL_DIR").expect("启用 Metal 真机测试时需要 CLIP_MODEL_DIR"));
        let images = [gradient(400, 300), DynamicImage::ImageRgb8(image::RgbImage::from_fn(300, 400, |x, y| image::Rgb([((x + y) % 256) as u8, 30, ((x * 3) % 256) as u8])))];
        let cpu = Encoder::load_with_preference(&dir, StyleDevicePreference::Cpu).unwrap();
        let start = std::time::Instant::now();
        let expected = cpu.embed_batch(&images).unwrap();
        let cpu_elapsed = start.elapsed();
        drop(cpu);

        let metal = Encoder::load_with_preference(&dir, StyleDevicePreference::Metal).unwrap();
        assert_eq!(metal.device_kind(), StyleDevice::Metal);
        assert!(metal.fallback_reason().is_none());
        let start = std::time::Instant::now();
        let actual = metal.embed_batch(&images).unwrap();
        let metal_elapsed = start.elapsed();
        for (cpu, gpu) in expected.iter().zip(&actual) {
            assert_eq!(gpu.len(), DIM);
            assert!(gpu.iter().all(|value| value.is_finite()));
            let norm = gpu.iter().map(|value| value * value).sum::<f32>();
            let cosine = cpu.iter().zip(gpu).map(|(a, b)| a * b).sum::<f32>();
            assert!((norm - 1.0).abs() < 1e-4, "Metal 输出单位向量：{norm}");
            assert!(cosine > 0.999, "CPU / Metal 同图一致：{cosine}");
        }
        assert_eq!(actual.len(), expected.len());
        eprintln!("CLIP F32 CPU / Metal parity ok; 2 images: CPU {cpu_elapsed:?}, Metal {metal_elapsed:?} (after warmup)");
    }
}
