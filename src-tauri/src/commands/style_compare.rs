//! Independent local comparison job; dropping an IPC await cannot release its running slot.

use parking_lot::Mutex;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::state::{load_settings, AppState};
use crate::style::compare::{compare_with, CompareRequest, CompareResult};

struct ComparisonController(Mutex<Option<CancellationToken>>);

impl ComparisonController {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn start(&self) -> AppResult<RunningComparison<'_>> {
        let mut active = self.0.lock();
        if active.is_some() {
            return Err(AppError::new(
                "batch_running",
                "画风比较正在运行，请完成或停止后再开始",
            ));
        }
        let token = CancellationToken::new();
        *active = Some(token.clone());
        Ok(RunningComparison {
            controller: self,
            token,
        })
    }

    fn cancel(&self) {
        if let Some(token) = self.0.lock().as_ref() {
            token.cancel();
        }
    }
}

struct RunningComparison<'a> {
    controller: &'a ComparisonController,
    token: CancellationToken,
}

impl Drop for RunningComparison<'_> {
    fn drop(&mut self) {
        *self.controller.0.lock() = None;
    }
}

static COMPARISON: ComparisonController = ComparisonController::new();

#[tauri::command(rename_all = "snake_case")]
pub async fn style_compare(
    state: State<'_, AppState>,
    req: CompareRequest,
) -> AppResult<CompareResult> {
    let _activity = state.activity.enter()?;
    let running = COMPARISON.start()?;
    let data_dir = state.data_dir.clone();
    let style = state.style.clone();
    let db = state.db.clone();
    // Keep the slot and inference lock owned by this task until all blocking work finishes.
    tokio::spawn(async move {
        let _activity = _activity;
        let _running = running;
        let preference = tokio::task::spawn_blocking(move || {
            load_settings(&db).map(|settings| settings.style_device)
        })
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))??;
        compare_with(data_dir, style, preference, req, _running.token.clone()).await
    })
    .await
    .map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub fn style_compare_cancel() -> AppResult<()> {
    COMPARISON.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_job_remains_exclusive_until_worker_finishes() {
        let controller = ComparisonController::new();
        controller.cancel();
        let running = controller.start().unwrap();
        assert!(controller.start().is_err());
        controller.cancel();
        assert!(running.token.is_cancelled());
        assert!(controller.start().is_err());
        drop(running);
        let next = controller.start().unwrap();
        assert!(!next.token.is_cancelled());
    }
}
