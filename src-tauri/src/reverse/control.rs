//! 一个批次（包括准备阶段）独占运行槽。租约随任务释放；停止只作用于对应的批次 ID。

use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct BatchController {
    active: Mutex<Option<ActiveBatch>>,
}

struct ActiveBatch {
    reservation: String,
    batch_id: Option<String>,
    cancel: CancellationToken,
}

pub struct BatchLease {
    controller: Arc<BatchController>,
    reservation: String,
    pub cancel: CancellationToken,
}

impl BatchController {
    /// 在任何异步准备工作之前占槽，避免并发启动覆盖取消令牌。
    pub fn reserve(self: &Arc<Self>) -> AppResult<BatchLease> {
        let mut active = self.active.lock();
        if active.is_some() {
            return Err(AppError::new("batch_running", "已有反推批次正在运行，请先停止或等待完成"));
        }
        let reservation = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        *active = Some(ActiveBatch { reservation: reservation.clone(), batch_id: None, cancel: cancel.clone() });
        Ok(BatchLease { controller: self.clone(), reservation, cancel })
    }

    /// 重复停止、已结束的 ID、旧页面发来的 ID 都是无害的空操作。
    pub fn cancel(&self, batch_id: &str) {
        let active = self.active.lock();
        if let Some(batch) = active.as_ref().filter(|b| b.batch_id.as_deref() == Some(batch_id)) {
            batch.cancel.cancel();
        }
    }
}

impl BatchLease {
    pub fn bind(&self, batch_id: &str) {
        let mut active = self.controller.active.lock();
        if let Some(batch) = active.as_mut().filter(|b| b.reservation == self.reservation) {
            batch.batch_id = Some(batch_id.to_string());
        }
    }

    /// 终态落库与停止请求在同一把锁下裁定，最后一道题结束时也不会把已接受的停止写成完成。
    /// 回调不得发事件（事件接收者可能立即发停止/启动请求）；返回之后再通知前端。
    pub fn complete<T>(self, finish: impl FnOnce(bool) -> T) -> T {
        let mut active = self.controller.active.lock();
        let result = finish(self.cancel.is_cancelled());
        if active.as_ref().is_some_and(|b| b.reservation == self.reservation) {
            *active = None;
        }
        result
    }
}

impl Drop for BatchLease {
    fn drop(&mut self) {
        let mut active = self.controller.active.lock();
        if active.as_ref().is_some_and(|b| b.reservation == self.reservation) {
            *active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_includes_preparation_and_drop_releases_failed_preparation() {
        let controller = Arc::new(BatchController::default());
        let preparing = controller.reserve().unwrap();
        assert_eq!(controller.reserve().err().unwrap().code, "batch_running");
        controller.cancel("unknown");
        assert!(!preparing.cancel.is_cancelled());
        drop(preparing); // 配置/钥匙串/prepare 报错的早退路径。
        assert!(controller.reserve().is_ok());
    }

    #[test]
    fn cancellation_is_idempotent_and_stale_id_never_stops_new_batch() {
        let controller = Arc::new(BatchController::default());
        let first = controller.reserve().unwrap();
        first.bind("first");
        controller.cancel("other");
        assert!(!first.cancel.is_cancelled());
        controller.cancel("first");
        controller.cancel("first");
        assert!(first.complete(|cancelled| cancelled));
        let second = controller.reserve().unwrap();
        second.bind("second");
        controller.cancel("first");
        assert!(!second.cancel.is_cancelled());
        assert!(!second.complete(|cancelled| cancelled));
        controller.cancel("second");
        assert!(controller.reserve().is_ok());
    }

    #[test]
    fn simultaneous_start_only_reserves_once_and_completion_wins_or_observes_stop() {
        let controller = Arc::new(BatchController::default());
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let starts: Vec<_> = (0..2).map(|_| {
            let controller = controller.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || { barrier.wait(); controller.reserve() })
        }).collect();
        barrier.wait();
        let leases: Vec<_> = starts.into_iter().filter_map(|t| t.join().unwrap().ok()).collect();
        assert_eq!(leases.len(), 1);
        drop(leases);
        // 多次交错最终完成与停止，两者必须在同一串行点裁定。
        for _ in 0..32 {
            let lease = controller.reserve().unwrap();
            lease.bind("race");
            let token = lease.cancel.clone();
            let canceller = controller.clone();
            let stop = std::thread::spawn(move || canceller.cancel("race"));
            let cancelled = lease.complete(|cancelled| cancelled);
            stop.join().unwrap();
            assert_eq!(cancelled, token.is_cancelled());
        }
    }
}
