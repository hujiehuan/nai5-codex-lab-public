//! Source updates from the project repository. All build commands run in an owned detached
//! worktree. The source checkout and the application's database are never changed.
use crate::error::{AppError, AppResult};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

const OWNER_REPO: &str = "hujiehuan/nai5-codex-lab-public";
const LOG_LINES: usize = 100;
const CAPTURE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct UpdateCandidate {
    pub version: String,
    pub revision: String,
    pub tag: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppUpdateStatus {
    pub supported: bool,
    pub phase: String,
    pub current_version: String,
    pub current_revision: Option<String>,
    pub source_dir: Option<String>,
    pub candidate: Option<UpdateCandidate>,
    pub stage: Option<String>,
    pub message: Option<String>,
    pub log_tail: Vec<String>,
    pub started_at: Option<String>,
    pub can_install: bool,
}

#[derive(Clone, Debug)]
pub struct PreparedUpdate {
    pub candidate: UpdateCandidate,
    pub bundle_path: PathBuf,
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedState {
    status: AppUpdateStatus,
    bundle_path: Option<PathBuf>,
    job: Option<PathBuf>,
}

struct Inner {
    root: PathBuf,
    saved: Mutex<SavedState>,
    cancel: AtomicBool,
    process: Mutex<Option<u32>>,
    // Tool paths are discovered once per command invocation, never by sourcing a shell profile.
    #[cfg(test)]
    test_path: Option<OsString>,
    #[cfg(test)]
    test_validator: Option<fn(&Path, &str) -> AppResult<()>>,
}

#[derive(Clone)]
pub struct UpdateController {
    inner: Arc<Inner>,
}

impl UpdateController {
    pub fn new(data_dir: PathBuf) -> AppResult<Self> {
        match Self::create(data_dir.clone(), cfg!(target_os = "macos")) {
            Ok(controller) => Ok(controller),
            Err(error) => {
                // Updates are optional: a broken directory must not stop existing curation work.
                let status = AppUpdateStatus {
                    supported: cfg!(target_os = "macos"),
                    phase: "error".into(),
                    current_version: env!("CARGO_PKG_VERSION").into(),
                    current_revision: option_env!("APP_BUILD_REVISION")
                        .filter(|v| valid_sha(v))
                        .map(str::to_owned),
                    source_dir: None,
                    candidate: None,
                    stage: Some("更新记录不可用".into()),
                    message: Some(format!(
                        "更新目录无法使用，现有功能仍可使用：{}",
                        redact(&error.message)
                    )),
                    log_tail: vec![],
                    started_at: None,
                    can_install: false,
                };
                Ok(Self {
                    inner: Arc::new(Inner {
                        root: data_dir.join("updates"),
                        saved: Mutex::new(SavedState {
                            status,
                            bundle_path: None,
                            job: None,
                        }),
                        cancel: AtomicBool::new(false),
                        process: Mutex::new(None),
                        #[cfg(test)]
                        test_path: None,
                        #[cfg(test)]
                        test_validator: None,
                    }),
                })
            }
        }
    }

    fn create(data_dir: PathBuf, supported: bool) -> AppResult<Self> {
        let root = data_dir.join("updates");
        fs::create_dir_all(root.join("jobs"))?;
        let state_file = root.join("state.json");
        let revision = option_env!("APP_BUILD_REVISION")
            .filter(|v| valid_sha(v))
            .map(str::to_owned);
        let initial = AppUpdateStatus {
            supported,
            phase: "idle".into(),
            current_version: env!("CARGO_PKG_VERSION").into(),
            current_revision: revision.clone(),
            source_dir: None,
            candidate: None,
            stage: None,
            message: None,
            log_tail: vec![],
            started_at: None,
            can_install: false,
        };
        let mut saved = match fs::read(&state_file) {
            Ok(bytes) => match serde_json::from_slice::<SavedState>(&bytes) {
                Ok(saved) => saved,
                Err(_) => SavedState {
                    status: AppUpdateStatus {
                        phase: "error".into(),
                        message: Some("更新记录无法读取，请重新选择源码目录并检查更新".into()),
                        ..initial.clone()
                    },
                    bundle_path: None,
                    job: None,
                },
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SavedState {
                status: initial.clone(),
                bundle_path: None,
                job: None,
            },
            Err(e) => return Err(AppError::io(format!("无法读取更新记录：{e}"))),
        };
        saved.status.supported = supported;
        saved.status.current_version = initial.current_version;
        saved.status.current_revision = revision;
        let awaiting_install_result = saved.status.phase == "installing";
        if matches!(
            saved.status.phase.as_str(),
            "checking" | "preparing" | "installing"
        ) {
            saved.status.phase = "error".into();
            saved.status.stage = Some("interrupted".into());
            saved.status.message =
                Some("上次更新被中断；没有自动恢复构建或安装，请重新检查".into());
            saved.bundle_path = None;
        }
        // Startup must retain the expected revision until AppState consumes the
        // helper result and confirms the new binary, even when it is now current.
        if !awaiting_install_result {
            discard_obsolete_candidate(&mut saved);
        }
        if saved.status.phase == "ready"
            && !saved
                .bundle_path
                .as_ref()
                .is_some_and(|p| p.is_dir() && p.starts_with(root.join("jobs")))
        {
            saved.status.phase = "error".into();
            saved.status.message = Some("准备好的应用已不存在，请重新准备更新".into());
            saved.bundle_path = None;
        }
        saved.status.can_install =
            supported && saved.status.phase == "ready" && saved.bundle_path.is_some();
        if !supported {
            saved.status.message = Some("源码自动更新仅支持 macOS 桌面应用".into());
        }
        let this = Self {
            inner: Arc::new(Inner {
                root,
                saved: Mutex::new(saved),
                cancel: AtomicBool::new(false),
                process: Mutex::new(None),
                #[cfg(test)]
                test_path: None,
                #[cfg(test)]
                test_validator: None,
            }),
        };
        this.persist()?;
        Ok(this)
    }

    pub fn status(&self) -> AppUpdateStatus {
        self.inner.saved.lock().status.clone()
    }

    fn persist_locked(&self, saved: &SavedState) -> AppResult<()> {
        let tmp = self.inner.root.join("state.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(saved)?)?;
        fs::rename(tmp, self.inner.root.join("state.json"))?;
        Ok(())
    }
    fn persist(&self) -> AppResult<()> {
        self.persist_locked(&self.inner.saved.lock())
    }

    fn idle_guard(saved: &SavedState) -> AppResult<()> {
        if !saved.status.supported {
            return Err(AppError::new(
                "update_unsupported",
                "源码自动更新仅支持 macOS",
            ));
        }
        if matches!(
            saved.status.phase.as_str(),
            "checking" | "preparing" | "installing"
        ) {
            return Err(AppError::new("update_busy", "已有更新操作正在进行"));
        }
        Ok(())
    }

    fn reserve(&self, phase: &str, stage: &str) -> AppResult<()> {
        let mut saved = self.inner.saved.lock();
        Self::idle_guard(&saved)?;
        let previous = saved.clone();
        saved.status.phase = phase.into();
        saved.status.stage = Some(stage.into());
        saved.status.message = None;
        saved.status.log_tail.clear();
        saved.status.started_at = Some(chrono::Utc::now().to_rfc3339());
        saved.status.can_install = false;
        self.inner.cancel.store(false, Ordering::SeqCst);
        if let Err(e) = self.persist_locked(&saved) {
            *saved = previous;
            return Err(e);
        }
        Ok(())
    }

    /// Called after the native directory picker. Validation failure keeps the previous source.
    pub fn set_source(&self, directory: PathBuf) -> AppResult<AppUpdateStatus> {
        self.reserve("checking", "验证源码目录")?;
        let result = (|| {
            let source = fs::canonicalize(directory)
                .map_err(|_| AppError::bad_request("所选源码目录不存在或无法读取"))?;
            let tools = Tools::discover(self)?;
            self.validate_source(&tools, &source)?;
            self.check_cancel()?;
            let mut saved = self.inner.saved.lock();
            saved.status.source_dir = Some(source.to_string_lossy().into());
            saved.status.candidate = None;
            saved.bundle_path = None;
            saved.status.phase = "idle".into();
            saved.status.stage = None;
            saved.status.message = Some("源码目录已验证，可以检查稳定版本".into());
            self.persist_locked(&saved)?;
            Ok(saved.status.clone())
        })();
        match result {
            Ok(value) => Ok(value),
            Err(e) => {
                self.finish_error(&e);
                Err(e)
            }
        }
    }

    pub fn start_check(&self) -> AppResult<AppUpdateStatus> {
        self.reserve("checking", "检查稳定版本")?;
        let initial = self.status();
        let this = self.clone();
        thread::spawn(move || {
            if let Err(e) = this.check() {
                this.finish_error(&e);
            }
        });
        Ok(initial)
    }

    pub fn start_prepare(&self) -> AppResult<AppUpdateStatus> {
        {
            let mut saved = self.inner.saved.lock();
            Self::idle_guard(&saved)?;
            if let Some(message) = discard_obsolete_candidate(&mut saved) {
                self.persist_locked(&saved)?;
                return Err(AppError::new("update_obsolete", message));
            }
            if saved.status.candidate.is_none() {
                return Err(AppError::bad_request("请先检查并选择可用的稳定版本"));
            }
            if saved.status.phase == "ready" {
                return Err(AppError::bad_request(
                    "更新已准备好，请安装或先重新检查版本",
                ));
            }
            if saved.status.phase == "up_to_date" {
                return Err(AppError::bad_request("当前已经是最新稳定版本"));
            }
        }
        self.reserve("preparing", "核验构建环境")?;
        let initial = self.status();
        let this = self.clone();
        thread::spawn(move || {
            if let Err(e) = this.prepare() {
                this.finish_error(&e);
            }
        });
        Ok(initial)
    }

    pub fn cancel(&self) -> AppResult<AppUpdateStatus> {
        let phase = self.inner.saved.lock().status.phase.clone();
        if phase == "installing" {
            return Err(AppError::new(
                "update_busy",
                "正在交接安装助手，不能取消应用替换",
            ));
        }
        if matches!(phase.as_str(), "checking" | "preparing") {
            self.shutdown();
            self.stage("正在停止本次更新进程")?;
        }
        Ok(self.status())
    }

    /// Called on actual application exit as well as cancellation. Only this
    /// controller's currently owned subprocess group can be signalled.
    pub fn shutdown(&self) {
        let process = self.inner.process.lock();
        self.inner.cancel.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        if let Some(pid) = *process {
            signal_group(pid, 15);
            signal_group(pid, 9);
        }
    }

    pub fn begin_install(&self) -> AppResult<PreparedUpdate> {
        let mut saved = self.inner.saved.lock();
        Self::idle_guard(&saved)?;
        if let Some(message) = discard_obsolete_candidate(&mut saved) {
            self.persist_locked(&saved)?;
            return Err(AppError::new("update_obsolete", message));
        }
        if saved.status.phase != "ready" || !saved.status.can_install {
            return Err(AppError::bad_request("更新尚未准备完成"));
        }
        let candidate = saved
            .status
            .candidate
            .clone()
            .ok_or_else(|| AppError::bad_request("缺少已冻结的版本"))?;
        let bundle_path = saved
            .bundle_path
            .clone()
            .ok_or_else(|| AppError::bad_request("缺少构建应用"))?;
        if !bundle_path.is_dir() || !bundle_path.starts_with(self.inner.root.join("jobs")) {
            return Err(AppError::bad_request("构建应用路径无效，请重新准备更新"));
        }
        let previous = saved.clone();
        saved.status.phase = "installing".into();
        saved.status.stage = Some("等待备份和安装交接".into());
        saved.status.message = None;
        saved.status.can_install = false;
        if let Err(e) = self.persist_locked(&saved) {
            *saved = previous;
            return Err(e);
        }
        Ok(PreparedUpdate {
            candidate,
            bundle_path,
        })
    }

    pub fn finish_install_error(&self, message: &str) {
        let mut saved = self.inner.saved.lock();
        saved.status.phase = if saved.bundle_path.is_some() {
            "ready"
        } else {
            "error"
        }
        .into();
        saved.status.stage = Some("安装未完成".into());
        saved.status.message = Some(redact(message));
        saved.status.can_install = saved.status.supported && saved.bundle_path.is_some();
        if self.persist_locked(&saved).is_err() {
            saved.status.message = Some("安装失败，且更新状态无法保存；原软件仍保留".into());
        }
    }

    pub fn record_install_result(&self, success: bool, message: &str) {
        let mut saved = self.inner.saved.lock();
        saved.status.phase = if success { "idle" } else { "error" }.into();
        saved.status.stage = Some(
            if success {
                "安装完成"
            } else {
                "安装未完成"
            }
            .into(),
        );
        saved.status.message = Some(redact(message));
        saved.status.can_install = false;
        saved.bundle_path = None;
        let _ = self.persist_locked(&saved);
    }

    fn stage(&self, stage: &str) -> AppResult<()> {
        let mut saved = self.inner.saved.lock();
        saved.status.stage = Some(stage.into());
        self.persist_locked(&saved)
    }
    fn log(&self, line: &str) {
        let line = redact(line);
        if line.trim().is_empty() {
            return;
        }
        let mut saved = self.inner.saved.lock();
        saved
            .status
            .log_tail
            .push(line.chars().take(1200).collect());
        let excess = saved.status.log_tail.len().saturating_sub(LOG_LINES);
        saved.status.log_tail.drain(..excess);
    }
    fn check_cancel(&self) -> AppResult<()> {
        if self.inner.cancel.load(Ordering::SeqCst) {
            Err(AppError::cancelled())
        } else {
            Ok(())
        }
    }
    fn finish_error(&self, e: &AppError) {
        let mut saved = self.inner.saved.lock();
        saved.status.phase = if e.code == "cancelled" {
            "cancelled"
        } else {
            "error"
        }
        .into();
        saved.status.can_install = false;
        saved.status.message = Some(redact(&e.message));
        if self.persist_locked(&saved).is_err() {
            saved.status.message = Some(format!("{}；更新状态保存失败", redact(&e.message)));
        }
    }
    fn source(&self) -> AppResult<PathBuf> {
        self.inner
            .saved
            .lock()
            .status
            .source_dir
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| AppError::bad_request("请先选择本机 nai5-codex-lab 源码目录"))
    }

    fn validate_source(&self, tools: &Tools, source: &Path) -> AppResult<()> {
        let top = self.git(tools, source, &["rev-parse", "--show-toplevel"], false)?;
        let canonical_top = fs::canonicalize(top.trim())
            .map_err(|_| AppError::bad_request("所选目录不是完整 Git 源码仓库"))?;
        if canonical_top != source {
            return Err(AppError::bad_request("请选择源码仓库根目录，不是其子目录"));
        }
        let remote = self.git(tools, source, &["remote", "get-url", "origin"], false)?;
        let host = repository_host(remote.trim())?;
        if host != "github.com"
            || (!remote.trim().starts_with("https://") && !remote.trim().contains("git@"))
        {
            let ssh = tools.find("ssh")?;
            let config = self.run(
                tools,
                &ssh,
                &[
                    OsString::from("-G"),
                    OsString::from(if remote.trim().contains("git@") {
                        format!("git@{host}")
                    } else {
                        host.clone()
                    }),
                ],
                source,
                false,
                Duration::from_secs(15),
            )?;
            let resolved = config
                .lines()
                .find_map(|line| line.strip_prefix("hostname "))
                .unwrap_or("");
            let user = config
                .lines()
                .find_map(|line| line.strip_prefix("user "))
                .unwrap_or("");
            if !resolved.eq_ignore_ascii_case("github.com") || user != "git" {
                return Err(AppError::bad_request(
                    "Git SSH 别名没有指向 github.com，请选择指定仓库",
                ));
            }
        }
        for rel in [
            "package.json",
            "package-lock.json",
            "src-tauri/tauri.conf.json",
            "src-tauri/Cargo.toml",
        ] {
            if !source.join(rel).is_file() {
                return Err(AppError::bad_request(format!("源码目录缺少 {rel}")));
            }
        }
        Ok(())
    }
    fn remote_candidate(&self, tools: &Tools, source: &Path) -> AppResult<UpdateCandidate> {
        let tags = self.git(
            tools,
            source,
            &["ls-remote", "--tags", "origin", "refs/tags/v*"],
            false,
        )?;
        newest_candidate(&tags)
    }
    fn check(&self) -> AppResult<()> {
        let source = self.source()?;
        let tools = Tools::discover(self)?;
        self.validate_source(&tools, &source)?;
        let candidate = self.remote_candidate(&tools, &source)?;
        self.check_cancel()?;
        let mut saved = self.inner.saved.lock();
        let current = version_tuple(&format!("v{}", saved.status.current_version));
        let newer = version_tuple(&candidate.tag) > current
            || (version_tuple(&candidate.tag) == current
                && saved
                    .status
                    .current_revision
                    .as_ref()
                    .is_some_and(|r| r != &candidate.revision));
        saved.status.phase = if newer { "available" } else { "up_to_date" }.into();
        saved.status.stage = None;
        saved.status.message = Some(if newer {
            format!("发现稳定版本 {}", candidate.version)
        } else {
            "已核验远端稳定标签，当前无需更新".into()
        });
        saved.status.candidate = Some(candidate);
        saved.bundle_path = None;
        self.persist_locked(&saved)
    }

    fn prepare(&self) -> AppResult<()> {
        let source = self.source()?;
        let candidate = self
            .inner
            .saved
            .lock()
            .status
            .candidate
            .clone()
            .ok_or_else(|| AppError::bad_request("缺少目标版本"))?;
        let tools = Tools::discover(self)?;
        self.validate_source(&tools, &source)?;
        for name in ["node", "npm", "cargo", "rustc"] {
            let tool = tools.find(name)?;
            self.run(
                &tools,
                &tool,
                &["--version".into()],
                &source,
                true,
                Duration::from_secs(30),
            )?;
        }
        #[cfg(target_os = "macos")]
        self.run(
            &tools,
            &tools.find("xcode-select")?,
            &["-p".into()],
            &source,
            true,
            Duration::from_secs(30),
        )?;
        if candidate.tag != format!("v{}", candidate.version)
            || version_tuple(&candidate.tag).is_none()
            || !valid_sha(&candidate.revision)
        {
            return Err(AppError::bad_request("冻结版本记录无效，请重新检查"));
        }
        self.stage("复核目标标签")?;
        self.verify_tag(&tools, &source, &candidate)?;
        self.stage("清理本软件上次构建目录")?;
        self.cleanup_old_jobs(&tools)?;
        let job = self
            .inner
            .root
            .join("jobs")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&job)?;
        fs::write(
            job.join(".codex-update-owned"),
            candidate.revision.as_bytes(),
        )?;
        fs::write(job.join("source.json"), serde_json::to_vec(&source)?)?;
        let checkout = job.join("checkout");
        {
            let mut saved = self.inner.saved.lock();
            saved.job = Some(job.clone());
            saved.bundle_path = None;
            self.persist_locked(&saved)?;
        }
        let result = (|| {
            self.stage("获取冻结版本")?;
            // A private ref leaves existing local tags and branches untouched.
            let private_ref = format!(
                "refs/codex-updates/{}",
                job.file_name().unwrap().to_string_lossy()
            );
            let refspec = format!("refs/tags/{}:{private_ref}", candidate.tag);
            self.git(
                &tools,
                &source,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "origin",
                    &refspec,
                ],
                true,
            )?;
            let fetched = self.git(
                &tools,
                &source,
                &["rev-parse", &format!("{private_ref}^{{commit}}")],
                false,
            )?;
            if fetched.trim() != candidate.revision {
                return Err(AppError::new(
                    "update_tag_changed",
                    "目标标签已改变，已停止。请重新检查版本",
                ));
            }
            self.verify_tag(&tools, &source, &candidate)?;
            self.stage("创建独立构建目录")?;
            self.git_os(
                &tools,
                &source,
                &[
                    "-c".into(),
                    "core.hooksPath=/dev/null".into(),
                    "worktree".into(),
                    "add".into(),
                    "--detach".into(),
                    checkout.as_os_str().into(),
                    candidate.revision.clone().into(),
                ],
                true,
            )?;
            let head = self.git(&tools, &checkout, &["rev-parse", "HEAD"], false)?;
            if head.trim() != candidate.revision {
                return Err(AppError::bad_request("独立目录的提交与冻结版本不一致"));
            }
            validate_manifest_versions(&checkout, &candidate.version)?;
            self.stage("安装锁定的前端依赖")?;
            let npm = tools.find("npm")?;
            self.run(
                &tools,
                &npm,
                &["ci".into(), "--no-audit".into(), "--no-fund".into()],
                &checkout,
                true,
                Duration::from_secs(30 * 60),
            )?;
            self.stage("编译 macOS 应用")?;
            // CARGO_TARGET_DIR is set per invocation; no dependency/build output goes into the source checkout.
            self.run(
                &tools,
                &npm,
                &[
                    "run".into(),
                    "tauri".into(),
                    "--".into(),
                    "build".into(),
                    "--bundles".into(),
                    "app".into(),
                ],
                &checkout,
                true,
                Duration::from_secs(3 * 60 * 60),
            )?;
            self.check_cancel()?;
            self.stage("校验构建应用")?;
            let bundle = find_bundle(&job.join("target/release/bundle/macos"))?;
            self.validate_bundle(&bundle, &candidate.version)?;
            let mut saved = self.inner.saved.lock();
            saved.bundle_path = Some(bundle);
            saved.status.phase = "ready".into();
            saved.status.stage = Some("已准备完成".into());
            saved.status.message =
                Some("新版本已构建并校验。点击「安装并重启」才会替换应用".into());
            saved.status.can_install = saved.status.supported;
            self.persist_locked(&saved)
        })();
        // Failed/cancelled jobs are removed before the next explicit prepare, never by an untracked process after cancellation.
        result
    }

    fn validate_bundle(&self, bundle: &Path, version: &str) -> AppResult<()> {
        #[cfg(test)]
        if let Some(validate) = self.inner.test_validator {
            return validate(bundle, version);
        }
        crate::update_installer::validate_candidate(bundle, version)
            .map(|_| ())
            .map_err(|e| AppError::new("update_bundle", e))
    }

    fn verify_tag(
        &self,
        tools: &Tools,
        source: &Path,
        candidate: &UpdateCandidate,
    ) -> AppResult<()> {
        let tag = format!("refs/tags/{}", candidate.tag);
        let peeled = format!("{tag}^{{}}");
        let raw = self.git(
            tools,
            source,
            &["ls-remote", "--tags", "origin", &tag, &peeled],
            false,
        )?;
        if newest_candidate(&raw)? != *candidate {
            return Err(AppError::new(
                "update_tag_changed",
                "目标稳定标签已改变，请重新检查后再构建",
            ));
        }
        Ok(())
    }

    fn cleanup_old_jobs(&self, tools: &Tools) -> AppResult<()> {
        let protected = {
            let saved = self.inner.saved.lock();
            saved.bundle_path.as_ref().and_then(|bundle| {
                self.inner
                    .root
                    .join("jobs")
                    .read_dir()
                    .ok()?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .find(|job| bundle.starts_with(job))
            })
        };
        for entry in fs::read_dir(self.inner.root.join("jobs"))? {
            self.check_cancel()?;
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let job = entry.path();
            if protected.as_ref() == Some(&job)
                || uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err()
            {
                continue;
            }
            if !fs::read_to_string(job.join(".codex-update-owned")).is_ok_and(|sha| valid_sha(&sha))
            {
                continue;
            }
            let Ok(bytes) = fs::read(job.join("source.json")) else {
                continue;
            };
            let Ok(source) = serde_json::from_slice::<PathBuf>(&bytes) else {
                continue;
            };
            let checkout = job.join("checkout");
            if source.is_dir() {
                if checkout.exists() {
                    self.git_os(
                        tools,
                        &source,
                        &[
                            "-c".into(),
                            "core.hooksPath=/dev/null".into(),
                            "worktree".into(),
                            "remove".into(),
                            "--force".into(),
                            checkout.as_os_str().into(),
                        ],
                        false,
                    )?;
                }
                let reference =
                    format!("refs/codex-updates/{}", entry.file_name().to_string_lossy());
                self.git(
                    tools,
                    &source,
                    &[
                        "-c",
                        "core.hooksPath=/dev/null",
                        "update-ref",
                        "-d",
                        &reference,
                    ],
                    false,
                )?;
            }
            // This UUID directory was created by this updater; no caller-supplied cleanup path.
            fs::remove_dir_all(&job)?;
        }
        Ok(())
    }

    fn git(&self, tools: &Tools, source: &Path, args: &[&str], log: bool) -> AppResult<String> {
        self.git_os(
            tools,
            source,
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            log,
        )
    }
    fn git_os(
        &self,
        tools: &Tools,
        source: &Path,
        args: &[OsString],
        log: bool,
    ) -> AppResult<String> {
        let mut all = vec![
            OsString::from("-C"),
            source.as_os_str().into(),
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
            "-c".into(),
            "maintenance.auto=false".into(),
            "-c".into(),
            "gc.auto=0".into(),
        ];
        all.extend_from_slice(args);
        self.run(
            tools,
            &tools.git,
            &all,
            source,
            log,
            Duration::from_secs(180),
        )
    }

    fn run(
        &self,
        tools: &Tools,
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        log: bool,
        timeout: Duration,
    ) -> AppResult<String> {
        self.check_cancel()?;
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(cwd)
            .env("PATH", &tools.path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("CI", "true")
            .env("NO_COLOR", "1")
            .env("CARGO_TERM_COLOR", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(job) = self
            .inner
            .saved
            .lock()
            .job
            .as_ref()
            .filter(|job| cwd.starts_with(job))
        {
            command.env("CARGO_TARGET_DIR", job.join("target"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut owned_process = self.inner.process.lock();
        self.check_cancel()?;
        let mut child = command.spawn().map_err(|e| {
            AppError::new(
                "update_tool",
                format!(
                    "无法启动 {}：{e}",
                    executable.file_name().unwrap_or_default().to_string_lossy()
                ),
            )
        })?;
        let pid = child.id();
        *owned_process = Some(pid);
        drop(owned_process);
        let (tx, rx) = mpsc::channel();
        for (is_stdout, pipe) in [
            (
                true,
                child
                    .stdout
                    .take()
                    .map(|v| Box::new(v) as Box<dyn Read + Send>),
            ),
            (
                false,
                child
                    .stderr
                    .take()
                    .map(|v| Box::new(v) as Box<dyn Read + Send>),
            ),
        ] {
            if let Some(pipe) = pipe {
                let tx = tx.clone();
                thread::spawn(move || {
                    let mut reader = BufReader::new(pipe);
                    loop {
                        // Split huge compiler/tool lines to keep telemetry memory bounded.
                        let mut bytes = Vec::new();
                        match reader
                            .by_ref()
                            .take(16 * 1024)
                            .read_until(b'\n', &mut bytes)
                        {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                if tx
                                    .send((is_stdout, String::from_utf8_lossy(&bytes).into_owned()))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
        }
        drop(tx);
        let started = Instant::now();
        let mut output = String::new();
        let mut error_tail = String::new();
        let mut overflow = false;
        loop {
            for (is_stdout, line) in rx.try_iter() {
                if log {
                    self.log(&line);
                }
                if is_stdout {
                    if output.len() + line.len() <= CAPTURE_BYTES {
                        output.push_str(&line);
                    } else {
                        overflow = true;
                    }
                }
                if !is_stdout {
                    error_tail = redact(&line).chars().take(600).collect();
                }
            }
            if self.inner.cancel.load(Ordering::SeqCst) || started.elapsed() > timeout {
                let mut owned_process = self.inner.process.lock();
                terminate_group(&mut child, pid);
                *owned_process = None;
                drop(owned_process);
                return if self.inner.cancel.load(Ordering::SeqCst) {
                    Err(AppError::cancelled())
                } else {
                    Err(AppError::new(
                        "update_timeout",
                        "更新命令超时，已停止本次进程；可检查网络或工具链后重试",
                    ))
                };
            }
            let status = {
                let mut owned_process = self.inner.process.lock();
                match child.try_wait() {
                    Ok(Some(status)) => Some(status),
                    Ok(None) => None,
                    Err(e) => {
                        terminate_group(&mut child, pid);
                        *owned_process = None;
                        return Err(e.into());
                    }
                }
            };
            if let Some(status) = status {
                // The child is reaped; drain remaining reader output without waiting for inherited pipes forever.
                let drain_until = Instant::now() + Duration::from_secs(2);
                let mut streams_closed = false;
                while Instant::now() < drain_until {
                    match rx.recv_timeout(Duration::from_millis(10)) {
                        Ok((is_stdout, line)) => {
                            if log {
                                self.log(&line);
                            }
                            if is_stdout {
                                if output.len() + line.len() <= CAPTURE_BYTES {
                                    output.push_str(&line);
                                } else {
                                    overflow = true;
                                }
                            }
                            if !is_stdout {
                                error_tail = redact(&line).chars().take(600).collect();
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            streams_closed = true;
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                if !streams_closed {
                    let mut active = self.inner.process.lock();
                    terminate_group(&mut child, pid);
                    *active = None;
                    return Err(AppError::new(
                        "update_output",
                        "更新命令退出后仍有子进程占用输出，已停止本次进程",
                    ));
                }
                *self.inner.process.lock() = None;
                self.check_cancel()?;
                if overflow {
                    return Err(AppError::new(
                        "update_output",
                        "更新工具输出超过可读取范围，已停止而未截断版本检查",
                    ));
                }
                if status.success() {
                    return Ok(output);
                }
                return Err(AppError::new(
                    "update_command",
                    format!(
                        "{} 执行失败（{}）。{}",
                        executable.file_name().unwrap_or_default().to_string_lossy(),
                        status,
                        error_tail
                    ),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

struct Tools {
    path: OsString,
    git: PathBuf,
}
impl Tools {
    fn discover(controller: &UpdateController) -> AppResult<Self> {
        #[cfg(test)]
        if let Some(path) = &controller.inner.test_path {
            let git = find_tool(path, "git")?;
            return Ok(Self {
                path: path.clone(),
                git,
            });
        }
        let _ = controller;
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            dirs.extend([
                home.join(".cargo/bin"),
                home.join(".volta/bin"),
                home.join(".asdf/shims"),
                home.join(".local/share/mise/shims"),
                home.join("miniconda3/bin"),
                home.join("anaconda3/bin"),
                home.join("miniforge3/bin"),
            ]);
            if let Ok(entries) = fs::read_dir(home.join(".nvm/versions/node")) {
                let mut versions = entries
                    .filter_map(Result::ok)
                    .map(|e| e.path().join("bin"))
                    .collect::<Vec<_>>();
                versions.sort_by_key(|path| {
                    path.parent()
                        .and_then(Path::file_name)
                        .and_then(OsStr::to_str)
                        .and_then(version_tuple)
                });
                versions.reverse();
                dirs.extend(versions);
            }
        }
        dirs.extend(
            [
                "/opt/homebrew/bin",
                "/opt/anaconda3/bin",
                "/opt/miniconda3/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
            ]
            .map(PathBuf::from),
        );
        dirs.retain(|p| p.is_absolute());
        let path = std::env::join_paths(dirs)
            .map_err(|_| AppError::new("update_tool", "工具搜索路径无效"))?;
        Ok(Self {
            git: find_tool(&path, "git")?,
            path,
        })
    }
    fn find(&self, name: &str) -> AppResult<PathBuf> {
        find_tool(&self.path, name)
    }
}
fn find_tool(path: &OsStr, name: &str) -> AppResult<PathBuf> {
    std::env::split_paths(path).map(|p| p.join(name)).find(|p| {
        if !p.is_file() { return false; }
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) }
        #[cfg(not(unix))] { true }
    }).ok_or_else(|| AppError::new("update_tool", format!("未找到 {name}。请先在 Mac 安装或启用 Node.js、Rust 和 Xcode 命令行工具；不会自动安装系统依赖")))
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // `pid` is the child just spawned into a new process group, never a persisted/arbitrary PID.
    if let Ok(pid) = i32::try_from(pid) {
        unsafe {
            kill(-pid, signal);
        }
    }
}
fn terminate_group(child: &mut std::process::Child, pid: u32) {
    #[cfg(unix)]
    signal_group(pid, 15);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    thread::sleep(Duration::from_millis(200));
    #[cfg(unix)]
    signal_group(pid, 9);
    let _ = child.kill();
    let _ = child.wait();
}

/// Compare against this running binary, not the version persisted by the old app.
/// Clearing a stale local candidate does not claim that the remote was checked.
fn discard_obsolete_candidate(saved: &mut SavedState) -> Option<&'static str> {
    let candidate = saved.status.candidate.as_ref()?;
    let current_version = version_tuple(&format!("v{}", saved.status.current_version))?;
    let candidate_version = version_tuple(&format!("v{}", candidate.version))?;
    let message = if candidate_version < current_version {
        "旧更新记录的版本低于当前软件，已清除以防降级；请重新检查远端版本"
    } else if candidate_version == current_version
        && saved
            .status
            .current_revision
            .as_ref()
            .is_some_and(|revision| revision.eq_ignore_ascii_case(&candidate.revision))
    {
        "已运行此构建，旧待安装记录已清除；可重新检查远端版本"
    } else {
        // Unknown revision deliberately permits an explicit same-version rebuild.
        return None;
    };
    saved.status.phase = "idle".into();
    saved.status.stage = None;
    saved.status.message = Some(message.into());
    saved.status.candidate = None;
    saved.status.can_install = false;
    saved.bundle_path = None;
    Some(message)
}

fn version_tuple(tag: &str) -> Option<(u64, u64, u64)> {
    let parts = tag.strip_prefix('v')?.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|v| {
            v.is_empty()
                || !v.bytes().all(|b| b.is_ascii_digit())
                || (v.len() > 1 && v.starts_with('0'))
        })
    {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}
fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}
fn newest_candidate(raw: &str) -> AppResult<UpdateCandidate> {
    let mut tags: BTreeMap<(u64, u64, u64), (String, Option<String>, Option<String>)> =
        BTreeMap::new();
    for line in raw.lines() {
        let Some((sha, name)) = line.split_once('\t') else {
            continue;
        };
        if !valid_sha(sha) {
            continue;
        }
        let Some(name) = name.strip_prefix("refs/tags/") else {
            continue;
        };
        let peeled = name.ends_with("^{}");
        let tag = name.strip_suffix("^{}").unwrap_or(name);
        let Some(version) = version_tuple(tag) else {
            continue;
        };
        let entry = tags
            .entry(version)
            .or_insert_with(|| (tag.into(), None, None));
        if peeled {
            entry.2 = Some(sha.into());
        } else {
            entry.1 = Some(sha.into());
        }
    }
    let (_, (tag, direct, peeled)) = tags.into_iter().next_back().ok_or_else(|| {
        AppError::new(
            "update_no_release",
            "远端没有有效的稳定 vX.Y.Z 标签；没有把检查失败当作最新版",
        )
    })?;
    let revision = peeled
        .or(direct)
        .ok_or_else(|| AppError::new("update_no_release", "稳定标签缺少提交"))?;
    Ok(UpdateCandidate {
        version: tag[1..].into(),
        revision,
        tag,
    })
}

/// Accept the one repository only; SSH aliases must additionally resolve to GitHub.
fn repository_host(remote: &str) -> AppResult<String> {
    let bad =
        || AppError::bad_request("所选仓库的 origin 不是指定的 hujiehuan/nai5-codex-lab-public 仓库");
    let (host, path) = if let Some(rest) = remote.strip_prefix("https://") {
        let (authority, path) = rest.split_once('/').ok_or_else(bad)?;
        let host = authority.rsplit('@').next().unwrap_or("");
        if host != "github.com"
            || (!remote.trim().starts_with("https://") && !remote.trim().contains("git@"))
        {
            return Err(bad());
        }
        (host, path)
    } else if let Some(rest) = remote.strip_prefix("ssh://git@") {
        rest.split_once('/').ok_or_else(bad)?
    } else if let Some(rest) = remote.strip_prefix("git@") {
        rest.split_once(':').ok_or_else(bad)?
    } else if !remote.contains('/')
        || remote
            .find(':')
            .is_some_and(|i| remote.find('/').is_some_and(|j| i < j))
    {
        remote.split_once(':').ok_or_else(bad)?
    } else {
        return Err(bad());
    };
    if host.is_empty()
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        || path.strip_suffix(".git").unwrap_or(path) != OWNER_REPO
    {
        return Err(bad());
    }
    Ok(host.into())
}

fn validate_manifest_versions(checkout: &Path, version: &str) -> AppResult<()> {
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(checkout.join("package.json"))?)?;
    let tauri: serde_json::Value =
        serde_json::from_slice(&fs::read(checkout.join("src-tauri/tauri.conf.json"))?)?;
    if package.get("name").and_then(|v| v.as_str()) != Some("nai5-codex-lab")
        || package.get("version").and_then(|v| v.as_str()) != Some(version)
        || tauri.get("identifier").and_then(|v| v.as_str()) != Some("moe.vivian.codexlab")
        || tauri.get("version").and_then(|v| v.as_str()) != Some(version)
    {
        return Err(AppError::new(
            "update_version",
            "标签版本与软件包版本或应用标识不一致，已停止构建",
        ));
    }
    if !checkout.join("package-lock.json").is_file() {
        return Err(AppError::bad_request(
            "冻结版本缺少 package-lock.json，不能执行锁定安装",
        ));
    }
    Ok(())
}
fn find_bundle(directory: &Path) -> AppResult<PathBuf> {
    let found = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|s| s == "app"))
        .collect::<Vec<_>>();
    if found.len() != 1 {
        return Err(AppError::new(
            "update_bundle",
            "构建结果必须包含且只包含一个 .app",
        ));
    }
    Ok(found[0].clone())
}
fn redact(text: &str) -> String {
    use std::sync::OnceLock;
    static URL: OnceLock<regex::Regex> = OnceLock::new();
    static SECRET: OnceLock<regex::Regex> = OnceLock::new();
    let clean = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect::<String>();
    let clean = URL
        .get_or_init(|| regex::Regex::new(r"(?i)(?:https?://|ssh://|git@)[^\s<>]+\b").unwrap())
        .replace_all(&clean, "[远程地址已隐藏]");
    SECRET.get_or_init(|| regex::Regex::new(r"(?i)(?:bearer\s+\S+|(?:token|password|passwd|authorization|_auth|_authToken)\s*[:=]\s*\S+|(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]+)").unwrap()).replace_all(&clean, "[凭据已隐藏]").into_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn real_git(source: &Path, args: &[&str]) -> String {
        let out = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(source)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    struct Fixture {
        _temp: TempDir,
        source: PathBuf,
        remote: PathBuf,
        tools: PathBuf,
        updater: UpdateController,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let source = temp.path().join("source with spaces");
            let remote = temp.path().join("remote.git");
            let tools = temp.path().join("tools");
            fs::create_dir(&source).unwrap();
            fs::create_dir(&tools).unwrap();
            real_git(&source, &["init", "-q"]);
            real_git(
                &source,
                &["config", "user.email", "synthetic@example.invalid"],
            );
            real_git(&source, &["config", "user.name", "Synthetic Test"]);
            fs::create_dir(source.join("src-tauri")).unwrap();
            fs::write(
                source.join("package.json"),
                r#"{"name":"nai5-codex-lab","version":"9.0.0"}"#,
            )
            .unwrap();
            fs::write(source.join("package-lock.json"), "{}").unwrap();
            fs::write(
                source.join("src-tauri/tauri.conf.json"),
                r#"{"identifier":"moe.vivian.codexlab","version":"9.0.0"}"#,
            )
            .unwrap();
            fs::write(
                source.join("src-tauri/Cargo.toml"),
                "[package]\nname='synthetic'\nversion='9.0.0'\n",
            )
            .unwrap();
            real_git(&source, &["add", "."]);
            real_git(&source, &["commit", "-qm", "fixture"]);
            real_git(&source, &["tag", "-a", "v9.0.0", "-m", "stable"]);
            real_git(
                &source,
                &[
                    "clone",
                    "--bare",
                    source.to_str().unwrap(),
                    remote.to_str().unwrap(),
                ],
            );
            real_git(
                &source,
                &[
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/hujiehuan/nai5-codex-lab-public.git",
                ],
            );
            // Redirect only this synthetic wrapper's origin to the temporary bare repo.
            executable(&tools.join("git"), &format!("#!/usr/bin/python3\nimport os,sys\na=[{} if v=='origin' and ('ls-remote' in sys.argv or 'fetch' in sys.argv) else v for v in sys.argv[1:]]\nos.execv('/usr/bin/git',['git']+a)\n", serde_json::to_string(remote.to_str().unwrap()).unwrap()));
            for name in ["node", "cargo", "rustc"] {
                executable(
                    &tools.join(name),
                    "#!/bin/sh\nprintf 'synthetic-tool 1.0\\n'\n",
                );
            }
            executable(&tools.join("npm"), "#!/bin/sh\nif [ \"$1\" = '--version' ]; then printf 'synthetic-npm 1.0\\n'; exit 0; fi\nif [ \"$1\" = 'ci' ]; then printf 'lockfile install completed\\n'; exit 0; fi\nmkdir -p \"$CARGO_TARGET_DIR/release/bundle/macos/Synthetic.app\"\nprintf 'valid 9.0.0' > \"$CARGO_TARGET_DIR/release/bundle/macos/Synthetic.app/marker\"\n");
            let mut updater = UpdateController::create(temp.path().join("data"), true).unwrap();
            let inner = Arc::get_mut(&mut updater.inner).unwrap();
            inner.test_path = Some(
                std::env::join_paths([
                    tools.clone(),
                    PathBuf::from("/usr/bin"),
                    PathBuf::from("/bin"),
                ])
                .unwrap(),
            );
            inner.test_validator = Some(|p, version| {
                if fs::read_to_string(p.join("marker")).unwrap_or_default()
                    == format!("valid {version}")
                {
                    Ok(())
                } else {
                    Err(AppError::bad_request("invalid synthetic bundle"))
                }
            });
            updater.set_source(source.clone()).unwrap();
            Self {
                _temp: temp,
                source,
                remote,
                tools,
                updater,
            }
        }
        fn wait(&self) -> AppUpdateStatus {
            let start = Instant::now();
            loop {
                let status = self.updater.status();
                if !matches!(status.phase.as_str(), "checking" | "preparing") {
                    return status;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(15),
                    "updater timed out: {:?}",
                    status
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    #[test]
    fn unavailable_updates_directory_does_not_block_app_startup() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("updates"), "not a directory").unwrap();
        let controller = UpdateController::new(temp.path().to_owned()).unwrap();
        assert_eq!(controller.status().phase, "error");
        assert!(controller
            .status()
            .message
            .unwrap()
            .contains("现有功能仍可使用"));
        assert!(!controller.status().can_install);
    }

    #[test]
    fn stable_tags_use_semver_and_peeled_commit_and_reject_bad_remotes() {
        let a = "1".repeat(40);
        let b = "2".repeat(40);
        let c = "3".repeat(40);
        let raw = format!("{a}\trefs/tags/v1.9.0\n{b}\trefs/tags/v1.10.0\n{c}\trefs/tags/v1.10.0^{{}}\n{a}\trefs/tags/v99.0.0-beta\n{a}\trefs/tags/v01.0.0\n");
        assert_eq!(
            newest_candidate(&raw).unwrap(),
            UpdateCandidate {
                tag: "v1.10.0".into(),
                version: "1.10.0".into(),
                revision: c
            }
        );
        for url in [
            "https://github.com/hujiehuan/nai5-codex-lab-public.git",
            "git@github-alias:hujiehuan/nai5-codex-lab-public.git",
            "github-alias:hujiehuan/nai5-codex-lab-public.git",
            "ssh://git@github.com/hujiehuan/nai5-codex-lab-public",
        ] {
            assert!(repository_host(url).is_ok());
        }
        for url in [
            "/tmp/other",
            "https://evil.invalid/hujiehuan/nai5-codex-lab-public",
            "https://github.com/other/nai5-codex-lab",
            "git@-G:hujiehuan/nai5-codex-lab-public.git",
            "https://github.com/hujiehuan/nai5-codex-lab-public.git?token=x",
        ] {
            assert!(repository_host(url).is_err());
        }
    }

    #[test]
    fn check_prepare_preserves_dirty_source_and_freezes_annotated_commit() {
        let f = Fixture::new();
        fs::write(f.source.join("uncommitted.txt"), "irreplaceable user work").unwrap();
        fs::write(f.source.join("package.json"), "local modified file").unwrap();
        let before = real_git(&f.source, &["status", "--porcelain"]);
        let head = real_git(&f.source, &["rev-parse", "HEAD"]);
        assert_eq!(f.updater.start_check().unwrap().phase, "checking");
        let checked = f.wait();
        assert_eq!(checked.phase, "available", "{checked:?}");
        assert_eq!(checked.candidate.as_ref().unwrap().revision, head.trim());
        assert_eq!(f.updater.start_prepare().unwrap().phase, "preparing");
        assert_eq!(f.updater.start_check().unwrap_err().code, "update_busy");
        let ready = f.wait();
        assert_eq!(ready.phase, "ready", "{ready:?}");
        assert!(ready.can_install);
        assert_eq!(real_git(&f.source, &["status", "--porcelain"]), before);
        assert_eq!(real_git(&f.source, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read_to_string(f.source.join("package.json")).unwrap(),
            "local modified file"
        );
        let prepared = f.updater.begin_install().unwrap();
        assert!(prepared
            .bundle_path
            .starts_with(f.updater.inner.root.join("jobs")));
        assert_eq!(f.updater.status().phase, "installing");
        f.updater.finish_install_error("backup failed");
        assert_eq!(f.updater.status().phase, "ready");
        assert!(f.updater.status().can_install);
    }

    #[test]
    fn moved_release_tag_is_rejected_before_dependency_install() {
        let f = Fixture::new();
        f.updater.start_check().unwrap();
        assert_eq!(f.wait().phase, "available");
        fs::write(f.source.join("new.txt"), "new release").unwrap();
        real_git(&f.source, &["add", "."]);
        real_git(&f.source, &["commit", "-qm", "new"]);
        real_git(&f.source, &["tag", "-f", "v9.0.0"]);
        real_git(
            &f.source,
            &[
                "push",
                "--force",
                f.remote.to_str().unwrap(),
                "refs/tags/v9.0.0",
            ],
        );
        f.updater.start_prepare().unwrap();
        let failed = f.wait();
        assert_eq!(failed.phase, "error");
        assert!(failed.message.unwrap().contains("标签已改变"));
        assert!(!failed
            .log_tail
            .iter()
            .any(|s| s.contains("lockfile install")));
    }

    #[test]
    fn auth_failure_is_error_not_up_to_date_and_logs_are_redacted() {
        let f = Fixture::new();
        executable(&f.tools.join("git"), "#!/bin/sh\nprintf 'fatal: https://private:secret@github.com/repo token=secret github_pat_SENSITIVE' >&2\nexit 128\n");
        f.updater.start_check().unwrap();
        let status = f.wait();
        assert_eq!(status.phase, "error");
        let message = status.message.unwrap();
        assert!(!message.contains("secret"));
        assert!(!message.contains("SENSITIVE"));
        assert!(!message.contains("https://"));
        let old = f.updater.status().source_dir;
        assert!(f
            .updater
            .set_source(f.source.join("no-such-directory"))
            .is_err());
        assert_eq!(f.updater.status().source_dir, old);
    }

    #[test]
    fn stale_ready_candidate_cannot_downgrade_or_reinstall_running_commit() {
        let temp = tempfile::tempdir().unwrap();
        let controller = UpdateController::create(temp.path().to_owned(), true).unwrap();
        let bundle = controller
            .inner
            .root
            .join("jobs/synthetic-ready/Synthetic.app");
        fs::create_dir_all(&bundle).unwrap();
        let mut ready = controller.inner.saved.lock().clone();
        ready.status.phase = "ready".into();
        ready.status.can_install = true;
        ready.status.candidate = Some(UpdateCandidate {
            version: "0.0.0".into(),
            revision: "a".repeat(40),
            tag: "v0.0.0".into(),
        });
        ready.bundle_path = Some(bundle);
        controller.persist_locked(&ready).unwrap();
        let resumed = UpdateController::create(temp.path().to_owned(), true).unwrap();
        assert_eq!(resumed.status().phase, "idle");
        assert!(resumed.status().candidate.is_none());
        assert!(!resumed.status().can_install);
        assert!(resumed.status().message.unwrap().contains("防降级"));
        assert!(resumed.inner.process.lock().is_none());
        // Both entry points defend independently from constructor recovery.
        *controller.inner.saved.lock() = ready.clone();
        assert_eq!(
            controller.start_prepare().unwrap_err().code,
            "update_obsolete"
        );
        *controller.inner.saved.lock() = ready.clone();
        assert_eq!(
            controller.begin_install().unwrap_err().code,
            "update_obsolete"
        );
        ready.status.candidate.as_mut().unwrap().version = ready.status.current_version.clone();
        ready.status.candidate.as_mut().unwrap().tag = format!("v{}", ready.status.current_version);
        ready.status.current_revision = Some("a".repeat(40));
        *controller.inner.saved.lock() = ready.clone();
        assert_eq!(
            controller.start_prepare().unwrap_err().code,
            "update_obsolete"
        );
        assert_eq!(controller.status().phase, "idle");
        assert!(controller
            .status()
            .message
            .unwrap()
            .contains("已运行此构建"));
        *controller.inner.saved.lock() = ready.clone();
        assert_eq!(
            controller.begin_install().unwrap_err().code,
            "update_obsolete"
        );
        // Same version with unknown/different build revision is not silently blocked.
        for revision in [None, Some("b".repeat(40))] {
            ready.status.current_revision = revision;
            *controller.inner.saved.lock() = ready.clone();
            assert!(controller.begin_install().is_ok());
        }
        // A newly launched installed build still needs its frozen expected SHA
        // so AppState can verify the helper's result (including same-version updates).
        if let Some(revision) = option_env!("APP_BUILD_REVISION").filter(|r| valid_sha(r)) {
            ready.status.phase = "installing".into();
            ready.status.candidate.as_mut().unwrap().revision = revision.into();
            controller.persist_locked(&ready).unwrap();
            let installed = UpdateController::create(temp.path().to_owned(), true).unwrap();
            let status = installed.status();
            assert_eq!(
                status.candidate.as_ref().map(|c| &c.revision),
                status.current_revision.as_ref()
            );
            assert!(status.candidate.is_some());
            installed.record_install_result(true, "installed");
            assert_eq!(installed.status().phase, "idle");
        }
    }

    #[test]
    fn restart_marks_active_operation_interrupted_without_spawning() {
        let f = Fixture::new();
        f.updater.reserve("preparing", "synthetic pending").unwrap();
        let resumed =
            UpdateController::create(f.updater.inner.root.parent().unwrap().to_owned(), true)
                .unwrap();
        assert_eq!(resumed.status().phase, "error");
        assert_eq!(resumed.status().stage.as_deref(), Some("interrupted"));
        assert!(!resumed.status().can_install);
        assert!(resumed.inner.process.lock().is_none());
        assert_eq!(resumed.status().source_dir, f.updater.status().source_dir);
    }

    #[test]
    fn next_prepare_removes_only_owned_old_jobs_and_their_private_refs() {
        let f = Fixture::new();
        f.updater.start_check().unwrap();
        f.wait();
        f.updater.start_prepare().unwrap();
        assert_eq!(f.wait().phase, "ready");
        let old_job = f.updater.inner.saved.lock().job.clone().unwrap();
        let old_ref = format!(
            "refs/codex-updates/{}",
            old_job.file_name().unwrap().to_string_lossy()
        );
        assert!(!real_git(&f.source, &["for-each-ref", &old_ref]).is_empty());
        let unrelated = f.updater.inner.root.join("jobs/user-work");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), "user material").unwrap();
        // Explicitly rechecking abandons the old candidate; the next prepare can reclaim that build.
        f.updater.start_check().unwrap();
        f.wait();
        f.updater.start_prepare().unwrap();
        assert_eq!(f.wait().phase, "ready");
        assert!(!old_job.exists());
        assert!(real_git(&f.source, &["for-each-ref", &old_ref]).is_empty());
        assert_eq!(
            fs::read_to_string(unrelated.join("keep")).unwrap(),
            "user material"
        );
        assert!(f.updater.status().can_install);
    }

    #[test]
    fn complete_stdout_is_drained_and_oversized_output_fails_explicitly() {
        let f = Fixture::new();
        let tools = Tools::discover(&f.updater).unwrap();
        let helper = f.tools.join("output");
        executable(
            &helper,
            "#!/usr/bin/python3\nfor n in range(5000): print(n)\n",
        );
        let all = f
            .updater
            .run(
                &tools,
                &helper,
                &[],
                &f.source,
                false,
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(all.lines().count(), 5000);
        assert_eq!(all.lines().last(), Some("4999"));
        executable(
            &helper,
            "#!/usr/bin/python3\nprint('x' * (3 * 1024 * 1024))\n",
        );
        assert_eq!(
            f.updater
                .run(
                    &tools,
                    &helper,
                    &[],
                    &f.source,
                    false,
                    Duration::from_secs(5)
                )
                .unwrap_err()
                .code,
            "update_output"
        );
    }

    #[test]
    fn cancellation_kills_owned_process_group_and_keeps_source_untouched() {
        let f = Fixture::new();
        let pid_file = f._temp.path().join("child.pid");
        executable(&f.tools.join("npm"), &format!("#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo 1; exit 0; fi\nsleep 60 &\necho $! > {}\nwait\n", shell_quote_for_fixture(pid_file.to_str().unwrap())));
        f.updater.start_check().unwrap();
        f.wait();
        f.updater.start_prepare().unwrap();
        let start = Instant::now();
        while !pid_file.exists() {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(20));
        }
        f.updater.cancel().unwrap();
        let status = f.wait();
        assert_eq!(status.phase, "cancelled", "{status:?}");
        assert!(f.updater.inner.process.lock().is_none());
        assert!(real_git(&f.source, &["status", "--porcelain"]).is_empty());
        // On Linux an already killed child may briefly be a zombie; it is not running.
        #[cfg(target_os = "linux")]
        {
            let pid = fs::read_to_string(pid_file).unwrap();
            let state =
                fs::read_to_string(format!("/proc/{}/stat", pid.trim())).unwrap_or_default();
            assert!(
                state.is_empty() || state.split_whitespace().nth(2) == Some("Z"),
                "child still running: {state}"
            );
        }
    }
    fn shell_quote_for_fixture(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
