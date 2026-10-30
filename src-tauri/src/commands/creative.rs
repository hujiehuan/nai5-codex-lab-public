//! Dedicated IPC for the creative lab. The browser never supplies reference text or skills.
use crate::{
    creative::{
        self, Capabilities, EditRequest, Plan, PlanRequest, Reference, ReferencePage,
        ReferenceQuery, Run, StartRequest,
    },
    error::AppResult,
    state::AppState,
};
use tauri::State;

#[tauri::command(rename_all = "snake_case")]
pub async fn creative_capabilities(state: State<'_, AppState>) -> AppResult<Capabilities> {
    let _activity = state.activity.enter()?;
    creative::capabilities(&state).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_references(
    state: State<'_, AppState>,
    req: ReferenceQuery,
) -> AppResult<ReferencePage> {
    let _activity = state.activity.enter()?;
    creative::references(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_plan(state: State<'_, AppState>, req: PlanRequest) -> AppResult<Plan> {
    let _activity = state.activity.enter()?;
    creative::plan(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_edit(state: State<'_, AppState>, req: EditRequest) -> AppResult<Plan> {
    let _activity = state.activity.enter()?;
    creative::edit(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_plans_list(state: State<'_, AppState>, limit: usize) -> AppResult<Vec<Plan>> {
    let _activity = state.activity.enter()?;
    let dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || creative::plans_list(&dir, limit))
        .await
        .map_err(crate::plugin::join_err)?
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_plan_references(
    state: State<'_, AppState>,
    plan_id: String,
) -> AppResult<Vec<Reference>> {
    let _activity = state.activity.enter()?;
    let dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || creative::plan_references(&dir, &plan_id))
        .await
        .map_err(crate::plugin::join_err)?
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_start(state: State<'_, AppState>, req: StartRequest) -> AppResult<Run> {
    let _activity = state.activity.enter()?;
    creative::start(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_status(state: State<'_, AppState>, run_id: String) -> AppResult<Run> {
    let _activity = state.activity.enter()?;
    creative::status(&state, run_id).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_list(state: State<'_, AppState>, limit: usize) -> AppResult<Vec<Run>> {
    let _activity = state.activity.enter()?;
    creative::list(&state, limit).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_cancel(state: State<'_, AppState>, run_id: String) -> AppResult<Run> {
    let _activity = state.activity.enter()?;
    creative::cancel(&state, run_id).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn creative_image(
    state: State<'_, AppState>,
    run_id: String,
    idx: usize,
    full: bool,
) -> AppResult<String> {
    let _activity = state.activity.enter()?;
    creative::image(&state, run_id, idx, full).await
}
