//! Installation barrier shared by IPC requests and their detached workers.
use std::sync::Arc;
use parking_lot::Mutex;
use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct ActivityGate(Mutex<Counts>);

#[derive(Default)]
struct Counts { work: usize, installing: bool }

pub struct WorkLease(Arc<ActivityGate>);
pub struct InstallLease(Arc<ActivityGate>);

impl ActivityGate {
    pub fn enter(self: &Arc<Self>) -> AppResult<WorkLease> {
        let mut counts = self.0.lock();
        if counts.installing {
            return Err(AppError::new("update_installing", "正在备份并安装更新，请等待应用重新打开"));
        }
        counts.work += 1;
        Ok(WorkLease(self.clone()))
    }

    pub fn install(self: &Arc<Self>) -> AppResult<InstallLease> {
        let mut counts = self.0.lock();
        if counts.installing {
            return Err(AppError::new("update_installing", "更新安装已经开始"));
        }
        if counts.work != 0 {
            return Err(AppError::new("update_busy", "还有任务或保存请求正在进行，请完成后再安装；已构建的更新会保留"));
        }
        counts.installing = true;
        Ok(InstallLease(self.clone()))
    }
}

impl Drop for WorkLease {
    fn drop(&mut self) { self.0.0.lock().work -= 1; }
}
impl Drop for InstallLease {
    fn drop(&mut self) { self.0.0.lock().installing = false; }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_waits_for_last_worker_and_blocks_new_work_until_failure_releases_it() {
        let gate = Arc::new(ActivityGate::default());
        let a = gate.enter().unwrap();
        let b = gate.enter().unwrap();
        assert_eq!(gate.install().err().unwrap().code, "update_busy");
        drop(a);
        assert!(gate.install().is_err());
        drop(b);
        let install = gate.install().unwrap();
        assert!(gate.enter().is_err());
        assert!(gate.install().is_err());
        drop(install);
        assert!(gate.enter().is_ok());
    }
    #[test]
    fn a_detached_worker_owns_the_barrier_until_it_finishes() {
        let gate = Arc::new(ActivityGate::default());
        let lease = gate.enter().unwrap();
        let (finish, wait) = std::sync::mpsc::channel();
        let task = std::thread::spawn(move || { let _lease = lease; wait.recv().unwrap(); });
        assert!(gate.install().is_err());
        finish.send(()).unwrap(); task.join().unwrap();
        assert!(gate.install().is_ok());
    }
}
