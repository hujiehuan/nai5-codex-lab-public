use crate::{
    error::AppResult,
    state::AppState,
    studio::{
        self,
        skills::{ImportRequest, PluginSkillSnapshot, Skill},
        ReferenceQuery,
    },
};
use serde_json::Value;
use tauri::State;

#[tauri::command(rename_all = "snake_case")]
pub async fn studio_capabilities(state: State<'_, AppState>) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::capabilities(&state).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_references(
    state: State<'_, AppState>,
    req: ReferenceQuery,
) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::references(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_prepare(state: State<'_, AppState>, req: Value) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::prepare(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_plan(state: State<'_, AppState>, req: Value) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::plan(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_plan_get(state: State<'_, AppState>, plan_id: String) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    let p = studio::store::path(&state.data_dir, "plans", &plan_id)?;
    Ok(studio::store::read(&p)?["batch"].clone())
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_start(state: State<'_, AppState>, req: Value) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::start(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_status(state: State<'_, AppState>, run_id: String) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::status(&state, run_id, false).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_list(state: State<'_, AppState>, limit: usize) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::list(&state, limit).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_cancel(state: State<'_, AppState>, run_id: String) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::status(&state, run_id, true).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_image(
    state: State<'_, AppState>,
    run_id: String,
    idx: usize,
    format: String,
    quality: u8,
) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::image(&state, run_id, idx, format, quality).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_compare(state: State<'_, AppState>, req: Value) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::compare(&state, req).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_skills_list(state: State<'_, AppState>) -> AppResult<Vec<Skill>> {
    let _activity = state.activity.enter()?;
    let root = state.data_dir.clone();
    tokio::task::spawn_blocking(move || studio::skills::list(&root))
        .await
        .map_err(crate::plugin::join_err)?
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_skills_import(
    state: State<'_, AppState>,
    req: ImportRequest,
) -> AppResult<Skill> {
    let _activity = state.activity.enter()?;
    let root = state.data_dir.clone();
    tokio::task::spawn_blocking(move || studio::skills::import(&root, req))
        .await
        .map_err(crate::plugin::join_err)?
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_skills_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    studio::skills::delete(&state.data_dir, &id)
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_skills_fetch(state: State<'_, AppState>, id: String) -> AppResult<Skill> {
    let _activity = state.activity.enter()?;
    studio::skills::fetch(&state, &id).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_skills_sync(
    state: State<'_, AppState>,
    kind: String,
) -> AppResult<PluginSkillSnapshot> {
    let _activity = state.activity.enter()?;
    studio::skills::sync(&state, &kind).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_workspace_get(state: State<'_, AppState>) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    studio::workspace_get(&state.data_dir)
}
#[tauri::command(rename_all = "snake_case")]
pub async fn studio_workspace_save(app_state: State<'_, AppState>, state: Value) -> AppResult<()> {
    let _activity = app_state.activity.enter()?;
    studio::workspace_save(&app_state.data_dir, &state)
}
