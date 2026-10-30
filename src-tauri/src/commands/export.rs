//! export_libraries / push_to_plugin（§3）与 export_full_pool / push_full_to_plugin / plugin_library_status（§8.2）。

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::export;
use crate::models::{ExportOptions, ExportResult, PluginLibraryStatus, PushFullResult, PushResult};
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn export_libraries(state: State<'_, AppState>, options: ExportOptions) -> AppResult<ExportResult> {
    let _activity = state.activity.enter()?;
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let (result, _) = tokio::task::spawn_blocking(move || export::export_libraries(&db, &data_dir, &options))
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))??;
    Ok(result)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn push_to_plugin(state: State<'_, AppState>) -> AppResult<PushResult> {
    let _activity = state.activity.enter()?;
    export::push_to_plugin(&state).await
}

/// §8.2：写 `exports/<时间戳>/full_pool.json`（或 `out_dir`），`files` 一项，`skipped=0`。
#[tauri::command(rename_all = "snake_case")]
pub async fn export_full_pool(state: State<'_, AppState>, options: ExportOptions) -> AppResult<ExportResult> {
    let _activity = state.activity.enter()?;
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || export::export_full_pool(&db, &data_dir, &options))
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))?
}

/// §8.2：内存构建完整池后 POST `/page/library/import_full`，不落盘。
#[tauri::command(rename_all = "snake_case")]
pub async fn push_full_to_plugin(state: State<'_, AppState>) -> AppResult<PushFullResult> {
    let _activity = state.activity.enter()?;
    export::push_full_to_plugin(&state).await
}

/// §8.2：GET `/page/library/status`；连不上 / 未配置 → `reachable=false`，401/403 → `plugin_rejected`。
#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_library_status(state: State<'_, AppState>) -> AppResult<PluginLibraryStatus> {
    let _activity = state.activity.enter()?;
    export::plugin_library_status(&state).await
}
