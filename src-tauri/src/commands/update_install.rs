use tauri::State;
use crate::{error::{AppError, AppResult}, state::AppState, updater::AppUpdateStatus};

#[tauri::command(rename_all = "snake_case")]
pub async fn app_update_install(state: State<'_, AppState>) -> AppResult<AppUpdateStatus> {
    // The barrier also belongs to detached downloads, indexing and comparisons.
    // It stays locked through the backup and helper handoff, preventing new work.
    let install = state.activity.install()?;
    let prepared = state.updater.begin_install()?;
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let handoff = tokio::task::spawn_blocking(move || {
        crate::update_installer::installed_bundle().map_err(|e| AppError::new("update_install", e))?;
        crate::update_installer::validate_candidate(&prepared.bundle_path, &prepared.candidate.version)
            .map_err(|e| AppError::new("update_install", e))?;
        let backup = crate::update_backup::before_install(&db, &data_dir)
            .map_err(|e| AppError::new("update_backup", format!("升级备份失败，安装已停止：{e}")))?;
        db.log_event("info", "update", "安装前数据库备份完成", serde_json::json!({"path":backup,"version":prepared.candidate.version}));
        crate::update_installer::launch_installer(&prepared.bundle_path, &prepared.candidate.version, &data_dir.join("updates"))
            .map_err(|e| AppError::new("update_install", e))
    }).await.map_err(|e| AppError::new("update_install", format!("准备安装失败：{e}"))).and_then(|r| r);
    if let Err(error) = handoff {
        state.updater.finish_install_error(&error.message);
        return Err(error);
    }
    let status = state.updater.status();
    let app = state.app.clone();
    tauri::async_runtime::spawn(async move {
        let _install = install;
        // Allow the IPC response to paint the restart state before terminating.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        app.exit(0);
        // Keep new requests blocked until Tauri has actually exited.
        std::mem::forget(_install);
    });
    Ok(status)
}
