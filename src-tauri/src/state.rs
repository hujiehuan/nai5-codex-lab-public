//! AppState：数据目录与子目录、SQLite 连接、reqwest 客户端、对 assets 主机的全局节流
//! （Semaphore(4) + ≥100 ms 间隔，CONTRACT §0）、同步任务句柄与状态、tracing 按天滚动日志。
//! 首次启动：建目录、插入内置 `plugin v1` 模板/规则；每天一份 `VACUUM INTO` 备份（设计 §6）延后到
//! 启动 5 s 后在阻塞线程里做（`schedule_daily_backup`）。`inflight` 是图片下载的在飞集合：主同步与
//! 按需下载共用，同一 (entry_id, ordinal, kind) 同一时刻只允许一个任务在下。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::AppHandle;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_util::sync::CancellationToken;

use crate::db::{self, Db};
use crate::error::{AppError, AppResult};
use crate::models::{Settings, SyncStatus};
use crate::reverse::control::BatchController;
use crate::style::StyleState;
use crate::sync::images::InflightKey;

/// 正在下载中的图片键集合（v0.1.1 #4）。
pub type InflightSet = Arc<Mutex<HashSet<InflightKey>>>;

pub const USER_AGENT: &str = concat!("nai5-codex-lab/", env!("CARGO_PKG_VERSION"));
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SUBDIRS: &[&str] = &["codex_raw", "images", "originals", "thumbs", "backups", "exports", "logs", "labruns", "models"];

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// 对 `assets.quicktagcloud.com` 的全局节流：并发 ≤ 4，相邻请求发出时刻间隔 ≥ 100 ms。
pub struct Throttle {
    sem: Semaphore,
    last: tokio::sync::Mutex<Instant>,
    gap: Duration,
}

impl Throttle {
    pub fn new(concurrency: usize, gap: Duration) -> Self {
        Throttle {
            sem: Semaphore::new(concurrency),
            last: tokio::sync::Mutex::new(Instant::now() - gap),
            gap,
        }
    }

    /// 拿到许可并等够间隔后返回；许可在返回值 drop 时释放。
    pub async fn acquire(&self) -> SemaphorePermit<'_> {
        let permit = self.sem.acquire().await.expect("throttle semaphore closed");
        let wait = {
            let mut last = self.last.lock().await;
            let now = Instant::now();
            let next = std::cmp::max(now, *last + self.gap);
            *last = next;
            next.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        permit
    }
}

/// 同步/按需下载任务的共享控制块。
pub struct SyncController {
    pub running: AtomicBool,
    pub status: Mutex<SyncStatus>,
    pub cancel: Mutex<CancellationToken>,
}

impl SyncController {
    pub fn new() -> Self {
        SyncController {
            running: AtomicBool::new(false),
            status: Mutex::new(SyncStatus::default()),
            cancel: Mutex::new(CancellationToken::new()),
        }
    }

    /// 原子地从「空闲」切到「运行」；已在跑返回 false。
    pub fn try_start(&self) -> bool {
        self.running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
        let mut s = self.status.lock();
        s.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn fresh_token(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.cancel.lock() = token.clone();
        token
    }

    pub fn cancel(&self) {
        self.cancel.lock().cancel();
    }

    pub fn update(&self, f: impl FnOnce(&mut SyncStatus)) {
        let mut s = self.status.lock();
        f(&mut s);
    }

    pub fn snapshot(&self) -> SyncStatus {
        self.status.lock().clone()
    }
}

impl Default for SyncController {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AppState {
    pub activity: Arc<crate::activity::ActivityGate>,
    pub updater: crate::updater::UpdateController,
    pub app: AppHandle,
    pub data_dir: PathBuf,
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    pub throttle: Arc<Throttle>,
    pub sync: Arc<SyncController>,
    pub batch: Arc<BatchController>,
    pub inflight: InflightSet,
    /// v0.3.0（§12）：画风模型缓存 + 模型下载的取消令牌；同步的索引阶段与 `style_*` 命令共用。
    pub style: Arc<StyleState>,
}

impl AppState {
    pub fn init(app: AppHandle, data_dir: PathBuf) -> AppResult<AppState> {
        std::fs::create_dir_all(&data_dir)?;
        for sub in SUBDIRS {
            std::fs::create_dir_all(data_dir.join(sub))?;
        }
        init_tracing(&data_dir.join("logs"));
        tracing::info!(version = APP_VERSION, data_dir = %data_dir.display(), "app starting");

        let db = Arc::new(Db::open(&data_dir.join("library.db"))?);
        db.with(|c| crate::reverse::builtin::ensure_builtin(c))?;

        let http = build_http_client()?;
        let updater = crate::updater::UpdateController::new(data_dir.clone())?;
        match crate::update_installer::take_install_result(&data_dir.join("updates")) {
            Ok(Some(result)) => {
                let update_status = updater.status();
                let expected = update_status.candidate.as_ref().map(|c| c.revision.as_str());
                let succeeded = result.success && result.version == APP_VERSION
                    && expected.is_some() && expected == update_status.current_revision.as_deref();
                updater.record_install_result(succeeded, if result.success && !succeeded {
                    "安装助手已完成，但当前版本或构建编号与目标不一致或无法核验；请检查打开的应用位置"
                } else { &result.message });
            }
            Ok(None) => {},
            Err(error) => updater.record_install_result(false, &format!("无法读取上次安装结果：{error}")),
        }
        Ok(AppState {
            activity: Arc::new(crate::activity::ActivityGate::default()),
            updater,
            app,
            data_dir,
            db,
            http,
            throttle: Arc::new(Throttle::new(4, Duration::from_millis(100))),
            sync: Arc::new(SyncController::new()),
            batch: Arc::new(BatchController::default()),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            style: Arc::new(StyleState::new()),
        })
    }

    /// 每日备份延后到启动 5 s 后、在阻塞线程里做，不再挡住窗口出现（v0.1.1 #14）。
    /// 由 `lib.rs` 的 `setup` 在 `app.manage` 之后调用一次。
    pub fn schedule_daily_backup(&self) {
        let db = self.db.clone();
        let data_dir = self.data_dir.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Err(e) = tokio::task::spawn_blocking(move || daily_backup(&db, &data_dir)).await {
                tracing::error!("每日备份任务异常退出: {e}");
            }
        });
    }

    pub fn settings(&self) -> AppResult<Settings> {
        load_settings(&self.db)
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("library.db")
    }
}

pub fn build_http_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::network(e.to_string()))
}

pub fn load_settings(db: &Db) -> AppResult<Settings> {
    match db.app_state_get("settings")? {
        Some(raw) => {
            // 缺键用默认值补齐，避免旧版本设置读不出来。
            let mut base = serde_json::to_value(Settings::default())?;
            if let (Some(b), Ok(serde_json::Value::Object(patch))) = (base.as_object_mut(), serde_json::from_str::<serde_json::Value>(&raw)) {
                for (k, v) in patch {
                    b.insert(k, v);
                }
            }
            Ok(serde_json::from_value(base)?)
        }
        None => Ok(Settings::default()),
    }
}

pub fn save_settings(db: &Db, s: &Settings) -> AppResult<()> {
    db.app_state_set("settings", &serde_json::to_string(s)?)
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use crate::models::StyleDevicePreference;

    #[test]
    fn old_settings_default_to_auto_and_device_choice_persists() {
        let db = Db::open_memory().unwrap();
        db.app_state_set("settings", r#"{"plugin_base_url":"http://localhost:6185","retired_key":false}"#).unwrap();
        let mut settings = load_settings(&db).unwrap();
        assert_eq!(settings.style_device, StyleDevicePreference::Auto);
        assert_eq!(settings.download_concurrency, 4, "缺键补默认，已退役的键忽略");
        for device in [StyleDevicePreference::Cpu, StyleDevicePreference::Metal, StyleDevicePreference::Auto] {
            settings.style_device = device;
            save_settings(&db, &settings).unwrap();
            let loaded = load_settings(&db).unwrap();
            assert_eq!(loaded.style_device, device);
            assert_eq!(loaded.plugin_base_url, "http://localhost:6185");
        }
        assert!(serde_json::from_value::<StyleDevicePreference>(serde_json::json!("cuda")).is_err());
    }
}

/// 每天一份整库备份（保留 30 份）；文件名日期已是今天则跳过。
fn daily_backup(db: &Db, data_dir: &Path) {
    let backups = data_dir.join("backups");
    let today = chrono::Utc::now().format("%Y%m%d").to_string();
    if db::latest_daily_backup_stamp(&backups).as_deref() == Some(today.as_str()) {
        return;
    }
    match db.backup_into(&backups, None, 30) {
        Ok(p) => db.log_event("info", "backup", "每日备份完成", serde_json::json!({ "path": p })),
        Err(e) => db.log_event("error", "backup", &format!("每日备份失败: {e}"), serde_json::Value::Null),
    }
}

/// tracing → `logs/nai5-codex-lab.YYYY-MM-DD.log`（JSON 行，按天滚动），同时输出到 stderr。
fn init_tracing(logs_dir: &Path) {
    use tracing_subscriber::prelude::*;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("nai5-codex-lab")
        .filename_suffix("log")
        .build(logs_dir);
    let Ok(appender) = appender else { return };
    let (writer, guard) = tracing_appender::non_blocking(appender);
    if LOG_GUARD.set(guard).is_err() {
        return; // 已初始化过
    }
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,nai5_codex_lab_lib=debug"));
    let file_layer = tracing_subscriber::fmt::layer().json().with_writer(writer).with_ansi(false);
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr).compact();
    let _ = tracing_subscriber::registry().with(filter).with(file_layer).with(stderr_layer).try_init();
}
