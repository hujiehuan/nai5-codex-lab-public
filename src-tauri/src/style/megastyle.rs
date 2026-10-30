//! MegaStyle 的 SigLIP SO400M 专用画风编码器。
//!
//! 对齐腾讯官方 `Tencent/MegaStyle/style_score.py`：加载作者发布的
//! `megastyle_encoder.pth` 的 `model` 字典，使用 SiglipVisionModel 的
//! attention pooling head 输出（pooler_output），最后 L2 归一为 1152 维。
//! 预处理来自 google/siglip-so400m-patch14-384 的 preprocessor_config：
//! RGB 直接双三次缩放到 384×384、除以 255、mean/std 均为 0.5，无中心裁剪。

use std::path::Path;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::siglip::{VisionConfig, VisionModel};
use image::{imageops::FilterType, DynamicImage, GenericImageView};

use crate::error::{AppError, AppResult};

use super::model::l2_normalize;

pub const WEIGHTS_FILE: &str = "megastyle_encoder.pth";
pub const DIM: usize = 1152;
pub const IMAGE_SIZE: usize = 384;

pub struct MegaStyleEncoder {
    vision: VisionModel,
    config: VisionConfig,
    device: Device,
}

impl MegaStyleEncoder {
    pub fn load(dir: &Path, device: &Device) -> AppResult<Self> {
        let path = dir.join(WEIGHTS_FILE);
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)
            .map_err(|e| AppError::new("model", format!("MegaStyle 配置无效：{e}")))?;
        let config: VisionConfig = serde_json::from_value(config.get("vision_config").cloned().ok_or_else(|| AppError::new("model", "MegaStyle 配置缺少 vision_config"))?)
            .map_err(|e| AppError::new("model", format!("MegaStyle 图像塔配置无效：{e}")))?;
        if config.hidden_size != DIM || config.image_size != IMAGE_SIZE || config.patch_size != 14 || config.num_hidden_layers != 27 || config.num_attention_heads != 16 {
            return Err(AppError::new("model", "MegaStyle 需要 SigLIP SO400M/14 384 的配置，请重新下载模型"));
        }
        let vb = VarBuilder::from_pth_with_state(&path, DType::F32, "model", device)
            .map_err(|e| AppError::new("model", format!("读取 MegaStyle 权重失败（{}）：{e}", path.display())))?;
        if !vb.contains_tensor("vision_model.head.probe") {
            return Err(AppError::new("model", "MegaStyle 权重缺少画风池化头，请重新下载作者发布的完整编码器"));
        }
        // true 启用作者训练的 attention pooling，不能取 token 平均或第一项。
        let vision = VisionModel::new(&config, true, vb.pp("vision_model"))?;
        Ok(Self { vision, config, device: device.clone() })
    }

    pub fn dim(&self) -> usize { self.config.hidden_size }

    pub fn image_size(&self) -> usize { self.config.image_size }

    pub fn embed_batch(&self, images: &[DynamicImage]) -> AppResult<Vec<Vec<f32>>> {
        if images.is_empty() { return Ok(vec![]); }
        let size = self.image_size();
        let mut pixels = Vec::with_capacity(images.len() * 3 * size * size);
        for image in images { pixels.extend(preprocess(image, size as u32)?); }
        let pixels = Tensor::from_vec(pixels, (images.len(), 3, size, size), &self.device)?;
        let projected = pixels.apply(&self.vision)?;
        let rows = projected.to_vec2::<f32>()?;
        let rows: Vec<_> = rows.iter().map(|row| l2_normalize(row)).collect();
        if rows.iter().any(|row| row.len() != self.dim() || row.iter().any(|v| !v.is_finite()) || row.iter().map(|v| v * v).sum::<f32>() < 0.99) {
            return Err(AppError::new("model", "MegaStyle 未生成有效的归一化画风向量"));
        }
        Ok(rows)
    }
}

fn preprocess(image: &DynamicImage, size: u32) -> AppResult<Vec<f32>> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || size == 0 { return Err(AppError::bad_request("MegaStyle 输入图片尺寸不能为空")); }
    let rgb = image.to_rgb8();
    let resized = image::imageops::resize(&rgb, size, size, FilterType::CatmullRom);
    let plane = (size * size) as usize;
    let mut out = vec![0.; 3 * plane];
    for (i, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 { out[channel * plane + i] = (pixel[channel] as f32 / 255. - 0.5) / 0.5; }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn megastyle_resizes_entire_image_and_normalizes_to_minus_one_one() {
        let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(8, 4, |x, _| image::Rgb([if x < 2 { 255 } else { 0 }, 0, 255])));
        let pixels = preprocess(&image, 4).unwrap();
        assert_eq!(pixels.len(), 3 * 4 * 4);
        assert!(pixels[0] > 0.5, "保留左边缘，不能按 CLIP 方式中心裁剪");
        assert_eq!(pixels[4 * 4], -1.);
        assert_eq!(pixels[2 * 4 * 4], 1.);
        assert!(preprocess(&DynamicImage::new_rgb8(0, 4), 4).is_err());
    }

    #[test]
    fn megastyle_real_weights_smoke_if_available() {
        let Some(dir) = std::env::var_os("MEGASTYLE_MODEL_DIR") else { return; };
        let encoder = MegaStyleEncoder::load(Path::new(&dir), &Device::Cpu).expect("加载 MegaStyle 官方权重");
        let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(480, 384, |x, y| image::Rgb([(x * 255 / 480) as u8, (y * 255 / 384) as u8, 128])));
        let rows = encoder.embed_batch(&[image.clone(), image]).expect("MegaStyle 真实权重编码");
        assert_eq!(rows[0].len(), DIM);
        assert!(rows[0].iter().zip(&rows[1]).map(|(a, b)| a * b).sum::<f32>() > 0.9999);
        // HF Transformers SiglipVisionModel.pooler_output，官方权重 SHA256 79d08343…
        // 相同渐变图与预处理的参考值，检测误用通用 SigLIP 权重或丢失池化头。
        let reference = [0.004187607, -0.025704188, -0.047100652, -0.0021600607, -0.03031861, -0.01668178, -0.040666766, -0.018782143];
        assert!(rows[0].iter().zip(reference).all(|(actual, expected)| (actual - expected).abs() < 2e-5));
    }
}
