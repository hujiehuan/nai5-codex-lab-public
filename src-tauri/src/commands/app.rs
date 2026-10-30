//! app_info / settings_* / secret_* / events_recent。

use serde_json::Value;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::models::{AppEvent, AppInfo, Settings, SettingsPatch, StyleDevicePreference};
use crate::secrets;
use crate::state::{load_settings, save_settings, AppState, APP_VERSION};

use super::blocking;

#[tauri::command(rename_all = "snake_case")]
pub async fn app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    let _activity = state.activity.enter()?;
    let db = state.db.clone();
    let latest_release = blocking(db, |c| library::app_info_release(c)).await?;
    Ok(AppInfo {
        version: APP_VERSION.to_string(),
        app_data_dir: state.data_dir.to_string_lossy().to_string(),
        db_path: state.db_path().to_string_lossy().to_string(),
        latest_release,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<Settings> {
    let _activity = state.activity.enter()?;
    load_settings(&state.db)
}

const SETTINGS_KEYS: &[&str] = &["plugin_base_url", "webui_username", "download_concurrency", "style_device"];

fn validate_style_device_change(old: StyleDevicePreference, new: StyleDevicePreference, busy: bool) -> AppResult<()> {
    if old == new { return Ok(()); }
    if busy {
        return Err(AppError::bad_request("请先暂停索引或等待同步 / 模型下载结束，再切换计算设备"));
    }
    if new == StyleDevicePreference::Metal && !cfg!(target_os = "macos") {
        return Err(AppError::bad_request("当前平台未启用 Metal，请选择自动或 CPU"));
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set(state: State<'_, AppState>, patch: SettingsPatch) -> AppResult<Settings> {
    let _activity = state.activity.enter()?;
    let Value::Object(map) = patch else {
        return Err(AppError::bad_request("patch 必须是对象"));
    };
    let previous = load_settings(&state.db)?;
    let mut current = serde_json::to_value(&previous)?;
    let target = current.as_object_mut().expect("settings object");
    for (k, v) in map {
        if !SETTINGS_KEYS.contains(&k.as_str()) {
            return Err(AppError::bad_request(format!("未知设置键: {k}")));
        }
        target.insert(k, v);
    }
    let mut merged: Settings = serde_json::from_value(current).map_err(|e| AppError::bad_request(format!("设置值类型错误: {e}")))?;
    merged.plugin_base_url = merged.plugin_base_url.trim().trim_end_matches('/').to_string();
    merged.download_concurrency = merged.download_concurrency.clamp(1, 4);
    validate_style_device_change(previous.style_device, merged.style_device, state.sync.is_running() || state.style.is_downloading())?;
    save_settings(&state.db, &merged)?;
    if previous.style_device != merged.style_device {
        state.style.unload();
    }
    state.db.log_event("info", "settings", "设置已更新", serde_json::to_value(&merged).unwrap_or(Value::Null));
    Ok(merged)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_set(state: State<'_, AppState>, account: String, value: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    tokio::task::spawn_blocking(move || { let _activity = _activity; secrets::set(&account, &value) }).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_has(state: State<'_, AppState>, account: String) -> AppResult<bool> {
    let _activity = state.activity.enter()?;
    tokio::task::spawn_blocking(move || { let _activity = _activity; secrets::has(&account) }).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_delete(state: State<'_, AppState>, account: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    tokio::task::spawn_blocking(move || { let _activity = _activity; secrets::delete(&account) }).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn events_recent(state: State<'_, AppState>, limit: i64) -> AppResult<Vec<AppEvent>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| library::events_recent(c, limit)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_job_blocks_device_change_but_not_other_settings() {
        use StyleDevicePreference::*;
        assert!(validate_style_device_change(Auto, Auto, true).is_ok());
        assert_eq!(validate_style_device_change(Auto, Cpu, true).unwrap_err().code, "bad_request");
        assert!(validate_style_device_change(Auto, Cpu, false).is_ok());
        assert!(validate_style_device_change(Cpu, Auto, false).is_ok());
        assert_eq!(validate_style_device_change(Auto, Metal, false).is_ok(), cfg!(target_os = "macos"));
    }
}
