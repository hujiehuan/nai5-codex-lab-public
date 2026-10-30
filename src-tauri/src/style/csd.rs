//! CSD ViT-L/14 的专用画风描述器（不是普通 CLIP 的投影）。
//!
//! 对齐官方 `learn2phoenix/CSD` 的 `CSD/model.py`、`models/clip/model.py`
//! 和 `CSD/loss_utils.py::transforms_branch0`：224 像素双三次缩放、CLIP
//! 标准化、24 层 OpenAI ViT-L/14、`last_layer_style` 投影、768 维 L2 归一。
//! HF `tomg-group-umd/CSD-ViT-L/pytorch_model.bin` 是训练 checkpoint；只读取
//! `model_state_dict`，不加载其优化器、content head，也不执行 Python pickle。
//! 上游代码 MIT，模型卡权重 CC-BY-4.0；上游 README 提示公开权重与论文指标有差异。

use std::path::Path;

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{Conv2d, Conv2dConfig, LayerNorm, Linear, VarBuilder};
use image::{imageops::FilterType, DynamicImage, GenericImageView};

use crate::error::{AppError, AppResult};

use super::model::{l2_normalize, CLIP_MEAN, CLIP_STD};

pub const WEIGHTS_FILE: &str = "pytorch_model.bin";
pub const DIM: usize = 768;
pub const IMAGE_SIZE: usize = 224;

#[derive(Clone, Copy)]
struct Config {
    image_size: usize,
    patch_size: usize,
    width: usize,
    layers: usize,
    heads: usize,
    dim: usize,
}

impl Config {
    const VIT_LARGE: Self = Self { image_size: IMAGE_SIZE, patch_size: 14, width: 1024, layers: 24, heads: 16, dim: DIM };
}

/// OpenAI CLIP 使用一个合并的 Q/K/V 权重；保持 checkpoint 原键名，避免将它
/// 当成 Hugging Face CLIP 权重加载或将三个投影的位置弄反。
struct ResidualBlock {
    ln_1: LayerNorm,
    qkv: Linear,
    out_proj: Linear,
    ln_2: LayerNorm,
    fc: Linear,
    mlp_proj: Linear,
    heads: usize,
    width: usize,
}

impl ResidualBlock {
    fn load(vb: VarBuilder, c: Config) -> AppResult<Self> {
        let attn = vb.pp("attn");
        let qkv = Linear::new(
            attn.get_with_hints((3 * c.width, c.width), "in_proj_weight", candle_nn::init::DEFAULT_KAIMING_NORMAL)?,
            Some(attn.get_with_hints(3 * c.width, "in_proj_bias", candle_nn::Init::Const(0.))?),
        );
        Ok(Self {
            ln_1: candle_nn::layer_norm(c.width, 1e-5, vb.pp("ln_1"))?,
            qkv,
            out_proj: candle_nn::linear(c.width, c.width, attn.pp("out_proj"))?,
            ln_2: candle_nn::layer_norm(c.width, 1e-5, vb.pp("ln_2"))?,
            fc: candle_nn::linear(c.width, 4 * c.width, vb.pp("mlp.c_fc"))?,
            mlp_proj: candle_nn::linear(4 * c.width, c.width, vb.pp("mlp.c_proj"))?,
            heads: c.heads,
            width: c.width,
        })
    }

    fn forward(&self, x: &Tensor) -> AppResult<Tensor> {
        let (batch, tokens, _) = x.dims3()?;
        let head_dim = self.width / self.heads;
        let qkv = x.apply(&self.ln_1)?.apply(&self.qkv)?;
        let head = |offset| -> candle_core::Result<Tensor> {
            qkv.narrow(2, offset, self.width)?.contiguous()?.reshape((batch, tokens, self.heads, head_dim))?.transpose(1, 2)?.contiguous()
        };
        let q = (head(0)? * (head_dim as f64).powf(-0.5))?;
        let k = head(self.width)?;
        let v = head(2 * self.width)?;
        let weights = candle_nn::ops::softmax_last_dim(&q.matmul(&k.transpose(2, 3)?)?)?;
        let attention = weights.matmul(&v)?.transpose(1, 2)?.contiguous()?.reshape((batch, tokens, self.width))?.apply(&self.out_proj)?;
        let x = (x + attention)?;
        let mlp = x.apply(&self.ln_2)?.apply(&self.fc)?;
        // OpenAI QuickGELU，不能替换成标准 GELU 或 tanh approximation。
        let mlp = (&mlp * candle_nn::ops::sigmoid(&(&mlp * 1.702)?)?)?.apply(&self.mlp_proj)?;
        Ok((x + mlp)?)
    }
}

pub struct CsdEncoder {
    patch: Conv2d,
    class: Tensor,
    position: Tensor,
    ln_pre: LayerNorm,
    blocks: Vec<ResidualBlock>,
    ln_post: LayerNorm,
    style_projection: Tensor,
    config: Config,
    device: Device,
}

impl CsdEncoder {
    pub fn load(dir: &Path, device: &Device) -> AppResult<Self> {
        let path = dir.join(WEIGHTS_FILE);
        let vb = VarBuilder::from_pth_with_state(&path, DType::F32, "model_state_dict", device)
            .map_err(|e| AppError::new("model", format!("读取 CSD 权重失败（{}）：{e}", path.display())))?;
        // 官方 HF checkpoint 有 DDP 的 module. 前缀；也兼容去掉前缀的同构导出。
        let vb = if vb.contains_tensor("module.last_layer_style") { vb.pp("module") } else { vb };
        if !vb.contains_tensor("last_layer_style") {
            return Err(AppError::new("model", "CSD checkpoint 缺少 last_layer_style 画风投影；请重新下载官方 CSD 权重"));
        }
        Self::build(vb, Config::VIT_LARGE)
    }

    fn build(vb: VarBuilder, config: Config) -> AppResult<Self> {
        let backbone = vb.pp("backbone");
        let std = (config.width as f64).powf(-0.5);
        let init = candle_nn::Init::Randn { mean: 0., stdev: std };
        let positions = (config.image_size / config.patch_size).pow(2) + 1;
        let mut blocks = Vec::with_capacity(config.layers);
        for i in 0..config.layers {
            blocks.push(ResidualBlock::load(backbone.pp(format!("transformer.resblocks.{i}")), config)?);
        }
        Ok(Self {
            patch: candle_nn::conv2d_no_bias(3, config.width, config.patch_size, Conv2dConfig { stride: config.patch_size, ..Default::default() }, backbone.pp("conv1"))?,
            class: backbone.get_with_hints(config.width, "class_embedding", init)?,
            position: backbone.get_with_hints((positions, config.width), "positional_embedding", init)?,
            ln_pre: candle_nn::layer_norm(config.width, 1e-5, backbone.pp("ln_pre"))?,
            blocks,
            ln_post: candle_nn::layer_norm(config.width, 1e-5, backbone.pp("ln_post"))?,
            // 官方 forward 是 feature @ last_layer_style，权重形状为 [1024,768]。
            // 不转置、不添加 bias，不使用 last_layer_content。
            style_projection: vb.get_with_hints((config.width, config.dim), "last_layer_style", init)?,
            config,
            device: vb.device().clone(),
        })
    }

    pub fn dim(&self) -> usize { self.config.dim }

    pub fn image_size(&self) -> usize { self.config.image_size }

    pub fn embed_batch(&self, images: &[DynamicImage]) -> AppResult<Vec<Vec<f32>>> {
        if images.is_empty() { return Ok(vec![]); }
        let size = self.image_size();
        let mut pixels = Vec::with_capacity(images.len() * 3 * size * size);
        for image in images {
            pixels.extend(preprocess(image, size as u32)?);
        }
        let pixels = Tensor::from_vec(pixels, (images.len(), 3, size, size), &self.device)?;
        let patches = pixels.apply(&self.patch)?.flatten_from(2)?.transpose(1, 2)?;
        let class = self.class.expand((images.len(), 1, self.config.width))?;
        let mut x = Tensor::cat(&[class, patches], 1)?.broadcast_add(&self.position)?.apply(&self.ln_pre)?;
        for block in &self.blocks { x = block.forward(&x)?; }
        let projected = x.i((.., 0, ..))?.contiguous()?.apply(&self.ln_post)?.matmul(&self.style_projection)?;
        let rows = projected.to_vec2::<f32>()?;
        let rows: Vec<_> = rows.iter().map(|row| l2_normalize(row)).collect();
        if rows.iter().any(|row| row.len() != self.dim() || row.iter().any(|v| !v.is_finite()) || row.iter().map(|v| v * v).sum::<f32>() < 0.99) {
            return Err(AppError::new("model", "CSD 未生成有效的归一化画风向量"));
        }
        Ok(rows)
    }
}

/// 与 torchvision 的整数短边 Resize / CenterCrop 对齐：长边向下取整；
/// 中心裁边界使用 Python round 的 ties-to-even。Catmull-Rom 对应 bicubic
/// a=-0.5，不能复用旧 CLIP 路径的 Triangle 双线性缩放。
fn preprocess(img: &DynamicImage, size: u32) -> AppResult<Vec<f32>> {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 || size == 0 { return Err(AppError::bad_request("CSD 输入图片尺寸不能为空")); }
    let (nw, nh) = if width <= height {
        (size, ((height as u64 * size as u64) / width as u64) as u32)
    } else {
        (((width as u64 * size as u64) / height as u64) as u32, size)
    };
    let rgb = img.to_rgb8();
    let resized = image::imageops::resize(&rgb, nw, nh, FilterType::CatmullRom);
    let center = |length: u32| {
        let delta = length - size;
        let floor = delta / 2;
        floor + u32::from(delta % 2 == 1 && floor % 2 == 1)
    };
    let rgb = image::imageops::crop_imm(&resized, center(nw), center(nh), size, size).to_image();
    let plane = (size * size) as usize;
    let mut out = vec![0.; 3 * plane];
    for (i, pixel) in rgb.pixels().enumerate() {
        for channel in 0..3 { out[channel * plane + i] = (pixel[channel] as f32 / 255. - CLIP_MEAN[channel]) / CLIP_STD[channel]; }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| image::Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, 128])))
    }

    #[test]
    fn csd_uses_style_projection_and_normalizes_batch() {
        let vars = candle_nn::VarMap::new();
        let vb = VarBuilder::from_varmap(&vars, DType::F32, &Device::Cpu);
        let cfg = Config { image_size: 28, patch_size: 14, width: 16, layers: 2, heads: 4, dim: 8 };
        let encoder = CsdEncoder::build(vb, cfg).unwrap();
        let img = gradient(40, 28);
        let rows = encoder.embed_batch(&[img.clone(), img]).unwrap();
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row.len(), 8);
            assert!((row.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-5);
        }
        assert!(rows[0].iter().zip(&rows[1]).all(|(a, b)| (a - b).abs() < 1e-5));
        assert!(encoder.embed_batch(&[]).unwrap().is_empty());
        let names = vars.data().lock().unwrap();
        assert!(names.contains_key("last_layer_style"));
        assert!(!names.contains_key("last_layer_content"));
    }

    #[test]
    fn csd_preprocess_keeps_rgb_normalization_and_center_rounding() {
        let white = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(224, 224, image::Rgb([255, 255, 255])));
        let tensor = preprocess(&white, 224).unwrap();
        for channel in 0..3 {
            let expected = (1. - CLIP_MEAN[channel]) / CLIP_STD[channel];
            assert!((tensor[channel * 224 * 224] - expected).abs() < 1e-6);
        }
        let strip = DynamicImage::ImageRgb8(image::RgbImage::from_fn(7, 4, |x, _| image::Rgb([x as u8, 0, 0])));
        // (7 - 4) / 2 = 1.5 -> 2; floor crop would silently shift the input.
        let tensor = preprocess(&strip, 4).unwrap();
        assert!((tensor[0] - (2. / 255. - CLIP_MEAN[0]) / CLIP_STD[0]).abs() < 1e-6);
        assert!(preprocess(&DynamicImage::new_rgb8(0, 1), 224).is_err());
    }

    #[test]
    fn csd_real_weights_smoke_if_available() {
        let Some(dir) = std::env::var_os("CSD_MODEL_DIR") else { return; };
        let encoder = CsdEncoder::load(Path::new(&dir), &Device::Cpu).expect("加载 CSD 官方权重");
        let image = gradient(280, 224);
        let rows = encoder.embed_batch(&[image.clone(), image]).expect("CSD 真实权重编码");
        assert_eq!(rows[0].len(), DIM);
        assert!(rows[0].iter().zip(&rows[1]).map(|(a, b)| a * b).sum::<f32>() > 0.9999);
        // 官方 PyTorch VisionTransformer + last_layer_style，权重 SHA256 40e92fad…
        // 对同一 gradient(280,224) 的前八维；防止投影、张量布局或预处理回归。
        let reference = [0.018891782, 0.005962855, -0.0020741243, 0.050656807, -0.0064162896, -0.077140406, -0.04629829, 0.022523466];
        assert!(rows[0].iter().zip(reference).all(|(actual, expected)| (actual - expected).abs() < 2e-5));
    }
}
