//! 模型下载（CONTRACT §12.2 `style_model_download`）：从 `https://huggingface.co/openai/clip-vit-base-patch32/resolve/main/<file>`
//! 流式下到 `<file>.part`，写完改名；`.part` 已存在则带 `Range: bytes=<len>-` 续传（上游回 200 就从头重写，回 416 就删掉重来一次）；
//! `DownloadEvent` 每 ≥1 MiB 或 ≥500 ms 发一次，文件落地发 `finished=true`，失败发 `error`；取消令牌生效时保留 `.part` 供下次续传。
//! 大文件没有整体超时（600 MB 慢网可能要很久），只对每个数据块设 60 s 读超时防假死。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::models::DownloadEvent;

use super::model::{weights_path, MODEL_FILES, WEIGHTS_FILE};

/// HF 仓库的 resolve 前缀（§12.2）。
pub const HF_BASE_URL: &str = "https://huggingface.co/openai/clip-vit-base-patch32/resolve/main";
/// 单个数据块的读超时。
pub const CHUNK_TIMEOUT_SECS: u64 = 60;
const PROGRESS_BYTES: u64 = 1 << 20;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

pub type DownloadEmitter<'a> = &'a (dyn Fn(DownloadEvent) + Send + Sync);

pub fn file_url(base_url: &str, file: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), file)
}

pub fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", dest.to_string_lossy()))
}

/// 固定文件的长度 / SHA256 验证。未验证文件留在 .download，最终文件只在验证通过后替换。
pub async fn download_checked(http: &reqwest::Client, spec: &super::catalog::ModelFile, dir: &Path, cancel: &CancellationToken, on_event: DownloadEmitter<'_>) -> AppResult<()> {
    let dest = dir.join(spec.name);
    let valid = |path: PathBuf| {
        let bytes = spec.bytes;
        let sha = spec.sha256;
        tokio::task::spawn_blocking(move || -> bool {
            let Ok(meta) = std::fs::metadata(&path) else { return false };
            if !meta.is_file() || meta.len() == 0 || (bytes > 0 && meta.len() != bytes) { return false; }
            sha.map(|expected| crate::sync::images::sha256_file(&path).map(|actual| actual == expected).unwrap_or(false)).unwrap_or(true)
        })
    };
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    if valid(dest.clone()).await.map_err(|e| AppError::new("internal", e.to_string()))? {
        if cancel.is_cancelled() { return Err(AppError::cancelled()); }
        let n = std::fs::metadata(&dest)?.len();
        on_event(DownloadEvent { file: spec.name.into(), done_bytes: n, total_bytes: n, finished: true, error: None });
        return Ok(());
    }
    let staging = dir.join(format!("{}.download", spec.name));
    let relay = |mut ev: DownloadEvent| { ev.finished = false; on_event(ev); };
    // v0.3.x 的同名 .part 也继续使用；最后仍以固定版本的哈希校验完整文件。
    let legacy_part = part_path(&dest);
    let checked_part = part_path(&staging);
    if !checked_part.exists() && legacy_part.is_file() { tokio::fs::rename(&legacy_part, &checked_part).await?; }
    if !valid(staging.clone()).await.map_err(|e| AppError::new("internal", e.to_string()))? {
        download_file(http, spec.url, &staging, spec.name, cancel, &relay).await?;
    }
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    if !valid(staging.clone()).await.map_err(|e| AppError::new("internal", e.to_string()))? {
        let _ = tokio::fs::remove_file(&staging).await;
        let error = format!("{} 长度或 SHA256 校验失败，请重新下载", spec.name);
        on_event(DownloadEvent { file: spec.name.into(), done_bytes: 0, total_bytes: spec.bytes, finished: false, error: Some(error.clone()) });
        return Err(AppError::new("model", error));
    }
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    tokio::fs::rename(&staging, &dest).await?;
    let n = tokio::fs::metadata(&dest).await?.len();
    on_event(DownloadEvent { file: spec.name.into(), done_bytes: n, total_bytes: n, finished: true, error: None });
    Ok(())
}

/// `Content-Range: bytes a-b/total` → total。
fn content_range_total(v: &str) -> Option<u64> {
    v.trim().strip_prefix("bytes")?.trim().rsplit('/').next()?.trim().parse().ok()
}

/// 下载整个模型目录：已落地的文件（非空）直接发一条 `finished`，其余逐个下载；任一文件失败即返回 Err（事件里已带 error）。
pub async fn download_model(http: &reqwest::Client, base_url: &str, dir: &Path, cancel: &CancellationToken, on_event: DownloadEmitter<'_>) -> AppResult<()> {
    tokio::fs::create_dir_all(dir).await?;
    for file in MODEL_FILES {
        // 兼容已手动放入的 safetensors，不再下载一份同模型权重。
        let dest = if file == WEIGHTS_FILE { weights_path(dir).unwrap_or_else(|| dir.join(file)) } else { dir.join(file) };
        if let Ok(meta) = tokio::fs::metadata(&dest).await {
            if meta.is_file() && meta.len() > 0 {
                on_event(DownloadEvent { file: dest.file_name().unwrap().to_string_lossy().into_owned(), done_bytes: meta.len(), total_bytes: meta.len(), finished: true, error: None });
                continue;
            }
        }
        download_file(http, &file_url(base_url, file), &dest, file, cancel, on_event).await?;
    }
    Ok(())
}

/// 单文件：`.part` 续传 → 流式写 → rename。失败 / 取消都先发一条带 `error` 的事件再返回 Err。
pub async fn download_file(http: &reqwest::Client, url: &str, dest: &Path, label: &str, cancel: &CancellationToken, on_event: DownloadEmitter<'_>) -> AppResult<()> {
    match download_inner(http, url, dest, label, cancel, on_event, true).await {
        Ok(()) => Ok(()),
        Err(e) => {
            on_event(DownloadEvent { file: label.to_string(), done_bytes: 0, total_bytes: 0, finished: false, error: Some(e.message.clone()) });
            Err(e)
        }
    }
}

async fn download_inner(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
    cancel: &CancellationToken,
    on_event: DownloadEmitter<'_>,
    allow_retry_416: bool,
) -> AppResult<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = part_path(dest);
    let mut offset: u64 = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    let mut req = http.get(url);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let resp = tokio::select! {
        r = req.send() => r.map_err(|e| AppError::network(format!("{url}: {e}")))?,
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
    };
    let status = resp.status();
    let (append, total) = match status.as_u16() {
        206 => {
            let total = resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(content_range_total).unwrap_or(0);
            (true, total)
        }
        200 => {
            // 上游不认 Range：从头写
            offset = 0;
            (false, resp.content_length().unwrap_or(0))
        }
        416 if allow_retry_416 => {
            // .part 比上游还长（上游换了文件）：丢掉重来一次
            let _ = tokio::fs::remove_file(&part).await;
            return Box::pin(download_inner(http, url, dest, label, cancel, on_event, false)).await;
        }
        _ => return Err(AppError::upstream(format!("{url}: HTTP {status}"))),
    };
    let mut file = tokio::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&part).await?;
    let mut done = offset;
    on_event(DownloadEvent { file: label.to_string(), done_bytes: done, total_bytes: total, finished: false, error: None });
    let mut stream = resp.bytes_stream();
    let mut last_emit = Instant::now();
    let mut last_bytes = done;
    loop {
        let next = tokio::select! {
            n = tokio::time::timeout(Duration::from_secs(CHUNK_TIMEOUT_SECS), stream.next()) => n,
            _ = cancel.cancelled() => {
                let _ = file.flush().await;
                return Err(AppError::cancelled());
            }
        };
        let chunk = match next {
            Err(_) => return Err(AppError::network(format!("{url}: {CHUNK_TIMEOUT_SECS} 秒没有收到数据"))),
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(AppError::network(format!("{url}: {e}"))),
            Ok(Some(Ok(c))) => c,
        };
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if done - last_bytes >= PROGRESS_BYTES || last_emit.elapsed() >= PROGRESS_INTERVAL {
            on_event(DownloadEvent { file: label.to_string(), done_bytes: done, total_bytes: total, finished: false, error: None });
            last_emit = Instant::now();
            last_bytes = done;
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    if total > 0 && done != total {
        return Err(AppError::upstream(format!("{url}: 收到 {done} 字节，期望 {total}")));
    }
    if done == 0 {
        return Err(AppError::upstream(format!("{url}: 空响应")));
    }
    tokio::fs::rename(&part, dest).await?;
    on_event(DownloadEvent { file: label.to_string(), done_bytes: done, total_bytes: if total > 0 { total } else { done }, finished: true, error: None });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    use tokio::net::TcpListener;

    /// 本地 HTTP 桩：一个连接一个请求，`ranges=true` 时按 `Range: bytes=N-` 回 206 + Content-Range，否则永远 200 全量。
    /// 仅提供官方文件名与通用 `/x`；不存在的文件（含旧 safetensors URL）回 404。记录收到的 Range 头。
    async fn serve(body: Vec<u8>, ranges: bool, seen: Arc<Mutex<Vec<Option<String>>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let body = body.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(sock);
                    let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let mut range: Option<String> = None;
                    loop {
                        let mut h = String::new();
                        reader.read_line(&mut h).await.unwrap();
                        if h == "\r\n" || h.is_empty() {
                            break;
                        }
                        if let Some(v) = h.to_ascii_lowercase().strip_prefix("range:") {
                            range = Some(v.trim().to_string());
                        }
                    }
                    seen.lock().unwrap().push(range.clone());
                    let mut sock = reader.into_inner();
                    let resp: Vec<u8> = if !matches!(path.as_str(), "/config.json" | "/pytorch_model.bin" | "/x") {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                    } else {
                        match (ranges, range.as_deref().and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.trim_end_matches('-').parse::<u64>().ok())) {
                            (true, Some(start)) if start as usize <= body.len() => {
                                let slice = &body[start as usize..];
                                let mut r = format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                                    slice.len(),
                                    start,
                                    body.len().saturating_sub(1),
                                    body.len()
                                )
                                .into_bytes();
                                r.extend_from_slice(slice);
                                r
                            }
                            (true, Some(_)) => b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                            _ => {
                                let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                                r.extend_from_slice(&body);
                                r
                            }
                        }
                    };
                    let _ = sock.write_all(&resp).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn events() -> (Arc<Mutex<Vec<DownloadEvent>>>, impl Fn(DownloadEvent) + Send + Sync) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        (log, move |e| l2.lock().unwrap().push(e))
    }

    #[tokio::test]
    async fn checked_download_rejects_corruption_and_only_publishes_verified_files() {
        use super::super::catalog::ModelFile;
        use sha2::{Digest, Sha256};
        let body = b"verified model bytes".to_vec();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = serve(body.clone(), true, seen.clone()).await;
        let url: &'static str = Box::leak(format!("{base}/x").into_boxed_str());
        let sha: &'static str = Box::leak(hex::encode(Sha256::digest(&body)).into_boxed_str());
        let good = ModelFile { name: "weight.bin", url, bytes: body.len() as u64, sha256: Some(sha) };
        let bad = ModelFile { name: "weight.bin", url, bytes: body.len() as u64, sha256: Some("wrong hash") };
        let dir = tempfile::tempdir().unwrap();
        let (events, emit) = events();
        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();
        assert!(download_checked(&http, &bad, dir.path(), &cancel, &emit).await.is_err());
        assert!(!dir.path().join("weight.bin").exists());
        assert!(events.lock().unwrap().iter().all(|e| !e.finished));
        // 取消留下的部分数据可以续传，校验通过后才出现最终文件。
        std::fs::write(dir.path().join("weight.bin.download.part"), &body[..5]).unwrap();
        download_checked(&http, &good, dir.path(), &cancel, &emit).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("weight.bin")).unwrap(), body);
        assert!(events.lock().unwrap().last().unwrap().finished);
        let requests = seen.lock().unwrap().len();
        download_checked(&http, &good, dir.path(), &cancel, &emit).await.unwrap();
        assert_eq!(seen.lock().unwrap().len(), requests, "完整且校验正确时不重复下载");
    }

    /// 整包下载：`.part` → rename、字节一致、事件首尾（起始 done=0 / 结束 finished 且 done=total）、无 `.part` 残留；
    /// 第二次调用已落地的文件直接发 finished 不再请求；404 → upstream 错误 + 带 error 的事件。
    #[tokio::test]
    async fn downloads_via_part_then_rename() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = serve(body.clone(), true, seen.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();
        let (log, emit) = events();
        download_file(&http, &format!("{base}/pytorch_model.bin"), &dir.path().join("pytorch_model.bin"), "pytorch_model.bin", &cancel, &emit).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("pytorch_model.bin")).unwrap(), body);
        assert!(!part_path(&dir.path().join("pytorch_model.bin")).exists(), "不留 .part");
        let evs = log.lock().unwrap().clone();
        assert!(evs.len() >= 2);
        assert_eq!((evs[0].done_bytes, evs[0].total_bytes, evs[0].finished), (0, body.len() as u64, false));
        let last = evs.last().unwrap();
        assert_eq!((last.file.as_str(), last.done_bytes, last.total_bytes, last.finished, last.error.clone()), ("pytorch_model.bin", body.len() as u64, body.len() as u64, true, None));
        assert!(evs.iter().all(|e| e.error.is_none()));
        assert_eq!(seen.lock().unwrap().as_slice(), &[None], "首次下载不带 Range");

        // 404
        let (log2, emit2) = events();
        let err = download_file(&http, &format!("{base}/404"), &dir.path().join("config.json"), "config.json", &cancel, &emit2).await.unwrap_err();
        assert_eq!(err.code, "upstream");
        let evs = log2.lock().unwrap().clone();
        assert_eq!(evs.len(), 1);
        assert!(evs[0].error.as_deref().unwrap().contains("404") && !evs[0].finished);
        assert!(!dir.path().join("config.json").exists());
    }

    /// 续传：预先留一半的 `.part` → 请求带 `Range: bytes=N-`、上游 206 → 拼出完整文件；上游不认 Range（200）时从头重写也正确；
    /// `.part` 比上游长 → 416 → 删掉重来。`download_model` 对已落地文件直接发 finished，对缺的文件走下载。
    #[tokio::test]
    async fn resumes_from_part_with_range() {
        let body: Vec<u8> = (0..200_000u32).map(|i| (i.wrapping_mul(7) % 253) as u8).collect();
        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();

        // 206 续传
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = serve(body.clone(), true, seen.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pytorch_model.bin");
        std::fs::write(part_path(&dest), &body[..120_000]).unwrap();
        let (log, emit) = events();
        download_file(&http, &format!("{base}/x"), &dest, "pytorch_model.bin", &cancel, &emit).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(seen.lock().unwrap().as_slice(), &[Some("bytes=120000-".to_string())]);
        let evs = log.lock().unwrap().clone();
        assert_eq!((evs[0].done_bytes, evs[0].total_bytes), (120_000, body.len() as u64), "起始事件从续传点算");
        assert!(evs.last().unwrap().finished);

        // 上游不认 Range → 200 从头写
        let seen2 = Arc::new(Mutex::new(Vec::new()));
        let base2 = serve(body.clone(), false, seen2.clone()).await;
        let dir2 = tempfile::tempdir().unwrap();
        let dest2 = dir2.path().join("pytorch_model.bin");
        std::fs::write(part_path(&dest2), b"garbage garbage").unwrap();
        let (log2, emit2) = events();
        download_file(&http, &format!("{base2}/x"), &dest2, "pytorch_model.bin", &cancel, &emit2).await.unwrap();
        assert_eq!(std::fs::read(&dest2).unwrap(), body, "200 时丢掉旧 .part 从头写");
        assert_eq!(seen2.lock().unwrap().as_slice(), &[Some("bytes=15-".to_string())]);
        assert_eq!(log2.lock().unwrap()[0].done_bytes, 0);

        // .part 比上游长 → 416 → 删掉重来
        let seen3 = Arc::new(Mutex::new(Vec::new()));
        let base3 = serve(body.clone(), true, seen3.clone()).await;
        let dir3 = tempfile::tempdir().unwrap();
        let dest3 = dir3.path().join("pytorch_model.bin");
        std::fs::write(part_path(&dest3), vec![1u8; body.len() + 10]).unwrap();
        let (_log3, emit3) = events();
        download_file(&http, &format!("{base3}/x"), &dest3, "pytorch_model.bin", &cancel, &emit3).await.unwrap();
        assert_eq!(std::fs::read(&dest3).unwrap(), body);
        assert_eq!(seen3.lock().unwrap().len(), 2, "416 后重来一次");
        assert_eq!(seen3.lock().unwrap()[1], None);

        // download_model：config.json 已在场 → 直接 finished；pytorch_model.bin 走下载
        let seen4 = Arc::new(Mutex::new(Vec::new()));
        let base4 = serve(body.clone(), true, seen4.clone()).await;
        let dir4 = tempfile::tempdir().unwrap();
        std::fs::write(dir4.path().join("config.json"), b"{}").unwrap();
        let (log4, emit4) = events();
        download_model(&http, &base4, dir4.path(), &cancel, &emit4).await.unwrap();
        let evs = log4.lock().unwrap().clone();
        assert_eq!((evs[0].file.as_str(), evs[0].finished, evs[0].done_bytes), ("config.json", true, 2));
        assert!(evs.iter().filter(|e| e.file == "pytorch_model.bin").last().unwrap().finished);
        assert_eq!(std::fs::read(dir4.path().join("pytorch_model.bin")).unwrap(), body);
        assert_eq!(seen4.lock().unwrap().len(), 1, "只请求了缺的那个文件");
        assert_eq!(file_url(HF_BASE_URL, "config.json"), "https://huggingface.co/openai/clip-vit-base-patch32/resolve/main/config.json");
        assert_eq!(file_url("http://x/", "a"), "http://x/a");
        assert_eq!(content_range_total("bytes 10-99/100"), Some(100));
        assert_eq!(content_range_total("bytes */100"), Some(100));
        assert_eq!(content_range_total("x"), None);
    }

    /// 已有 safetensors 时只补缺失配置；零字节配置必须重新下载，不请求官方不存在的 safetensors URL。
    #[tokio::test]
    async fn keeps_existing_safetensors_and_repairs_empty_config() {
        let body = b"{}".to_vec();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = serve(body.clone(), true, seen.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(super::super::model::SAFETENSORS_FILE);
        std::fs::write(&legacy, b"existing-weights").unwrap();
        std::fs::write(dir.path().join("config.json"), b"").unwrap();
        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();
        let (log, emit) = events();
        download_model(&http, &base, dir.path(), &cancel, &emit).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("config.json")).unwrap(), body);
        assert_eq!(std::fs::read(legacy).unwrap(), b"existing-weights");
        assert!(!dir.path().join(WEIGHTS_FILE).exists());
        assert_eq!(seen.lock().unwrap().len(), 1, "只修复配置，不重新下载已有权重");
        assert!(log.lock().unwrap().iter().any(|e| e.file == "model.safetensors" && e.finished));
        download_model(&http, &base, dir.path(), &cancel, &emit).await.unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1, "再次下载不发网络请求");
    }

    /// 取消：令牌取消后返回 `cancelled`、事件带 error、`.part` 保留（下次续传）、目标文件不存在。
    #[tokio::test]
    async fn cancel_keeps_part_for_resume() {
        let body: Vec<u8> = vec![7u8; 4_000_000];
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = serve(body, true, seen).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pytorch_model.bin");
        let http = reqwest::Client::new();
        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        let emit = move |e: DownloadEvent| {
            // 收到第一条进度事件就取消
            if !e.finished && e.error.is_none() {
                cancel2.cancel();
            }
        };
        let err = download_file(&http, &format!("{base}/x"), &dest, "pytorch_model.bin", &cancel, &emit).await.unwrap_err();
        assert_eq!(err.code, "cancelled");
        assert!(!dest.exists());
        assert!(part_path(&dest).exists(), "取消保留 .part");
        let mut f = tokio::fs::File::open(part_path(&dest)).await.unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).await.unwrap();
        assert!(buf.len() < 4_000_000 || buf.iter().all(|b| *b == 7));
    }
}
