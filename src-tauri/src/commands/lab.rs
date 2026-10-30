//! 画图实验室七个命令（CONTRACT §9.4）：全部转调 `lab` 模块，参数校验（count / artist_ratio / run_id / idx）也在那里。
//! 出图在插件跑，这里没有长任务：`lab_run_start` 由插件立即返回 `{ run_id, plans }`，前端每 2 s 轮询 `lab_run_status`。

use tauri::State;

use crate::error::AppResult;
use crate::lab;
use crate::models::{LabPlan, LabRating, LabRun, LabRunRequest, LabRunStart, LabRunSummary};
use crate::state::AppState;

/// POST `/page/lab/run/preview`：只组装不出图（LLM 路径会真的调 LLM）。
#[tauri::command(rename_all = "snake_case")]
pub async fn lab_run_preview(state: State<'_, AppState>, req: LabRunRequest) -> AppResult<Vec<LabPlan>> {
    let _activity = state.activity.enter()?;
    lab::preview(&state, req).await
}

/// POST `/page/lab/run/start`：插件运行中 409 → `plugin_rejected`。
#[tauri::command(rename_all = "snake_case")]
pub async fn lab_run_start(state: State<'_, AppState>, req: LabRunRequest) -> AppResult<LabRunStart> {
    let _activity = state.activity.enter()?;
    lab::start(&state, req).await
}

/// GET `/page/lab/run/status?run_id=`；`done|error` 时整份落进本地 `lab_run`。
#[tauri::command(rename_all = "snake_case")]
pub async fn lab_run_status(state: State<'_, AppState>, run_id: String) -> AppResult<LabRun> {
    let _activity = state.activity.enter()?;
    lab::status(&state, run_id).await
}

/// 本地 `lab_run` ∪ 插件 `run/list`，本地优先、`created_at` 倒序；插件不可达只回本地。
#[tauri::command(rename_all = "snake_case")]
pub async fn lab_run_list(state: State<'_, AppState>, limit: i64) -> AppResult<Vec<LabRunSummary>> {
    let _activity = state.activity.enter()?;
    lab::list(&state, limit).await
}

/// 返回相对数据目录的路径 `labruns/<run_id>/<idx>[.thumb].png`；首次从插件拉取落盘。
#[tauri::command(rename_all = "snake_case")]
pub async fn lab_run_image(state: State<'_, AppState>, run_id: String, idx: i64, full: bool) -> AppResult<String> {
    let _activity = state.activity.enter()?;
    lab::image(&state, run_id, idx, full).await
}

/// 先写插件，成功后写本地镜像；`rating` 为 `"fav" | "dis" | null`。
#[tauri::command(rename_all = "snake_case")]
pub async fn lab_run_rate(state: State<'_, AppState>, run_id: String, idx: i64, rating: Option<LabRating>) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    lab::rate(&state, run_id, idx, rating).await
}

