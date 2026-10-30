//! 固定版本的画风编码器目录。模型 ID 同时隔离权重、索引和均值；不接受任意路径。
use std::path::Path;

use crate::error::{AppError, AppResult};
use super::model;

pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: Option<&'static str>,
}

pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub dim: usize,
    pub default_center: bool,
    pub batch_size: usize,
    pub files: &'static [ModelFile],
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: model::MODEL_NAME, label: "CLIP ViT-B/32", description: "原有基线，通用图像向量；保留已计算的画风词典索引。",
        dim: 512, default_center: true, batch_size: 16,
        files: &[
            ModelFile { name: "config.json", url: "https://huggingface.co/openai/clip-vit-base-patch32/resolve/3d74acf9a28c67741b2f4f2ea7635f0aaf6f0268/config.json", bytes: 0, sha256: None },
            ModelFile { name: "pytorch_model.bin", url: "https://huggingface.co/openai/clip-vit-base-patch32/resolve/3d74acf9a28c67741b2f4f2ea7635f0aaf6f0268/pytorch_model.bin", bytes: 605247071, sha256: Some("a63082132ba4f97a80bea76823f544493bffa8082296d62d71581a4feff1576f") },
        ],
    },
    ModelSpec {
        id: "megastyle-siglip-so400m", label: "MegaStyle-Encoder", description: "面向画风相似度微调的 SigLIP 图像编码器，独立建索引后对比。",
        dim: 1152, default_center: false, batch_size: 2,
        files: &[
            ModelFile { name: "config.json", url: "https://huggingface.co/google/siglip-so400m-patch14-384/resolve/9fdffc58afc957d1a03a25b10dba0329ab15c2a3/config.json", bytes: 576, sha256: None },
            ModelFile { name: "preprocessor_config.json", url: "https://huggingface.co/google/siglip-so400m-patch14-384/resolve/9fdffc58afc957d1a03a25b10dba0329ab15c2a3/preprocessor_config.json", bytes: 368, sha256: None },
            ModelFile { name: "megastyle_encoder.pth", url: "https://huggingface.co/Gaojunyao/MegaStyle/resolve/8ac053fb78602495e3a932789ccc6f5edbed943e/megastyle_encoder.pth", bytes: 856600406, sha256: Some("79d08343d9208e8348c9f2d61fc627988fd5ad34dda7e977db9a6ead19078d56") },
        ],
    },
    ModelSpec {
        id: "csd-vit-large-patch14", label: "CSD ViT-L/14", description: "专门提取画风描述向量的编码器，使用作者公开的 style projection 权重。",
        dim: 768, default_center: false, batch_size: 4,
        files: &[
            ModelFile { name: "pytorch_model.bin", url: "https://huggingface.co/tomg-group-umd/CSD-ViT-L/resolve/5bc26a6fb0487f3f00a2a7313135103a005b1b67/pytorch_model.bin", bytes: 2438228893, sha256: Some("40e92fad63a361b8136100cd234c42d401ef9b34ff1748234318929ebcc7e7a1") },
        ],
    },
];

pub fn get(id: &str) -> AppResult<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id).ok_or_else(|| AppError::bad_request(format!("未知的画风模型：{id}")))
}

impl ModelSpec {
    pub fn download_size(&self) -> u64 { self.files.iter().map(|f| f.bytes).sum() }
    pub fn present(&self, dir: &Path) -> bool {
        if self.id == model::MODEL_NAME { return model::is_present(dir); }
        self.files.iter().all(|f| std::fs::metadata(dir.join(f.name)).map(|m| m.is_file() && m.len() > 0 && (f.bytes == 0 || m.len() == f.bytes)).unwrap_or(false))
    }
    pub fn size_bytes(&self, dir: &Path) -> u64 {
        if self.id == model::MODEL_NAME { return model::size_bytes(dir); }
        self.files.iter().filter_map(|f| std::fs::metadata(dir.join(f.name)).ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_known_and_models_have_distinct_dimensions() {
        for bad in ["", "../clip-vit-base-patch32", "/tmp/weights", "other-model"] { assert!(get(bad).is_err()); }
        assert_eq!(MODELS.iter().map(|m| m.dim).collect::<Vec<_>>(), vec![512, 1152, 768]);
        assert!(MODELS.iter().all(|m| m.files.iter().all(|f| f.url.starts_with("https://huggingface.co/") && !f.url.contains("/main/"))));
    }
}
