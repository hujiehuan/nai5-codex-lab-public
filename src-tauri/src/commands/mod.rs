//! tauri 命令层（CONTRACT §1）：薄，只做参数校验、拿 State、调用模块函数。全部
//! `#[tauri::command(rename_all = "snake_case")]`，返回 `Result<T, AppError>`。
//! 同步 DB 查询放 `spawn_blocking`，长任务用 `tauri::async_runtime::spawn` 后立即返回。

mod app;
mod creative;
mod entries;
mod export;
mod lab;
mod reverse;
mod style;
mod style_compare;
mod sync;
mod studio;
mod updater;
mod update_install;

pub use app::*;
pub use creative::*;
pub use entries::*;
pub use export::*;
pub use lab::*;
pub use reverse::*;
pub use style::*;
pub use style_compare::*;
pub use sync::*;
pub use studio::*;
pub use updater::*;
pub use update_install::*;

use std::sync::Arc;

use crate::db::Db;
use crate::error::{AppError, AppResult};

/// 在阻塞线程里跑一段持锁的 DB 操作，避免卡住 tauri 的异步运行时。
pub(crate) async fn blocking<R: Send + 'static>(db: Arc<Db>, f: impl FnOnce(&mut rusqlite::Connection) -> AppResult<R> + Send + 'static) -> AppResult<R> {
    tokio::task::spawn_blocking(move || db.with(f)).await.map_err(|e| AppError::new("internal", e.to_string()))?
}
