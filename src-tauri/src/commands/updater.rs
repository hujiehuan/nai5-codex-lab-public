//! Source-update commands. Installation/backup handoff is deliberately separate.
use crate::{
    error::{AppError, AppResult},
    state::AppState,
    updater::AppUpdateStatus,
};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

#[tauri::command(rename_all = "snake_case")]
pub fn app_update_status(state: State<'_, AppState>) -> AppUpdateStatus {
    state.updater.status()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn app_update_choose_source(state: State<'_, AppState>) -> AppResult<AppUpdateStatus> {
    let _activity = state.activity.enter()?;
    let app = state.app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("选择 nai5-codex-lab 源码目录")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::new("internal", e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(state.updater.status());
    };
    let directory = picked
        .into_path()
        .map_err(|_| AppError::bad_request("所选目录无法读取"))?;
    let updater = state.updater.clone();
    tokio::task::spawn_blocking(move || updater.set_source(directory))
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_update_check(state: State<'_, AppState>) -> AppResult<AppUpdateStatus> {
    let _activity = state.activity.enter()?;
    state.updater.start_check()
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_update_prepare(state: State<'_, AppState>) -> AppResult<AppUpdateStatus> {
    let _activity = state.activity.enter()?;
    state.updater.start_prepare()
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_update_cancel(state: State<'_, AppState>) -> AppResult<AppUpdateStatus> {
    state.updater.cancel()
}
