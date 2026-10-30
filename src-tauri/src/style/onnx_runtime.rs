//! 分类器使用的官方 ONNX Runtime 1.22 动态库，按平台下载并校验后安装。
//! 不依赖用户安装 Python；只提取固定文件名，不展开归档里的任意路径或符号链接。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use super::catalog::ModelFile;
use super::download::{download_checked, DownloadEmitter};

struct RuntimeSpec {
    target: &'static str,
    archive_root: &'static str,
    library: &'static str,
    archive: ModelFile,
}

const LINUX: RuntimeSpec = RuntimeSpec {
    target: "linux-x64-1.22.0", archive_root: "onnxruntime-linux-x64-1.22.0", library: "libonnxruntime.so.1.22.0",
    archive: ModelFile {
        name: "onnxruntime-linux-x64-1.22.0.tgz",
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/onnxruntime-linux-x64-1.22.0.tgz",
        bytes: 7798730, sha256: Some("8344d55f93d5bc5021ce342db50f62079daf39aaafb5d311a451846228be49b3"),
    },
};
const MAC_ARM: RuntimeSpec = RuntimeSpec {
    target: "osx-arm64-1.22.0", archive_root: "onnxruntime-osx-arm64-1.22.0", library: "libonnxruntime.1.22.0.dylib",
    archive: ModelFile {
        name: "onnxruntime-osx-arm64-1.22.0.tgz",
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/onnxruntime-osx-arm64-1.22.0.tgz",
        bytes: 25943843, sha256: Some("cab6dcbd77e7ec775390e7b73a8939d45fec3379b017c7cb74f5b204c1a1cc07"),
    },
};

fn platform() -> AppResult<&'static RuntimeSpec> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok(&MAC_ARM),
        ("linux", "x86_64") => Ok(&LINUX),
        _ => Err(AppError::bad_request("画师分类器目前支持 Apple Silicon Mac 和 Linux x86_64")),
    }
}

fn runtime_dir(data_dir: &Path, spec: &RuntimeSpec) -> PathBuf {
    data_dir.join("models").join("onnxruntime").join(spec.target)
}

pub fn is_ready(data_dir: &Path) -> bool {
    let Ok(spec) = platform() else { return false };
    let dir = runtime_dir(data_dir, spec);
    let marker = std::fs::read_to_string(dir.join("verified.sha256")).unwrap_or_default();
    marker.trim() == spec.archive.sha256.unwrap_or_default()
        && std::fs::metadata(dir.join(spec.library)).is_ok_and(|m| m.is_file() && m.len() > 0)
}

pub fn download_size_bytes() -> u64 {
    platform().map(|s| s.archive.bytes).unwrap_or(0)
}

/// 将固定版本、已校验的归档中需要的文件原子安装到专用目录。
fn install(archive: &Path, data_dir: &Path, spec: &RuntimeSpec) -> AppResult<()> {
    let dest = runtime_dir(data_dir, spec);
    let parent = dest.parent().expect("runtime directory has parent");
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".{}-{}", spec.target, uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging)?;
    let result = (|| -> AppResult<()> {
        let decoder = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut tar = tar::Archive::new(decoder);
        for entry in tar.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() { continue; }
            let path = entry.path()?.into_owned();
            let mut selected = None;
            for name in [spec.library, "libonnxruntime_providers_shared.so"] {
                if path == Path::new(spec.archive_root).join("lib").join(name) { selected = Some(name); }
            }
            for name in ["LICENSE", "ThirdPartyNotices.txt"] {
                if path == Path::new(spec.archive_root).join(name) { selected = Some(name); }
            }
            if let Some(name) = selected { entry.unpack(staging.join(name))?; }
        }
        if !std::fs::metadata(staging.join(spec.library)).is_ok_and(|m| m.is_file() && m.len() > 0) {
            return Err(AppError::new("model", "ONNX Runtime 归档缺少当前平台的动态库"));
        }
        std::fs::write(staging.join("verified.sha256"), spec.archive.sha256.unwrap_or_default())?;
        if dest.exists() { std::fs::remove_dir_all(&dest)?; }
        std::fs::rename(&staging, &dest)?;
        Ok(())
    })();
    if staging.exists() { let _ = std::fs::remove_dir_all(&staging); }
    result
}

pub async fn ensure_downloaded(http: &reqwest::Client, data_dir: &Path, cancel: &CancellationToken, on_event: DownloadEmitter<'_>) -> AppResult<()> {
    static INSTALL_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _guard = tokio::select! {
        guard = INSTALL_LOCK.get_or_init(|| tokio::sync::Mutex::new(())).lock() => guard,
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
    };
    if is_ready(data_dir) { return Ok(()); }
    let spec = platform()?;
    let archives = data_dir.join("models").join("onnxruntime").join("archives");
    download_checked(http, &spec.archive, &archives, cancel, on_event).await?;
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    let archive = archives.join(spec.archive.name);
    let dd = data_dir.to_path_buf();
    tokio::task::spawn_blocking(move || install(&archive, &dd, spec)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    // 解压短操作完成后再响应取消；已安装文件保留，但不能把取消请求报告为任务成功。
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    Ok(())
}

/// 必须在任何 Session/Tensor API 前调用。ORT 环境全进程共用，失败时返回可读错误。
pub fn initialize(data_dir: &Path) -> AppResult<()> {
    static INITIALIZED: Mutex<bool> = Mutex::new(false);
    // API表保存的是库内函数指针，动态库必须一直活到进程结束。
    static LIBRARY: OnceLock<libloading::Library> = OnceLock::new();
    let mut initialized = INITIALIZED.lock().map_err(|_| AppError::new("model", "ONNX Runtime 初始化锁异常"))?;
    if *initialized { return Ok(()); }
    if !is_ready(data_dir) { return Err(AppError::bad_request("画师分类运行库未下载，请先点击该模型的下载按钮")); }
    let spec = platform()?;
    let library = runtime_dir(data_dir, spec).join(spec.library);
    if LIBRARY.get().is_none() {
        // ort默认的dlopen失败会panic（release panic=abort）；先用可失败API验证，
        // 再设置API表，确保不会进入其panic式动态库加载路径。
        let loaded = unsafe { libloading::Library::new(&library) }.map_err(|e| AppError::new("model", format!("ONNX Runtime 动态库无法加载: {e}")))?;
        let api = unsafe {
            let get_base: libloading::Symbol<unsafe extern "system" fn() -> *const ort::sys::OrtApiBase> = loaded.get(b"OrtGetApiBase\0")
                .map_err(|e| AppError::new("model", format!("ONNX Runtime 缺少API入口: {e}")))?;
            let base = get_base();
            if base.is_null() { return Err(AppError::new("model", "ONNX Runtime API入口为空")); }
            let version = ((*base).GetVersionString)();
            if version.is_null() { return Err(AppError::new("model", "ONNX Runtime 缺少版本信息")); }
            let version = std::ffi::CStr::from_ptr(version).to_string_lossy();
            if version != "1.22.0" { return Err(AppError::new("model", format!("ONNX Runtime 版本错误，期望1.22.0，实际{version}"))); }
            let api = ((*base).GetApi)(ort::sys::ORT_API_VERSION);
            if api.is_null() { return Err(AppError::new("model", "ONNX Runtime 不支持API22")); }
            *api
        };
        LIBRARY.set(loaded).map_err(|_| AppError::new("model", "ONNX Runtime 重复加载"))?;
        if !ort::set_api(api) { return Err(AppError::new("model", "ONNX Runtime 已被其他组件初始化，请重启应用")); }
    }
    ort::init_from(library.to_string_lossy()).with_name("nai5-codex-lab-classifier").commit()
        .map_err(|e| AppError::new("model", format!("ONNX Runtime 初始化失败: {e}")))?;
    *initialized = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_requires_installed_library_and_matching_verification_marker() {
        let dir = tempfile::tempdir().unwrap();
        if let Ok(spec) = platform() {
            let runtime = runtime_dir(dir.path(), spec);
            std::fs::create_dir_all(&runtime).unwrap();
            std::fs::write(runtime.join(spec.library), b"incomplete").unwrap();
            assert!(!is_ready(dir.path()));
            std::fs::write(runtime.join("verified.sha256"), "wrong").unwrap();
            assert!(!is_ready(dir.path()));
            std::fs::write(runtime.join("verified.sha256"), spec.archive.sha256.unwrap()).unwrap();
            assert!(is_ready(dir.path()));
        }
    }

    /// 使用预先下载的官方归档，走与应用一致的校验下载/解压流程，不在测试里访问网络。
    #[tokio::test]
    async fn official_runtime_archive_install_smoke_opt_in() {
        let Ok(archive_dir) = std::env::var("CLASSIFIER_RUNTIME_ARCHIVE_DIR") else { return; };
        let spec = platform().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let archives = data_dir.path().join("models/onnxruntime/archives");
        std::fs::create_dir_all(&archives).unwrap();
        std::fs::copy(Path::new(&archive_dir).join(spec.archive.name), archives.join(spec.archive.name)).unwrap();
        ensure_downloaded(&reqwest::Client::new(), data_dir.path(), &CancellationToken::new(), &|_| {}).await.unwrap();
        assert!(is_ready(data_dir.path()));
        let path = runtime_dir(data_dir.path(), spec).join(spec.library);
        let loaded = unsafe { libloading::Library::new(&path) }.unwrap();
        let get_base: libloading::Symbol<unsafe extern "system" fn() -> *const ort::sys::OrtApiBase> = unsafe { loaded.get(b"OrtGetApiBase\0") }.unwrap();
        let version = unsafe { std::ffi::CStr::from_ptr(((*get_base()).GetVersionString)()) };
        assert_eq!(version.to_str().unwrap(), "1.22.0");
    }
}
