//! 统一错误类型：命令返回 `Result<T, AppError>`，序列化为 `{ code, message }`（见 CONTRACT.md §0）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }
    pub fn bad_request(m: impl Into<String>) -> Self { Self::new("bad_request", m) }
    pub fn not_found(m: impl Into<String>) -> Self { Self::new("not_found", m) }
    pub fn upstream(m: impl Into<String>) -> Self { Self::new("upstream", m) }
    pub fn network(m: impl Into<String>) -> Self { Self::new("network", m) }
    pub fn io(m: impl Into<String>) -> Self { Self::new("io", m) }
    pub fn db(m: impl Into<String>) -> Self { Self::new("db", m) }
    pub fn secret(m: impl Into<String>) -> Self { Self::new("secret", m) }
    pub fn sync_running() -> Self { Self::new("sync_running", "同步正在进行中") }
    pub fn cancelled() -> Self { Self::new("cancelled", "已取消") }
    /// 下载完成后 MD5 与 ETag 不符（v0.1.1，图片队列专用）。
    pub fn checksum(m: impl Into<String>) -> Self { Self::new("checksum", m) }
    pub fn plugin_rejected(m: impl Into<String>) -> Self { Self::new("plugin_rejected", m) }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self { Self::db(e.to_string()) }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self { Self::io(e.to_string()) }
}
impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self { Self::network(e.to_string()) }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self { Self::bad_request(format!("JSON: {e}")) }
}
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self { Self::new("internal", e.to_string()) }
}
impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self { Self::secret(e.to_string()) }
}
impl From<image::ImageError> for AppError {
    fn from(e: image::ImageError) -> Self { Self::io(format!("image: {e}")) }
}
/// v0.3.0：candle 推理 / 权重加载错误（§12.3），code `model`。
impl From<candle_core::Error> for AppError {
    fn from(e: candle_core::Error) -> Self { Self::new("model", format!("candle: {e}")) }
}

pub type AppResult<T> = Result<T, AppError>;
