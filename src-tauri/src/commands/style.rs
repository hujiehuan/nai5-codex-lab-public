//! style_* 命令（CONTRACT §12.2）：模型状态 / 下载（长命令，完成才返回，`style_model_download_cancel` 取消）/
//! 单独建索引（占用 `SyncController`，与同步互斥，`sync_cancel` 可取消，事件走 `Channel<SyncEvent>`）/ 画风检索。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::models::{DownloadEvent, StyleModelStatus, StyleSearchRequest, StyleSearchResult, SyncEvent, SyncPhase};
use crate::state::AppState;
use crate::style;

#[tauri::command(rename_all = "snake_case")]
pub async fn style_model_status(state: State<'_, AppState>, model: Option<String>) -> AppResult<StyleModelStatus> {
    let _activity = state.activity.enter()?;
    style::status(&state, model).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn style_models_list(state: State<'_, AppState>) -> AppResult<Vec<StyleModelStatus>> {
    let _activity = state.activity.enter()?;
    let mut result = Vec::new();
    for model in style::catalog::MODELS { result.push(style::status(&state, Some(model.id.into())).await?); }
    Ok(result)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn style_model_select(state: State<'_, AppState>, model: String) -> AppResult<StyleModelStatus> {
    let _activity = state.activity.enter()?;
    style::catalog::get(&model)?;
    let _inference = state.style.inference_lock.try_lock().map_err(|_| AppError::bad_request("画风模型正在计算，请完成或暂停后切换"))?;
    if state.sync.is_running() || state.style.is_downloading() { return Err(AppError::bad_request("同步、索引或下载期间不能切换模型")); }
    state.db.app_state_set(style::store::STYLE_MODEL_KEY, &model)?;
    state.style.unload();
    style::status(&state, Some(model)).await
}

/// 流式下载两个文件到 `.part` 再改名，进度走 `on_event`；已在下载 → `bad_request`；取消 → `cancelled`（`.part` 保留供续传）。
#[tauri::command(rename_all = "snake_case")]
pub async fn style_model_download(state: State<'_, AppState>, model: Option<String>, on_event: Channel<DownloadEvent>) -> AppResult<StyleModelStatus> {
    let _activity = state.activity.enter()?;
    let emit = move |ev: DownloadEvent| {
        let _ = on_event.send(ev);
    };
    style::download(&state, model, &emit).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn style_model_download_cancel(state: State<'_, AppState>) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    if state.style.cancel_download() {
        state.db.log_event("info", "style", "用户取消模型下载", serde_json::json!({}));
    }
    Ok(())
}

/// 不经同步单独建索引；`force=true` 删掉当前模型的全部向量重算。同步在跑 → `sync_running`；命令立即返回。
#[tauri::command(rename_all = "snake_case")]
pub async fn style_index_rebuild(state: State<'_, AppState>, model: Option<String>, force: bool, on_event: Channel<SyncEvent>) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    let model = match model { Some(id) => style::catalog::get(&id)?.id.to_string(), None => state.db.with(|c| style::store::current_model(c))? };
    if !state.sync.try_start() {
        return Err(AppError::sync_running());
    }
    state.sync.update(|s| {
        s.running = true;
        s.phase = SyncPhase::Embedding;
        s.message = "准备画风索引".into();
        s.images_total = 0;
        s.images_done = 0;
        s.images_failed = 0;
    });
    let cancel = state.sync.fresh_token();
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let style_state = state.style.clone();
    let controller = state.sync.clone();
    let emit: Arc<dyn Fn(SyncEvent) + Send + Sync> = Arc::new(move |ev: SyncEvent| {
        let _ = on_event.send(ev);
    });
    tauri::async_runtime::spawn(async move {
        let _activity = _activity;
        style::index::run_rebuild(db, data_dir, style_state, controller, cancel, force, model, emit).await;
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn style_search(state: State<'_, AppState>, req: StyleSearchRequest) -> AppResult<StyleSearchResult> {
    let _activity = state.activity.enter()?;
    style::search::search(&state, req).await
}
