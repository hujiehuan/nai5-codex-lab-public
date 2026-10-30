//! Local macOS app-bundle replacement. No database or keychain paths are ever copied.
//!
//! The existing executable runs this helper before Tauri starts, waits for the GUI
//! process to exit, then swaps only the app bundle. Every user-controlled path is
//! passed as an argv element; no shell is involved.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const BUNDLE_ID: &str = "moe.vivian.codexlab";
const HELPER_FLAG: &str = "--internal-apply-update";
const RESULT_FILE: &str = "install-result.json";

#[derive(Debug, Clone)]
pub struct ValidatedBundle {
    pub bundle_path: PathBuf,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallResult {
    pub success: bool,
    pub version: String,
    pub message: String,
    pub backup_path: Option<String>,
    pub finished_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallJob {
    parent_pid: u32,
    candidate: PathBuf,
    target: PathBuf,
    expected_version: String,
    updates_dir: PathBuf,
    ready_file: PathBuf,
}

#[derive(Debug, Serialize)]
struct InstallJournal<'a> {
    stage: &'a str,
    target: &'a Path,
    staged: &'a Path,
    backup: &'a Path,
    version: &'a str,
}

fn io_error(action: &str, error: std::io::Error) -> String {
    // Error kinds are sufficient here, and don't leak source/auth URL arguments.
    format!("{action}：{error}")
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("更新状态路径无效")?;
    fs::create_dir_all(parent).map_err(|e| io_error("无法创建更新状态目录", e))?;
    let tmp = parent.join(format!(".install-state-{}", uuid::Uuid::new_v4()));
    let write = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|e| io_error("无法写入更新状态", e))?;
        let json = serde_json::to_vec(value).map_err(|_| "更新状态无法序列化")?;
        file.write_all(&json)
            .map_err(|e| io_error("无法写入更新状态", e))?;
        file.sync_all()
            .map_err(|e| io_error("无法保存更新状态", e))?;
        fs::rename(&tmp, path).map_err(|e| io_error("无法提交更新状态", e))
    })();
    if write.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let file = File::open(path).map_err(|e| io_error("无法读取安装记录", e))?;
    if file
        .metadata()
        .map_err(|e| io_error("无法读取安装记录", e))?
        .len()
        > 64 * 1024
    {
        return Err("安装记录损坏：文件过大".into());
    }
    serde_json::from_reader(file).map_err(|_| "安装记录格式无效".into())
}

pub fn read_install_result(updates_dir: &Path) -> Result<Option<InstallResult>, String> {
    let path = updates_dir.join(RESULT_FILE);
    if !path.exists() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

/// Archive the consumed result, retaining diagnostics without replaying it on
/// every launch. If there was no final result yet, the caller may poll again.
pub fn take_install_result(updates_dir: &Path) -> Result<Option<InstallResult>, String> {
    // LaunchServices may enter the new app before `open` has returned to the
    // helper. A short bounded wait lets that helper publish its real outcome;
    // missing results are never converted to success. Only fresh helper markers
    // trigger the wait, so archived journals do not delay normal launches.
    let helper_pending = fs::read_dir(updates_dir).ok().is_some_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("installer-")
                && name.ends_with(".ready")
                && entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|time| time.elapsed().ok())
                    .is_some_and(|age| age < Duration::from_secs(300))
        })
    });
    if helper_pending {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !updates_dir.join(RESULT_FILE).exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
    }
    let result = read_install_result(updates_dir)?;
    if result.is_some() {
        fs::rename(
            updates_dir.join(RESULT_FILE),
            updates_dir.join("install-result-last.json"),
        )
        .map_err(|e| io_error("无法归档安装结果", e))?;
    }
    Ok(result)
}

fn expected_cpu() -> Result<u32, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok(0x0100_000c),
        "x86_64" => Ok(0x0100_0007),
        _ => Err("当前架构不支持 Mac 应用内安装".into()),
    }
}

/// Read Mach-O headers directly so validation does not depend on Xcode/lipo.
/// Universal binaries are accepted only if they contain the running architecture.
fn macho_supports(bytes: &[u8], cpu: u32) -> bool {
    fn word(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
        let data: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(data)
        } else {
            u32::from_be_bytes(data)
        })
    }
    let magic = bytes.get(..4);
    let thin = match magic {
        Some([0xce, 0xfa, 0xed, 0xfe] | [0xcf, 0xfa, 0xed, 0xfe]) => Some(true),
        Some([0xfe, 0xed, 0xfa, 0xce] | [0xfe, 0xed, 0xfa, 0xcf]) => Some(false),
        _ => None,
    };
    if let Some(little) = thin {
        return bytes.len() >= 28 && word(bytes, 4, little) == Some(cpu);
    }
    let (little, stride) = match magic {
        Some([0xca, 0xfe, 0xba, 0xbe]) => (false, 20usize),
        Some([0xbe, 0xba, 0xfe, 0xca]) => (true, 20),
        Some([0xca, 0xfe, 0xba, 0xbf]) => (false, 32),
        Some([0xbf, 0xba, 0xfe, 0xca]) => (true, 32),
        _ => return false,
    };
    let Some(count) = word(bytes, 4, little) else {
        return false;
    };
    if count == 0 || count > 64 || bytes.len() < 8 + count as usize * stride {
        return false;
    }
    (0..count as usize).any(|index| word(bytes, 8 + index * stride, little) == Some(cpu))
}

fn plist_value(plist: &Path, key: &str) -> Result<String, String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| io_error("无法检查 Mac 应用信息", e))?;
    if !output.status.success() || output.stdout.len() > 4096 {
        return Err(format!("应用信息缺少或损坏：{key}"));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| format!("应用信息编码无效：{key}"))
}

pub fn validate_candidate(
    bundle: &Path,
    expected_version: &str,
) -> Result<ValidatedBundle, String> {
    if !cfg!(target_os = "macos") {
        return Err("自动安装仅支持 macOS".into());
    }
    let bundle = fs::canonicalize(bundle).map_err(|e| io_error("找不到构建的应用", e))?;
    if bundle.extension().and_then(|x| x.to_str()) != Some("app") || !bundle.is_dir() {
        return Err("构建结果不是有效的 .app 应用".into());
    }
    let plist = bundle.join("Contents/Info.plist");
    if plist_value(&plist, "CFBundleIdentifier")? != BUNDLE_ID {
        return Err("应用标识不匹配，拒绝替换".into());
    }
    if plist_value(&plist, "CFBundleShortVersionString")? != expected_version {
        return Err("构建版本与已确认的更新版本不一致".into());
    }
    let executable_name = plist_value(&plist, "CFBundleExecutable")?;
    // CFBundleExecutable is a filename, never an arbitrary relative path.
    if executable_name.is_empty()
        || Path::new(&executable_name).components().count() != 1
        || executable_name == "."
        || executable_name == ".."
    {
        return Err("应用可执行文件名无效".into());
    }
    let macos = fs::canonicalize(bundle.join("Contents/MacOS"))
        .map_err(|e| io_error("应用缺少可执行目录", e))?;
    let executable = fs::canonicalize(macos.join(executable_name))
        .map_err(|e| io_error("应用缺少可执行文件", e))?;
    if !macos.starts_with(&bundle) || executable.parent() != Some(macos.as_path()) {
        return Err("应用可执行文件指向应用包外，拒绝安装".into());
    }
    let metadata = fs::metadata(&executable).map_err(|e| io_error("无法读取可执行文件", e))?;
    if !metadata.is_file() {
        return Err("应用可执行文件无效".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("应用文件没有执行权限".into());
        }
    }
    let mut header = Vec::new();
    File::open(&executable)
        .map_err(|e| io_error("无法读取应用架构", e))?
        .take(4096)
        .read_to_end(&mut header)
        .map_err(|e| io_error("无法读取应用架构", e))?;
    if !macho_supports(&header, expected_cpu()?) {
        return Err("应用架构与当前运行的 Mac 版本不匹配".into());
    }
    Ok(ValidatedBundle {
        bundle_path: bundle,
        executable,
    })
}

fn bundle_from_executable(executable: &Path) -> Result<PathBuf, String> {
    let macos = executable.parent().ok_or("当前不是已安装的 Mac 应用")?;
    let contents = macos.parent().ok_or("当前不是已安装的 Mac 应用")?;
    let bundle = contents.parent().ok_or("当前不是已安装的 Mac 应用")?;
    if macos.file_name().and_then(|p| p.to_str()) != Some("MacOS")
        || contents.file_name().and_then(|p| p.to_str()) != Some("Contents")
        || bundle.extension().and_then(|p| p.to_str()) != Some("app")
    {
        return Err("当前是开发窗口，请先安装正式 .app 后使用自动安装".into());
    }
    if bundle.starts_with("/Volumes")
        || (bundle.to_string_lossy().contains("/target/")
            && bundle.to_string_lossy().contains("/bundle/"))
    {
        return Err("请先将应用拖到「应用程序」目录并从那里启动，再使用自动安装".into());
    }
    Ok(bundle.to_path_buf())
}

pub fn installed_bundle() -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("自动安装仅支持 macOS".into());
    }
    if cfg!(debug_assertions) {
        return Err("开发构建不能替换应用，请先安装正式 .app".into());
    }
    let executable = std::env::current_exe().map_err(|e| io_error("无法定位当前应用", e))?;
    let executable = fs::canonicalize(executable).map_err(|e| io_error("无法定位当前应用", e))?;
    let bundle = bundle_from_executable(&executable)?;
    let validated = validate_candidate(&bundle, env!("CARGO_PKG_VERSION"))?;
    if validated.executable != executable {
        return Err("当前程序与应用包内执行文件不一致".into());
    }
    let parent = bundle.parent().ok_or("安装目录无效")?;
    let probe = parent.join(format!(".codex-update-check-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&probe).map_err(|_| {
        "当前应用目录不可写，请将应用移至当前用户可写的「应用程序」目录".to_string()
    })?;
    fs::remove_dir(&probe).map_err(|e| io_error("无法清理安装权限检查", e))?;
    Ok(bundle)
}

/// Start helper and wait until it has parsed/validated the job. The caller must
/// finish its database backup first, and only exit the GUI after this succeeds.
pub fn launch_installer(candidate: &Path, version: &str, updates_dir: &Path) -> Result<(), String> {
    let target = installed_bundle()?;
    let candidate = validate_candidate(candidate, version)?.bundle_path;
    if candidate == target || candidate.starts_with(&target) || target.starts_with(&candidate) {
        return Err("更新来源不能是当前安装应用本身".into());
    }
    fs::create_dir_all(updates_dir).map_err(|e| io_error("无法创建安装记录目录", e))?;
    let updates_dir =
        fs::canonicalize(updates_dir).map_err(|e| io_error("无法定位安装记录目录", e))?;
    let id = uuid::Uuid::new_v4();
    let job_path = updates_dir.join(format!("installer-{id}.json"));
    let ready_file = updates_dir.join(format!("installer-{id}.ready"));
    let job = InstallJob {
        parent_pid: std::process::id(),
        candidate,
        target,
        expected_version: version.into(),
        updates_dir: updates_dir.clone(),
        ready_file: ready_file.clone(),
    };
    write_json(&job_path, &job)?;
    // A previous successful install must not be mistaken for this attempt.
    let _ = take_install_result(&updates_dir)?;
    let mut command =
        Command::new(std::env::current_exe().map_err(|e| io_error("无法定位安装助手", e))?);
    command
        .arg(HELPER_FLAG)
        .arg(&job_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| io_error("无法启动安装助手", e))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ready_file.is_file() {
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(_)) => return Err("安装助手启动失败，当前应用未退出；可重试或手动安装".into()),
            Err(e) => return Err(io_error("无法检查安装助手", e)),
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("安装助手未就绪，当前应用未退出".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

trait BundleOps {
    fn validate(&self, bundle: &Path, version: &str) -> Result<(), String>;
    fn copy(&self, source: &Path, target: &Path) -> Result<(), String>;
    fn open(&self, bundle: &Path) -> Result<(), String>;
}

struct MacBundleOps;
impl BundleOps for MacBundleOps {
    fn validate(&self, bundle: &Path, version: &str) -> Result<(), String> {
        validate_candidate(bundle, version).map(|_| ())
    }
    fn copy(&self, source: &Path, target: &Path) -> Result<(), String> {
        let status = Command::new("/usr/bin/ditto")
            .arg(source)
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| io_error("无法复制新版应用", e))?;
        if status.success() {
            Ok(())
        } else {
            Err("复制新版应用失败；请检查磁盘空间和安装目录权限".into())
        }
    }
    fn open(&self, bundle: &Path) -> Result<(), String> {
        let status = Command::new("/usr/bin/open")
            .arg("-n")
            .arg(bundle)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| io_error("无法启动应用", e))?;
        if status.success() {
            Ok(())
        } else {
            Err("macOS 拒绝打开新版应用".into())
        }
    }
}

#[derive(Debug)]
struct InstallFailure {
    message: String,
    backup: Option<PathBuf>,
}

/// Isolated filesystem transaction, also executed against synthetic bundles in
/// tests. Backup bundles are deliberately retained; app data is never involved.
fn replace_bundle(job: &InstallJob, ops: &impl BundleOps) -> Result<PathBuf, InstallFailure> {
    let fail = |message: String| {
        let reopened = job.target.is_dir() && ops.open(&job.target).is_ok();
        InstallFailure {
            message: if reopened {
                format!("{message}；已重新打开旧应用")
            } else {
                format!("{message}；请手动重新打开旧应用")
            },
            backup: None,
        }
    };
    ops.validate(&job.candidate, &job.expected_version)
        .map_err(fail)?;
    let parent = job
        .target
        .parent()
        .ok_or_else(|| fail("安装目录无效".into()))?;
    if !job.target.is_dir() {
        return Err(fail("原应用已移动或删除，停止更新".into()));
    }
    let id = uuid::Uuid::new_v4();
    let name = job.target.file_stem().unwrap_or_default().to_string_lossy();
    let staged = parent.join(format!(".{name}.updating-{id}.app"));
    let backup = parent.join(format!("{name}.previous-{id}.app"));
    let failed = parent.join(format!(".{name}.failed-{id}.app"));
    let journal_path = job.updates_dir.join("install-journal.json");
    let journal = |stage| {
        write_json(
            &journal_path,
            &InstallJournal {
                stage,
                target: &job.target,
                staged: &staged,
                backup: &backup,
                version: &job.expected_version,
            },
        )
    };
    journal("copying").map_err(fail)?;
    if let Err(error) = ops
        .copy(&job.candidate, &staged)
        .and_then(|_| ops.validate(&staged, &job.expected_version))
    {
        let _ = fs::remove_dir_all(&staged);
        return Err(fail(format!("{error}；原应用保持不变")));
    }
    if let Err(error) = journal("switching") {
        let _ = fs::remove_dir_all(&staged);
        return Err(fail(error));
    }
    if let Err(error) = fs::rename(&job.target, &backup) {
        let _ = fs::remove_dir_all(&staged);
        return Err(fail(io_error("无法保留旧应用，停止更新", error)));
    }
    if let Err(error) = fs::rename(&staged, &job.target) {
        let restored = fs::rename(&backup, &job.target);
        let _ = fs::remove_dir_all(&staged);
        if restored.is_ok() {
            let _ = ops.open(&job.target);
        }
        return Err(InstallFailure {
            message: if restored.is_ok() {
                format!("新版切换失败，已恢复旧应用：{error}")
            } else {
                "新版切换失败，旧应用保留在 previous 备份目录，请手动打开".into()
            },
            backup: backup.exists().then_some(backup),
        });
    }
    if let Err(error) = ops.open(&job.target) {
        let moved_new = fs::rename(&job.target, &failed);
        let restored = if moved_new.is_ok() {
            fs::rename(&backup, &job.target).is_ok()
        } else {
            false
        };
        let reopened = restored && ops.open(&job.target).is_ok();
        return Err(InstallFailure {
            message: if reopened {
                format!("{error}；已恢复并重新打开旧应用")
            } else if restored {
                format!("{error}；已恢复旧应用，请手动重新打开")
            } else {
                format!("{error}；旧应用保留在 previous 备份目录，请手动打开")
            },
            backup: backup.exists().then_some(backup),
        });
    }
    let _ = journal("installed");
    Ok(backup)
}

fn wait_for_parent(pid: u32, timeout: Duration) -> Result<(), String> {
    if pid == 0 || pid == std::process::id() {
        return Err("安装助手收到无效的父进程标识".into());
    }
    let deadline = Instant::now() + timeout;
    loop {
        let status = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| io_error("无法等待当前应用退出", e))?;
        if !status.success() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("当前应用未能退出，已停止安装，原应用保持不变".into());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn execute_helper(job_path: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Mac 安装助手不能在此系统运行".into());
    }
    let job: InstallJob = read_json(job_path)?;
    if job_path.parent() != Some(job.updates_dir.as_path())
        || job.ready_file.parent() != Some(job.updates_dir.as_path())
        || !job.target.is_absolute()
        || !job.candidate.is_absolute()
        || job.target == job.candidate
    {
        return Err("安装作业路径无效".into());
    }
    let started = (|| {
        validate_candidate(&job.candidate, &job.expected_version)?;
        // Ready only after every startup check has passed. The parent still runs
        // until it sees this acknowledgement and exits through Tauri.
        write_json(&job.ready_file, &true)?;
        wait_for_parent(job.parent_pid, Duration::from_secs(120))?;
        Ok::<(), String>(())
    })();
    let result = match started {
        Err(error) => InstallResult {
            success: false,
            version: job.expected_version.clone(),
            message: error,
            backup_path: None,
            finished_at: chrono::Utc::now().to_rfc3339(),
        },
        Ok(()) => match replace_bundle(&job, &MacBundleOps) {
            Ok(backup) => InstallResult {
                success: true,
                version: job.expected_version.clone(),
                message: "新版应用已安装并请求启动；旧应用备份已保留".into(),
                backup_path: Some(backup.to_string_lossy().into_owned()),
                finished_at: chrono::Utc::now().to_rfc3339(),
            },
            Err(error) => InstallResult {
                success: false,
                version: job.expected_version.clone(),
                message: error.message,
                backup_path: error.backup.map(|p| p.to_string_lossy().into_owned()),
                finished_at: chrono::Utc::now().to_rfc3339(),
            },
        },
    };
    write_json(&job.updates_dir.join(RESULT_FILE), &result)?;
    let _ = fs::remove_file(&job.ready_file);
    let _ = fs::remove_file(job_path);
    Ok(())
}

/// Must run before Tauri initializes or opens the database. Returning true means
/// this process was the detached installer, so the caller must return immediately.
pub fn run_helper_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_FLAG)) {
        return false;
    }
    if let Some(path) = args.next() {
        if args.next().is_none() {
            let _ = execute_helper(Path::new(&path));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeOps {
        copy_failure: bool,
        invalid_copy: bool,
        fail_new_open: bool,
        opened: RefCell<Vec<String>>,
    }
    impl FakeOps {
        fn normal() -> Self {
            Self {
                copy_failure: false,
                invalid_copy: false,
                fail_new_open: false,
                opened: RefCell::new(Vec::new()),
            }
        }
    }
    impl BundleOps for FakeOps {
        fn validate(&self, bundle: &Path, version: &str) -> Result<(), String> {
            match fs::read_to_string(bundle.join("version")) {
                Ok(found) if found == version => Ok(()),
                _ => Err("fixture bundle version invalid".into()),
            }
        }
        fn copy(&self, source: &Path, target: &Path) -> Result<(), String> {
            fs::create_dir(target).unwrap();
            if self.copy_failure {
                return Err("fixture disk full".into());
            }
            fs::copy(source.join("version"), target.join("version")).unwrap();
            if self.invalid_copy {
                fs::write(target.join("version"), "corrupt").unwrap();
            }
            Ok(())
        }
        fn open(&self, bundle: &Path) -> Result<(), String> {
            let version = fs::read_to_string(bundle.join("version")).unwrap();
            self.opened.borrow_mut().push(version.clone());
            if self.fail_new_open && version == "0.5.9" {
                Err("fixture open rejected".into())
            } else {
                Ok(())
            }
        }
    }
    fn fixture() -> (tempfile::TempDir, InstallJob) {
        let dir = tempfile::tempdir().unwrap();
        // Deliberately include shell metacharacters and spaces: they must remain
        // ordinary filenames throughout the Rust filesystem transaction.
        let target = dir.path().join("NAI5 $(touch pwned) ' Lab.app");
        let candidate = dir.path().join("new.app");
        let updates_dir = dir.path().join("state");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&candidate).unwrap();
        fs::create_dir(&updates_dir).unwrap();
        fs::write(target.join("version"), "0.5.8").unwrap();
        fs::write(candidate.join("version"), "0.5.9").unwrap();
        fs::write(dir.path().join("library.db"), "irreplaceable decisions").unwrap();
        let job = InstallJob {
            parent_pid: 999999,
            candidate,
            target,
            expected_version: "0.5.9".into(),
            ready_file: updates_dir.join("ready"),
            updates_dir,
        };
        (dir, job)
    }
    #[test]
    fn successful_switch_keeps_old_bundle_and_never_changes_database() {
        let (dir, job) = fixture();
        let ops = FakeOps::normal();
        let backup = replace_bundle(&job, &ops).unwrap();
        assert_eq!(
            fs::read_to_string(job.target.join("version")).unwrap(),
            "0.5.9"
        );
        assert_eq!(fs::read_to_string(backup.join("version")).unwrap(), "0.5.8");
        assert_eq!(
            fs::read_to_string(dir.path().join("library.db")).unwrap(),
            "irreplaceable decisions"
        );
        assert_eq!(*ops.opened.borrow(), vec!["0.5.9"]);
        assert!(!dir.path().join("pwned").exists());
    }
    #[test]
    fn copy_failure_keeps_running_version_and_removes_partial_stage() {
        let (dir, job) = fixture();
        let mut ops = FakeOps::normal();
        ops.copy_failure = true;
        assert!(replace_bundle(&job, &ops)
            .unwrap_err()
            .message
            .contains("原应用保持不变"));
        assert_eq!(
            fs::read_to_string(job.target.join("version")).unwrap(),
            "0.5.8"
        );
        assert_eq!(*ops.opened.borrow(), vec!["0.5.8"]);
        assert!(!fs::read_dir(dir.path()).unwrap().any(|p| p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("updating-")));
    }
    #[test]
    fn copied_bundle_is_revalidated_before_old_bundle_moves() {
        let (_dir, job) = fixture();
        let mut ops = FakeOps::normal();
        ops.invalid_copy = true;
        assert!(replace_bundle(&job, &ops).is_err());
        assert_eq!(
            fs::read_to_string(job.target.join("version")).unwrap(),
            "0.5.8"
        );
        assert_eq!(*ops.opened.borrow(), vec!["0.5.8"]);
    }
    #[test]
    fn launch_failure_restores_and_reopens_old_bundle() {
        let (dir, job) = fixture();
        let mut ops = FakeOps::normal();
        ops.fail_new_open = true;
        let error = replace_bundle(&job, &ops).unwrap_err();
        assert!(error.message.contains("已恢复并重新打开旧应用"));
        assert_eq!(
            fs::read_to_string(job.target.join("version")).unwrap(),
            "0.5.8"
        );
        assert_eq!(*ops.opened.borrow(), vec!["0.5.9", "0.5.8"]);
        assert_eq!(
            fs::read_to_string(dir.path().join("library.db")).unwrap(),
            "irreplaceable decisions"
        );
    }
    #[test]
    fn missing_target_cannot_be_recreated_or_claimed_as_updated() {
        let (_dir, job) = fixture();
        let ops = FakeOps::normal();
        fs::remove_dir_all(&job.target).unwrap();
        assert!(replace_bundle(&job, &ops)
            .unwrap_err()
            .message
            .contains("原应用已移动"));
        assert!(!job.target.exists());
    }
    #[test]
    fn install_results_are_consumed_once_and_retained_for_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        assert!(take_install_result(dir.path()).unwrap().is_none());
        let result = InstallResult {
            success: false,
            version: "0.5.9".into(),
            message: "disk full".into(),
            backup_path: None,
            finished_at: "2026-09-21T00:00:00Z".into(),
        };
        write_json(&dir.path().join(RESULT_FILE), &result).unwrap();
        assert!(!take_install_result(dir.path()).unwrap().unwrap().success);
        assert!(take_install_result(dir.path()).unwrap().is_none());
        assert!(dir.path().join("install-result-last.json").is_file());
        fs::write(dir.path().join(RESULT_FILE), "{broken").unwrap();
        assert!(take_install_result(dir.path()).is_err());
        assert!(dir.path().join(RESULT_FILE).exists());
    }
    #[test]
    fn startup_waits_for_real_result_if_open_races_helper_publication() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("installer-fixture.ready"), "true").unwrap();
        // The status polling API remains non-blocking and cannot claim success.
        assert!(read_install_result(dir.path()).unwrap().is_none());
        let updates_dir = dir.path().to_path_buf();
        let helper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            write_json(
                &updates_dir.join(RESULT_FILE),
                &InstallResult {
                    success: true,
                    version: "0.5.9".into(),
                    message: "installed".into(),
                    backup_path: None,
                    finished_at: "2026-09-21T00:00:00Z".into(),
                },
            )
            .unwrap();
        });
        assert!(take_install_result(dir.path()).unwrap().unwrap().success);
        helper.join().unwrap();
    }

    #[test]
    fn mach_o_architecture_and_truncated_universal_binary_are_checked() {
        let arm = 0x0100_000cu32;
        let intel = 0x0100_0007u32;
        let mut thin = vec![0xcf, 0xfa, 0xed, 0xfe];
        thin.extend(arm.to_le_bytes());
        thin.resize(32, 0);
        assert!(macho_supports(&thin, arm));
        assert!(!macho_supports(&thin, intel));
        let mut universal = vec![0xca, 0xfe, 0xba, 0xbe];
        universal.extend(2u32.to_be_bytes());
        universal.extend(intel.to_be_bytes());
        universal.resize(28, 0);
        universal.extend(arm.to_be_bytes());
        universal.resize(48, 0);
        assert!(macho_supports(&universal, arm));
        assert!(macho_supports(&universal, intel));
        universal.truncate(30);
        assert!(!macho_supports(&universal, arm));
        assert!(!macho_supports(b"#!/bin/sh", arm));
    }
    #[test]
    fn development_and_disk_image_paths_cannot_be_replaced() {
        assert!(bundle_from_executable(Path::new("/tmp/target/debug/nai5-codex-lab")).is_err());
        assert!(bundle_from_executable(Path::new(
            "/Volumes/Installer/NAI5.app/Contents/MacOS/app"
        ))
        .is_err());
        assert!(bundle_from_executable(Path::new(
            "/repo/target/release/bundle/macos/NAI5.app/Contents/MacOS/app"
        ))
        .is_err());
        assert_eq!(
            bundle_from_executable(Path::new("/Applications/NAI5.app/Contents/MacOS/app")).unwrap(),
            Path::new("/Applications/NAI5.app")
        );
    }
    #[test]
    fn current_process_is_not_accepted_as_its_own_parent() {
        assert!(wait_for_parent(0, Duration::ZERO).is_err());
        assert!(wait_for_parent(std::process::id(), Duration::ZERO).is_err());
    }
}
