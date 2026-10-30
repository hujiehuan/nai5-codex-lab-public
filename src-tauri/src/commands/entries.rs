//! list_books / list_entries / get_entry / set_decision / set_member_note / undo_decision / blacklist_*。

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::models::*;
use crate::state::AppState;

use super::blocking;

#[tauri::command(rename_all = "snake_case")]
pub async fn list_books(state: State<'_, AppState>) -> AppResult<Vec<Book>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), |c| library::list_books(c)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_entries(state: State<'_, AppState>, query: EntryQuery) -> AppResult<EntryPage> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| library::list_entries(c, &query)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_entry(state: State<'_, AppState>, entry_id: String) -> AppResult<EntryDetail> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| library::get_entry(c, &entry_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_decision(state: State<'_, AppState>, entry_ids: Vec<String>, decision: Decision, via: DecisionVia) -> AppResult<ChangedResult> {
    let _activity = state.activity.enter()?;
    if entry_ids.is_empty() {
        return Ok(ChangedResult { changed: 0 });
    }
    let db = state.db.clone();
    let changed = tokio::task::spawn_blocking(move || library::set_decision(&db, &entry_ids, decision, via))
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))??;
    Ok(ChangedResult { changed })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_member_note(state: State<'_, AppState>, entry_id: String, note: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| library::set_member_note(c, &entry_id, &note)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn undo_decision(state: State<'_, AppState>) -> AppResult<Option<UndoResult>> {
    let _activity = state.activity.enter()?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || library::undo_decision(&db)).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn blacklist_list(state: State<'_, AppState>) -> AppResult<Vec<BlacklistItem>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), |c| library::blacklist_list(c)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn blacklist_add(state: State<'_, AppState>, name: String, reason: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    let db = state.db.clone();
    let n = name.clone();
    blocking(db, move |c| library::blacklist_add(c, &n, &reason)).await?;
    state.db.log_event("info", "blacklist", "加入黑名单", serde_json::json!({ "name": name }));
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn blacklist_remove(state: State<'_, AppState>, name: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| library::blacklist_remove(c, &name)).await
}
