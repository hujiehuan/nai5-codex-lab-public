//! sync_status / sync_start / sync_cancel / sync_logs / download_images。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::models::{SyncEvent, SyncLog, SyncStatus};
use crate::state::AppState;
use crate::sync::{self, images::ImageKind, SyncCtx};

use super::blocking;

#[tauri::command(rename_all = "snake_case")]
pub async fn sync_status(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    let _activity = state.activity.enter()?;
    let mut s = state.sync.snapshot();
    s.running = state.sync.is_running();
    if s.release_local.is_none() {
        s.release_local = state.db.app_state_get("latest_release")?;
    }
    Ok(s)
}

fn make_ctx(state: &AppState, on_event: Channel<SyncEvent>, update_status: bool) -> AppResult<Arc<SyncCtx>> {
    let settings = state.settings()?;
    // 主同步拿新令牌；按需下载用独立令牌，避免继承上一次同步已取消的状态
    let cancel = if update_status { state.sync.fresh_token() } else { tokio_util::sync::CancellationToken::new() };
    Ok(Arc::new(SyncCtx {
        db: state.db.clone(),
        http: state.http.clone(),
        throttle: state.throttle.clone(),
        data_dir: state.data_dir.clone(),
        cancel,
        concurrency: settings.download_concurrency.clamp(1, 4) as usize,
        controller: state.sync.clone(),
        update_status,
        emit: Arc::new(move |ev: SyncEvent| {
            let _ = on_event.send(ev);
        }),
        inflight: state.inflight.clone(),
        style: state.style.clone(),
    }))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn sync_start(state: State<'_, AppState>, force: bool, on_event: Channel<SyncEvent>) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    if !state.sync.try_start() {
        return Err(AppError::sync_running());
    }
    state.sync.update(|s| {
        s.running = true;
        s.phase = crate::models::SyncPhase::Manifest;
        s.message = "启动中".into();
        s.images_total = 0;
        s.images_done = 0;
        s.images_failed = 0;
    });
    let ctx = match make_ctx(&state, on_event, true) {
        Ok(c) => c,
        Err(e) => {
            state.sync.finish();
            return Err(e);
        }
    };
    tauri::async_runtime::spawn(async move {
        let _activity = _activity;
        sync::run_sync(ctx, force).await;
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn sync_cancel(state: State<'_, AppState>) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    state.sync.cancel();
    state.db.log_event("info", "sync", "用户取消同步", serde_json::json!({}));
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn sync_logs(state: State<'_, AppState>, limit: i64) -> AppResult<Vec<SyncLog>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| library::sync_logs(c, limit)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn download_images(state: State<'_, AppState>, entry_id: String, kind: String, on_event: Channel<SyncEvent>) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    let kind = match kind.as_str() {
        "display_all" => ImageKind::Display,
        "originals" => ImageKind::Original,
        other => return Err(AppError::bad_request(format!("kind 非法: {other}"))),
    };
    let exists = blocking(state.db.clone(), {
        let id = entry_id.clone();
        move |c| Ok(library::load_entry(c, &id)?.is_some())
    })
    .await?;
    if !exists {
        return Err(AppError::not_found(format!("条目不存在: {entry_id}")));
    }
    // 按需下载不占用主同步的 running 标记，只共用节流与在飞集合（sync_cancel 不会中止它；
    // 与主同步撞上同一张图时后到者跳过，见 images::run_queue）
    let ctx = make_ctx(&state, on_event, false)?;
    tauri::async_runtime::spawn(async move {
        let _activity = _activity;
        sync::run_entry_download(ctx, entry_id, kind).await;
    });
    Ok(())
}
