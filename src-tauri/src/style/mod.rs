//! 画风索引与反推画风（CONTRACT §12，v0.3.0）：反推画风 = 检索，不是让视觉模型猜画师名。
//! - `model`    CLIP ViT-B/32 图像塔（candle，CPU / Metal）：加载、预处理、批量编码（§12.3）
//! - `store`    `image_embedding` 表、当前模型名、均值缓存、待算清单、计数、f16 base64（§12.1 / §12.4）
//! - `index`    建索引：同步的 embedding 阶段与 `style_index_rebuild` 共用（§12.2）
//! - `search`   `style_search`：查询图编码 → 减均值 → 余弦 top-k → 拼条目信息，黑名单永不返回（§12.2）
//! - `download` 从 Hugging Face 流式下载权重到 `.part` 再改名，支持续传与取消（§12.2）
//!
//! `StyleState` 挂在 `AppState.style`：进程内缓存一份已加载的模型（约 350 MB，首次用到时加载，之后同步与检索共用）
//! 与模型下载的取消令牌。

pub mod catalog;
pub mod compare;
pub mod csd;
pub mod megastyle;
pub mod classifier;
pub mod onnx_runtime;
pub mod download;
pub mod index;
pub mod model;
pub mod search;
pub mod store;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::models::{StyleDevicePreference, StyleModelStatus};
use crate::state::{load_settings, AppState};

pub use model::Encoder;

/// 模型缓存 + 下载控制块。
pub struct StyleState {
    encoder: Mutex<Option<(PathBuf, StyleDevicePreference, Arc<Encoder>)>>,
    load_lock: tokio::sync::Mutex<()>,
    pub inference_lock: tokio::sync::Mutex<()>,
    download_cancel: Mutex<Option<CancellationToken>>,
    downloading: AtomicBool,
}

impl StyleState {
    pub fn new() -> Self {
        StyleState { encoder: Mutex::new(None), load_lock: tokio::sync::Mutex::new(()), inference_lock: tokio::sync::Mutex::new(()), download_cancel: Mutex::new(None), downloading: AtomicBool::new(false) }
    }

    /// 已加载且目录相同的模型。
    pub fn cached(&self, dir: &Path, preference: StyleDevicePreference) -> Option<Arc<Encoder>> {
        self.encoder.lock().as_ref().filter(|(p, mode, _)| p == dir && *mode == preference).map(|(_, _, e)| e.clone())
    }

    /// 拿模型：缓存命中直接给，否则在阻塞线程里加载再缓存（换了目录 = 换了模型，旧的丢掉）。
    pub async fn encoder(&self, dir: &Path, preference: StyleDevicePreference) -> AppResult<Arc<Encoder>> {
        // 同步与检索共用模型；并发首次调用只加载一次，避免重复占用 GPU 内存。
        let _loading = self.load_lock.lock().await;
        if let Some(e) = self.cached(dir, preference) {
            return Ok(e);
        }
        self.unload();
        let d = dir.to_path_buf();
        let started = std::time::Instant::now();
        let enc = tokio::task::spawn_blocking(move || Encoder::load_with_preference(&d, preference)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
        tracing::info!(dir = %dir.display(), device = ?enc.device_kind(), fallback = ?enc.fallback_reason(), ms = started.elapsed().as_millis() as u64, "画风模型已加载");
        let enc = Arc::new(enc);
        *self.encoder.lock() = Some((dir.to_path_buf(), preference, enc.clone()));
        Ok(enc)
    }

    /// 同步版（已在阻塞线程里时用）。
    #[cfg(test)]
    pub fn encoder_blocking(&self, dir: &Path) -> AppResult<Arc<Encoder>> {
        if let Some(e) = self.cached(dir, StyleDevicePreference::Cpu) {
            return Ok(e);
        }
        let enc = Arc::new(Encoder::load(dir)?);
        *self.encoder.lock() = Some((dir.to_path_buf(), StyleDevicePreference::Cpu, enc.clone()));
        Ok(enc)
    }

    /// 丢掉缓存（重新下载权重后调用，下次用到时再加载）。
    pub fn unload(&self) {
        *self.encoder.lock() = None;
    }

    /// 原子地标记「下载中」并发一枚新令牌；已在下载 → None。
    pub fn try_start_download(&self) -> Option<CancellationToken> {
        if self.downloading.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return None;
        }
        let token = CancellationToken::new();
        *self.download_cancel.lock() = Some(token.clone());
        Some(token)
    }

    pub fn finish_download(&self) {
        *self.download_cancel.lock() = None;
        self.downloading.store(false, Ordering::SeqCst);
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading.load(Ordering::SeqCst)
    }

    /// 取消进行中的下载；没有在下载返回 false。
    pub fn cancel_download(&self) -> bool {
        match self.download_cancel.lock().as_ref() {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }
}

impl Default for StyleState {
    fn default() -> Self {
        Self::new()
    }
}

/// §12.2 `StyleModelStatus`（持锁部分）：模型名、目录、在场与大小、已索引 / 应索引、均值缓存是否就绪。
pub fn status_sync(conn: &Connection, data_dir: &Path) -> AppResult<StyleModelStatus> {
    let model = store::current_model(conn)?;
    status_sync_for_model(conn, data_dir, &model)
}

pub fn status_sync_for_model(conn: &Connection, data_dir: &Path, model: &str) -> AppResult<StyleModelStatus> {
    let spec = catalog::get(model)?;
    let dir = model::model_dir(data_dir, model);
    let present = spec.present(&dir);
    Ok(StyleModelStatus {
        model: model.to_string(),
        label: spec.label.to_string(),
        description: spec.description.to_string(),
        dim: spec.dim,
        download_size_bytes: spec.download_size(),
        default_center: spec.default_center,
        dir: dir.to_string_lossy().to_string(),
        present,
        size_bytes: spec.size_bytes(&dir),
        indexed: store::count_indexed(conn, &model)?,
        total: store::count_total(conn)?,
        mean_ready: store::cached_mean(conn, &model)?.is_some(),
        device_preference: StyleDevicePreference::Auto,
        active_device: None,
        metal_available: cfg!(target_os = "macos"),
        device_message: None,
    })
}

pub async fn status(state: &AppState, requested: Option<String>) -> AppResult<StyleModelStatus> {
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let mut status = tokio::task::spawn_blocking(move || -> AppResult<StyleModelStatus> {
        let preference = load_settings(&db)?.style_device;
        let mut status = db.with(|c| match requested.as_deref() { Some(model) => status_sync_for_model(c, &data_dir, model), None => status_sync(c, &data_dir) })?;
        status.device_preference = preference;
        Ok(status)
    }).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    if let Some(enc) = state.style.cached(Path::new(&status.dir), status.device_preference) {
        status.active_device = Some(enc.device_kind());
        status.device_message = enc.fallback_reason().map(str::to_owned);
    }
    if !status.metal_available && status.device_message.is_none() {
        status.device_message = Some("当前平台未启用 Metal，使用 CPU 推理".into());
    }
    Ok(status)
}

/// §12.2 `style_model_download`：同一时刻只允许一个下载；成功后丢掉模型缓存（可能是重新下载）并返回最新状态。
pub async fn download(state: &AppState, requested: Option<String>, on_event: download::DownloadEmitter<'_>) -> AppResult<StyleModelStatus> {
    let model = match requested { Some(id) => catalog::get(&id)?.id.to_string(), None => state.db.with(|c| store::current_model(c))? };
    let spec = catalog::get(&model)?;
    let cancel = state.style.try_start_download().ok_or_else(|| AppError::bad_request("画风模型正在下载中"))?;
    let dir = model::model_dir(&state.data_dir, &model);
    state.db.log_event("info", "style", "开始下载画风模型", json!({ "model": model }));
    let result = async {
        for file in spec.files {
            // 兼容用户已放入的原版 CLIP safetensors。
            if spec.id == model::MODEL_NAME && file.name == model::WEIGHTS_FILE && model::weights_path(&dir).is_some_and(|p| p.file_name().is_some_and(|n| n == model::SAFETENSORS_FILE)) { continue; }
            download::download_checked(&state.http, file, &dir, &cancel, on_event).await?;
        }
        if cancel.is_cancelled() { return Err(AppError::cancelled()); }
        Ok::<(), AppError>(())
    }.await;
    state.style.finish_download();
    match result {
        Ok(()) => {
            state.style.unload();
            state.db.log_event("info", "style", "画风模型文件已准备", json!({ "model": model, "size_bytes": spec.size_bytes(&dir) }));
            status(state, Some(model)).await
        }
        Err(e) => {
            state.db.log_event(if e.code == "cancelled" { "info" } else { "error" }, "style", &format!("画风模型下载结束: {}", e.message), json!({ "model": model }));
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// `StyleModelStatus`：空目录 → present=false、size 0、indexed/total 0、mean_ready=false；只有 config.json 不算在场；
    /// 两个文件齐了才 present，size 是两者之和。下载控制块：同一时刻只一个、取消令牌生效、结束后可再开。
    #[test]
    fn status_and_download_guard() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let st = db.with(|c| status_sync(c, dir.path())).unwrap();
        assert_eq!(st.model, model::MODEL_NAME);
        assert_eq!(st.dir, dir.path().join("models").join(model::MODEL_NAME).to_string_lossy());
        assert!(!st.present && st.size_bytes == 0 && st.indexed == 0 && st.total == 0 && !st.mean_ready);
        let mdir = dir.path().join("models").join(model::MODEL_NAME);
        std::fs::create_dir_all(&mdir).unwrap();
        std::fs::write(mdir.join("config.json"), b"{}").unwrap();
        std::fs::write(mdir.join("model.safetensors.part"), b"half").unwrap();
        assert!(!db.with(|c| status_sync(c, dir.path())).unwrap().present, ".part 不算");
        std::fs::write(mdir.join("model.safetensors"), b"weights").unwrap();
        let st = db.with(|c| status_sync(c, dir.path())).unwrap();
        assert!(st.present);
        assert_eq!(st.size_bytes, 2 + 7);

        let s = StyleState::new();
        assert!(!s.is_downloading() && !s.cancel_download());
        let tok = s.try_start_download().unwrap();
        assert!(s.is_downloading());
        assert!(s.try_start_download().is_none(), "同一时刻只一个下载");
        assert!(s.cancel_download());
        assert!(tok.is_cancelled());
        s.finish_download();
        assert!(!s.is_downloading());
        assert!(s.try_start_download().is_some(), "结束后可再开");
        assert!(s.cached(&mdir, StyleDevicePreference::Auto).is_none());
        assert!(s.encoder_blocking(&mdir).is_err(), "假权重加载失败但不 panic");
    }
}
