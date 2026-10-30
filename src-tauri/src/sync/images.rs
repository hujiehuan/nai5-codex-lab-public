//! 图片队列（设计 §1.3 第 5 步）：队列 = `ordinal=0` 且未落地或 `asset_rev` 不等于条目当前 assetRev 的展示图，
//! 加上用户点过「下载全部/原图」的条目。写 `.part`，续传用 `Range` + `If-Range: <首次 ETag>`（ETag 存在 `.part.etag` 旁车文件），
//! 完成后按 ETag（MD5）校验，rename 成功后才写 `entry_image.asset_rev/etag/local_path/bytes/downloaded_at/sha256`，
//! 同时生成 320 px JPEG 缩略图到 `thumbs/<codex_id>/<image_file>.jpg`（CONTRACT §0）。
//! 单文件超时 60 s，失败重试 3 次（2/4/8 s）。下载路径统一返回 `AppError`（code = network / upstream / io /
//! cancelled / checksum / internal），失败清单 `SyncFailure` 带 `code`。开跑前先占 `AppState.inflight`
//! 在飞集合，主同步与按需下载不会同时写同一个 `.part`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::db::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::SyncFailure;
use crate::state::{InflightSet, Throttle};

use super::upstream::Media;
use super::SyncCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageKind {
    Display,
    Original,
}

/// 在飞集合的键：同一张图（条目、序号、展示图/原图）同一时刻只允许一个任务在下（v0.1.1 #4）。
/// 展示图与原图是两个文件，所以 kind 也进键，否则同一次同步里的两个任务会互相跳过。
pub type InflightKey = (String, i64, ImageKind);

/// 占住在飞集合里的一个键，Drop 时释放（正常结束、失败、取消都会走到）。
struct InflightGuard<'a> {
    set: &'a InflightSet,
    key: InflightKey,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.set.lock().remove(&self.key);
    }
}

fn try_occupy<'a>(set: &'a InflightSet, job: &ImageJob) -> Option<InflightGuard<'a>> {
    let key: InflightKey = (job.entry_id.clone(), job.ordinal, job.kind);
    if set.lock().insert(key.clone()) {
        Some(InflightGuard { set, key })
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct ImageJob {
    pub entry_id: String,
    pub codex_id: String,
    pub ordinal: i64,
    pub kind: ImageKind,
    pub url: String,
    pub dest: PathBuf,
    pub thumb: Option<PathBuf>,
    pub asset_rev: Option<String>,
}

#[derive(Debug, Default)]
pub struct QueueOutcome {
    pub ok: u64,
    pub failed: u64,
    /// 另一任务正在下同一张图而跳过的数量（不计入 ok/failed）。
    pub skipped: u64,
    pub failures: Vec<SyncFailure>,
}

/// 用户在详情里点过「下载全部展示图 / 下载原图」的条目，存 `app_state('download_all')`。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DownloadAllFlags {
    #[serde(default)]
    pub display_all: bool,
    #[serde(default)]
    pub originals: bool,
}

pub type DownloadAll = HashMap<String, DownloadAllFlags>;

pub fn load_download_all(conn: &Connection) -> DownloadAll {
    crate::db::app_state_get(conn, "download_all")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_download_all(conn: &Connection, all: &DownloadAll) -> AppResult<()> {
    crate::db::app_state_set(conn, "download_all", &serde_json::to_string(all)?)
}

fn local_display_name(codex_id: &str, image_file: &str) -> String {
    if codex_id == super::nax::BOOK_ID {
        use sha2::{Digest, Sha256};
        // CDN percent-encoded names can exceed filesystem filename limits.
        format!("{}.webp", hex::encode(Sha256::digest(image_file.as_bytes())))
    } else {
        image_file.to_string()
    }
}

pub fn display_dest(data_dir: &Path, codex_id: &str, image_file: &str) -> PathBuf {
    data_dir.join("images").join(codex_id).join(local_display_name(codex_id, image_file))
}
pub fn original_dest(data_dir: &Path, codex_id: &str, original_file: &str) -> PathBuf {
    data_dir.join("originals").join(codex_id).join(original_file)
}
pub fn thumb_dest(data_dir: &Path, codex_id: &str, image_file: &str) -> PathBuf {
    data_dir.join("thumbs").join(codex_id).join(format!("{}.jpg", local_display_name(codex_id, image_file)))
}

struct ImageRow {
    entry_id: String,
    codex_id: String,
    ordinal: i64,
    image_file: String,
    original_file: Option<String>,
    row_asset_rev: Option<String>,
    entry_asset_rev: Option<String>,
    local_path: Option<String>,
    original_local_path: Option<String>,
}

fn query_rows(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<ImageRow>> {
    let sql = format!(
        "SELECT i.entry_id, e.codex_id, i.ordinal, i.image_file, i.original_file, i.asset_rev, e.asset_rev, i.local_path, i.original_local_path
         FROM entry_image i JOIN codex_entry e ON e.entry_id = i.entry_id
         WHERE e.removed_at IS NULL AND {where_sql}
         ORDER BY e.codex_id, e.book_order, i.ordinal"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(args, |r| {
            Ok(ImageRow {
                entry_id: r.get(0)?,
                codex_id: r.get(1)?,
                ordinal: r.get(2)?,
                image_file: r.get(3)?,
                original_file: r.get(4)?,
                row_asset_rev: r.get(5)?,
                entry_asset_rev: r.get(6)?,
                local_path: r.get(7)?,
                original_local_path: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn needs_display(row: &ImageRow) -> bool {
    match &row.local_path {
        None => true,
        Some(p) => row.row_asset_rev != row.entry_asset_rev || !Path::new(p).exists(),
    }
}

fn needs_original(row: &ImageRow) -> bool {
    row.original_file.is_some()
        && match &row.original_local_path {
            None => true,
            Some(p) => !Path::new(p).exists(),
        }
}

fn job_from_row(data_dir: &Path, media: &Media, row: &ImageRow, kind: ImageKind) -> Option<ImageJob> {
    match kind {
        ImageKind::Display => Some(ImageJob {
            entry_id: row.entry_id.clone(),
            codex_id: row.codex_id.clone(),
            ordinal: row.ordinal,
            kind,
            url: media.image_url(&row.codex_id, &row.image_file),
            dest: display_dest(data_dir, &row.codex_id, &row.image_file),
            thumb: Some(thumb_dest(data_dir, &row.codex_id, &row.image_file)),
            asset_rev: row.entry_asset_rev.clone(),
        }),
        ImageKind::Original => {
            let orig = row.original_file.as_ref()?;
            Some(ImageJob {
                entry_id: row.entry_id.clone(),
                codex_id: row.codex_id.clone(),
                ordinal: row.ordinal,
                kind,
                url: media.original_url(&row.codex_id, orig),
                dest: original_dest(data_dir, &row.codex_id, orig),
                thumb: None,
                asset_rev: row.entry_asset_rev.clone(),
            })
        }
    }
}

/// 同步用队列：白名单书的主图补缺 + 「下载全部」条目的其余图。
pub fn build_sync_queue(conn: &Connection, data_dir: &Path, media: &Media, books: &[&str]) -> AppResult<Vec<ImageJob>> {
    let all = load_download_all(conn);
    let mut jobs = Vec::new();
    for book in books {
        let rows = query_rows(conn, "e.codex_id = ?1", &[book])?;
        for row in rows {
            let flags = all.get(&row.entry_id);
            let want_display = row.ordinal == 0 || flags.map(|f| f.display_all).unwrap_or(false);
            if want_display && needs_display(&row) {
                if let Some(j) = job_from_row(data_dir, media, &row, ImageKind::Display) {
                    jobs.push(j);
                }
            }
            if flags.map(|f| f.originals).unwrap_or(false) && needs_original(&row) {
                if let Some(j) = job_from_row(data_dir, media, &row, ImageKind::Original) {
                    jobs.push(j);
                }
            }
        }
    }
    Ok(jobs)
}

/// 按需下载队列（`download_images`）。
pub fn build_entry_queue(conn: &Connection, data_dir: &Path, media: &Media, entry_id: &str, kind: ImageKind) -> AppResult<Vec<ImageJob>> {
    let rows = query_rows(conn, "e.entry_id = ?1", &[&entry_id])?;
    let mut jobs = Vec::new();
    for row in rows {
        let need = match kind {
            ImageKind::Display => needs_display(&row),
            ImageKind::Original => needs_original(&row),
        };
        if need {
            if let Some(j) = job_from_row(data_dir, media, &row, kind) {
                jobs.push(j);
            }
        }
    }
    Ok(jobs)
}

/// 跑完整个队列：并发 `ctx.concurrency`，每张图落地后回调 `on_progress(done, failed)`。
/// 开跑前先占在飞集合，占不到（另一任务正在下同一张）→ 跳过并记 warn，计入 `skipped`。
pub async fn run_queue(ctx: &SyncCtx, jobs: Vec<ImageJob>, on_progress: impl Fn(u64, u64) + Send + Sync) -> QueueOutcome {
    let done = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicU64::new(0));
    let skipped = Arc::new(AtomicU64::new(0));
    let failures: Arc<Mutex<Vec<SyncFailure>>> = Arc::new(Mutex::new(Vec::new()));
    let concurrency = ctx.concurrency.clamp(1, 4);
    let on_progress = &on_progress;
    futures_util::stream::iter(jobs)
        .map(|job| {
            let done = done.clone();
            let failed = failed.clone();
            let skipped = skipped.clone();
            let failures = failures.clone();
            async move {
                if ctx.cancel.is_cancelled() {
                    return;
                }
                let Some(_guard) = try_occupy(&ctx.inflight, &job) else {
                    skipped.fetch_add(1, Ordering::SeqCst);
                    tracing::warn!(entry_id = %job.entry_id, ordinal = job.ordinal, kind = ?job.kind, url = %job.url, "另一任务正在下载同一张图，跳过");
                    return;
                };
                match process_job(ctx, &job).await {
                    Ok(()) => {
                        done.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(e) => {
                        if ctx.cancel.is_cancelled() || e.code == "cancelled" {
                            return;
                        }
                        failed.fetch_add(1, Ordering::SeqCst);
                        failures.lock().push(SyncFailure { url: job.url.clone(), error: e.message.clone(), code: e.code.clone() });
                        let _ = ctx.db.with(|c| {
                            c.execute(
                                "UPDATE entry_image SET error = ?3 WHERE entry_id = ?1 AND ordinal = ?2",
                                params![job.entry_id, job.ordinal, e.to_string()],
                            )?;
                            Ok(())
                        });
                        tracing::warn!(url = %job.url, code = %e.code, "image download failed: {}", e.message);
                    }
                }
                on_progress(done.load(Ordering::SeqCst), failed.load(Ordering::SeqCst));
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<()>>()
        .await;
    let collected = std::mem::take(&mut *failures.lock());
    QueueOutcome {
        ok: done.load(Ordering::SeqCst),
        failed: failed.load(Ordering::SeqCst),
        skipped: skipped.load(Ordering::SeqCst),
        failures: collected,
    }
}

async fn process_job(ctx: &SyncCtx, job: &ImageJob) -> AppResult<()> {
    let (bytes, etag) = download_file(&ctx.http, &ctx.throttle, &ctx.cancel, &job.url, &job.dest).await?;
    let mut thumb_path: Option<String> = None;
    if let Some(thumb) = &job.thumb {
        let src = job.dest.clone();
        let dst = thumb.clone();
        match tokio::task::spawn_blocking(move || make_thumbnail(&src, &dst)).await {
            Ok(Ok(())) => thumb_path = Some(thumb.to_string_lossy().to_string()),
            Ok(Err(e)) => tracing::warn!(dest = %job.dest.display(), "thumbnail failed: {e}"),
            Err(e) => tracing::warn!("thumbnail task panicked: {e}"),
        }
    }
    // v0.1.4：展示图落地后顺手算 sha256（完整池 §8.1 `images[].sha256`）；算不出不算下载失败，导出时会再补
    let sha256 = match job.kind {
        ImageKind::Display => {
            let p = job.dest.clone();
            match tokio::task::spawn_blocking(move || sha256_file(&p)).await {
                Ok(Ok(h)) => Some(h),
                Ok(Err(e)) => {
                    tracing::warn!(dest = %job.dest.display(), "sha256 failed: {e}");
                    None
                }
                Err(e) => {
                    tracing::warn!("sha256 task panicked: {e}");
                    None
                }
            }
        }
        ImageKind::Original => None,
    };
    let now = now_iso();
    let dest = job.dest.to_string_lossy().to_string();
    ctx.db.with(|c| {
        match job.kind {
            ImageKind::Display => c.execute(
                "UPDATE entry_image SET asset_rev = ?3, etag = ?4, local_path = ?5, thumb_path = COALESCE(?6, thumb_path), bytes = ?7, downloaded_at = ?8, sha256 = ?9, error = NULL
                 WHERE entry_id = ?1 AND ordinal = ?2",
                params![job.entry_id, job.ordinal, job.asset_rev, etag, dest, thumb_path, bytes as i64, now, sha256],
            )?,
            ImageKind::Original => c.execute(
                "UPDATE entry_image SET original_local_path = ?3, downloaded_at = ?4, error = NULL WHERE entry_id = ?1 AND ordinal = ?2",
                params![job.entry_id, job.ordinal, dest, now],
            )?,
        };
        Ok(())
    })?;
    Ok(())
}

fn md5_from_etag(etag: &str) -> Option<String> {
    let t = etag.trim().trim_start_matches("W/").trim_matches('"');
    if t.len() == 32 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(t.to_ascii_lowercase())
    } else {
        None
    }
}

fn md5_file(path: &Path) -> std::io::Result<String> {
    use md5::{Digest, Md5};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// 流式算文件 sha256（小写 hex）；`entry_image.sha256` 与完整池导出补算共用。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// 下载到 `dest`（经 `.part` 续传 + MD5 校验 + rename）。返回 (字节数, ETag)。
/// 错误 code：network（连接/读流）、upstream（非 2xx）、io（本地文件）、checksum（MD5 不符）、cancelled。
pub async fn download_file(
    http: &reqwest::Client,
    throttle: &Throttle,
    cancel: &CancellationToken,
    url: &str,
    dest: &Path,
) -> AppResult<(u64, Option<String>)> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = PathBuf::from(format!("{}.part", dest.to_string_lossy()));
    let side = PathBuf::from(format!("{}.part.etag", dest.to_string_lossy()));
    let mut last_err = AppError::new("internal", "unknown");
    for attempt in 0..4u32 {
        if attempt > 0 {
            let backoff = Duration::from_secs(2u64.pow(attempt));
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = cancel.cancelled() => return Err(AppError::cancelled()),
            }
        }
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        match download_attempt(http, throttle, cancel, url, dest, &part, &side).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if e.code == "cancelled" {
                    return Err(e);
                }
                last_err = e;
            }
        }
    }
    Err(last_err)
}

async fn download_attempt(
    http: &reqwest::Client,
    throttle: &Throttle,
    cancel: &CancellationToken,
    url: &str,
    dest: &Path,
    part: &Path,
    side: &Path,
) -> AppResult<(u64, Option<String>)> {
    let existing = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
    let side_etag = tokio::fs::read_to_string(side).await.ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let resume = existing > 0 && side_etag.is_some();

    let _permit = throttle.acquire().await;
    let mut req = http.get(url).timeout(Duration::from_secs(60));
    if resume {
        req = req
            .header(reqwest::header::RANGE, format!("bytes={existing}-"))
            .header(reqwest::header::IF_RANGE, side_etag.clone().unwrap_or_default());
    }
    let resp = tokio::select! {
        r = req.send() => r.map_err(|e| AppError::network(format!("{url}: {e}")))?,
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
    };
    let status = resp.status();
    let resp_etag = resp.headers().get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let mut file = match status.as_u16() {
        206 if resume => tokio::fs::OpenOptions::new().append(true).open(part).await?,
        200 => {
            if let Some(t) = &resp_etag {
                let _ = tokio::fs::write(side, t).await;
            } else {
                let _ = tokio::fs::remove_file(side).await;
            }
            tokio::fs::File::create(part).await?
        }
        416 => {
            let _ = tokio::fs::remove_file(part).await;
            let _ = tokio::fs::remove_file(side).await;
            return Err(AppError::upstream(format!("{url}: 416 range not satisfiable, restarting")));
        }
        code => return Err(AppError::upstream(format!("{url}: HTTP {code}"))),
    };
    let mut stream = resp.bytes_stream();
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = cancel.cancelled() => { let _ = file.flush().await; return Err(AppError::cancelled()); }
        };
        match chunk {
            Some(Ok(bytes)) => file.write_all(&bytes).await?,
            Some(Err(e)) => {
                let _ = file.flush().await;
                return Err(AppError::network(format!("{url}: {e}")));
            }
            None => break,
        }
    }
    file.flush().await?;
    drop(file);

    let etag = if status.as_u16() == 200 { resp_etag } else { side_etag.clone().or(resp_etag) };
    if let Some(expected) = etag.as_deref().and_then(md5_from_etag) {
        let p = part.to_path_buf();
        let actual = tokio::task::spawn_blocking(move || md5_file(&p)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
        if actual != expected {
            let _ = tokio::fs::remove_file(part).await;
            let _ = tokio::fs::remove_file(side).await;
            return Err(AppError::checksum(format!("{url}: md5 mismatch: etag {expected} != file {actual}")));
        }
    }
    let size = tokio::fs::metadata(part).await?.len();
    tokio::fs::rename(part, dest).await?;
    let _ = tokio::fs::remove_file(side).await;
    Ok((size, etag.as_deref().and_then(md5_from_etag).or(etag)))
}

/// 320 px 最长边 JPEG（质量 80），先写 `.tmp` 再 rename。
pub fn make_thumbnail(src: &Path, dst: &Path) -> AppResult<()> {
    use image::codecs::jpeg::JpegEncoder;
    let img = image::open(src)?;
    // `thumbnail` 会把小图放大到边界，所以先判断：最长边 ≤ 320 只转码不缩放
    let thumb = if img.width().max(img.height()) <= 320 { img.to_rgb8() } else { img.thumbnail(320, 320).to_rgb8() };
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", dst.to_string_lossy()));
    {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        let mut enc = JpegEncoder::new_with_quality(&mut out, 80);
        enc.encode_image(&thumb).map_err(|e| AppError::io(format!("jpeg encode: {e}")))?;
        use std::io::Write;
        out.flush()?;
    }
    std::fs::rename(&tmp, dst)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nax_urls_keep_remote_encoding_and_local_names_stay_short() {
        let filename = format!("{}.webp", "%25E7%2594%25BB".repeat(90));
        let dir = Path::new("/tmp/nax-test");
        let display = display_dest(dir, super::super::nax::BOOK_ID, &filename);
        let thumb = thumb_dest(dir, super::super::nax::BOOK_ID, &filename);
        assert_eq!(display.file_name().unwrap().len(), 69);
        assert_eq!(thumb.file_name().unwrap().len(), 73);
        assert_eq!(super::super::nax::media().image_url(super::super::nax::BOOK_ID, &filename), format!("https://cdn.zele.st/data/NAX/Images/danbooru-artist-tags-2-v5/{filename}"));
        assert_eq!(display_dest(dir, "artist_nai5_personal", "old.png"), dir.join("images/artist_nai5_personal/old.png"));
    }

    #[test]
    fn thumbnail_is_320_max_side_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.png");
        let img = image::RgbaImage::from_fn(800, 500, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
        img.save(&src).unwrap();
        let dst = dir.path().join("thumbs").join("book").join("src.png.jpg");
        make_thumbnail(&src, &dst).unwrap();
        let t = image::open(&dst).unwrap();
        assert_eq!(t.width(), 320);
        assert_eq!(t.height(), 200);
        assert_eq!(image::guess_format(&std::fs::read(&dst).unwrap()).unwrap(), image::ImageFormat::Jpeg);
        assert!(!dir.path().join("thumbs/book/src.png.jpg.tmp").exists());
        // 小图不放大
        let small = dir.path().join("small.png");
        image::RgbImage::from_pixel(100, 60, image::Rgb([1, 2, 3])).save(&small).unwrap();
        let dst2 = dir.path().join("small.jpg");
        make_thumbnail(&small, &dst2).unwrap();
        assert_eq!(image::open(&dst2).unwrap().width(), 100);
    }

    fn test_ctx(dir: &Path) -> SyncCtx {
        SyncCtx {
            db: Arc::new(crate::db::Db::open_memory().unwrap()),
            http: reqwest::Client::builder().connect_timeout(Duration::from_secs(2)).build().unwrap(),
            throttle: Arc::new(Throttle::new(4, Duration::from_millis(0))),
            data_dir: dir.to_path_buf(),
            cancel: CancellationToken::new(),
            concurrency: 2,
            controller: Arc::new(crate::state::SyncController::new()),
            update_status: false,
            emit: Arc::new(|_| {}),
            inflight: Arc::new(Mutex::new(std::collections::HashSet::new())),
            style: Arc::new(crate::style::StyleState::new()),
        }
    }

    fn job(dir: &Path, kind: ImageKind) -> ImageJob {
        ImageJob {
            entry_id: "e1".into(),
            codex_id: "b".into(),
            ordinal: 0,
            kind,
            url: "http://127.0.0.1:9/x.png".into(),
            dest: dir.join("x.png"),
            thumb: None,
            asset_rev: None,
        }
    }

    /// #3：下载路径统一 AppError，失败清单带 code；旧 sync_log 没有 code 也能读回。
    #[tokio::test]
    async fn download_errors_carry_app_error_codes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = download_file(&ctx.http, &ctx.throttle, &cancel, "http://127.0.0.1:9/never", &dir.path().join("x.png")).await.unwrap_err();
        assert_eq!(err.code, "cancelled");
        // 没人监听的端口：连接被拒 → network（单次尝试，不走 2/4/8 s 退避）
        let part = dir.path().join("y.png.part");
        let side = dir.path().join("y.png.part.etag");
        let err = download_attempt(&ctx.http, &ctx.throttle, &ctx.cancel, "http://127.0.0.1:9/y.png", &dir.path().join("y.png"), &part, &side)
            .await
            .unwrap_err();
        assert_eq!(err.code, "network", "{err}");
        let f = SyncFailure { url: "u".into(), error: err.message.clone(), code: err.code.clone() };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"code\":\"network\""), "{json}");
        let old: SyncFailure = serde_json::from_str(r#"{"url":"u","error":"e"}"#).unwrap();
        assert_eq!(old.code, "");
    }

    /// #4：在飞集合被占 → 跳过并计入 skipped；展示图与原图是不同的键。
    #[tokio::test]
    async fn inflight_key_skips_duplicate_job() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let display = job(dir.path(), ImageKind::Display);
        ctx.inflight.lock().insert(("e1".into(), 0, ImageKind::Display));
        let out = run_queue(&ctx, vec![display.clone()], |_, _| {}).await;
        assert_eq!((out.ok, out.failed, out.skipped), (0, 0, 1));
        assert!(out.failures.is_empty());
        assert!(ctx.inflight.lock().contains(&("e1".to_string(), 0, ImageKind::Display)), "跳过不应释放别人占的键");
        let original = job(dir.path(), ImageKind::Original);
        assert!(try_occupy(&ctx.inflight, &original).is_some(), "原图与展示图不互斥");
        assert!(try_occupy(&ctx.inflight, &display).is_none());
        // 释放后可再占
        ctx.inflight.lock().clear();
        let guard = try_occupy(&ctx.inflight, &display).unwrap();
        drop(guard);
        assert!(ctx.inflight.lock().is_empty(), "guard drop 后释放");
    }

    #[test]
    fn etag_md5_parsing() {
        assert_eq!(md5_from_etag("\"1b576eb297bf8ea72e0bd9e30e76d83e\"").as_deref(), Some("1b576eb297bf8ea72e0bd9e30e76d83e"));
        assert_eq!(md5_from_etag("W/\"1B576EB297BF8EA72E0BD9E30E76D83E\"").as_deref(), Some("1b576eb297bf8ea72e0bd9e30e76d83e"));
        assert_eq!(md5_from_etag("\"abc-1\""), None);
    }

    #[test]
    fn queue_builder_uses_asset_rev_and_download_all_flags() {
        let db = crate::db::Db::open_memory().unwrap();
        let media = Media { base_url: "https://assets.example".into(), image_prefix: "images".into(), original_prefix: "originals".into() };
        let book = crate::sync::upstream::parse_book(include_str!("fixtures/book_v1.json")).unwrap();
        db.with(|c| {
            crate::sync::upsert::ensure_release(c, "r-1", "p", "h", "f", "codex_raw/r-1")?;
            crate::sync::upsert::upsert_book(c, "r-1", &book, None, "t")
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jobs = db.with(|c| build_sync_queue(c, dir.path(), &media, &["artist_nai5_personal"])).unwrap();
        // 3 条主图，combo 的第二张不在默认队列
        assert_eq!(jobs.len(), 3);
        assert!(jobs.iter().all(|j| j.ordinal == 0 && j.kind == ImageKind::Display));
        assert_eq!(jobs[0].url, "https://assets.example/images/artist_nai5_personal/artist_nai5_personal_demo_0001.png");
        assert!(jobs[0].thumb.as_ref().unwrap().ends_with("thumbs/artist_nai5_personal/artist_nai5_personal_demo_0001.png.jpg"));
        // 标记「下载全部 + 原图」后：第二张展示图与 2 张原图入队
        db.with(|c| {
            let mut all = DownloadAll::new();
            all.insert("artist_nai5_personal_combo_v1_0001".into(), DownloadAllFlags { display_all: true, originals: true });
            save_download_all(c, &all)
        })
        .unwrap();
        let jobs = db.with(|c| build_sync_queue(c, dir.path(), &media, &["artist_nai5_personal"])).unwrap();
        assert_eq!(jobs.len(), 6);
        assert_eq!(jobs.iter().filter(|j| j.kind == ImageKind::Original).count(), 2);
        // 主图落地（asset_rev 一致）后不再入队；asset_rev 变了重新入队
        let local = dir.path().join("a.png");
        std::fs::write(&local, b"x").unwrap();
        db.with(|c| {
            c.execute(
                "UPDATE entry_image SET local_path = ?1, asset_rev = '28620c7a6fe3f640' WHERE entry_id = 'artist_nai5_personal_demo_0001'",
                params![local.to_string_lossy().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
        let jobs = db.with(|c| build_sync_queue(c, dir.path(), &media, &["artist_nai5_personal"])).unwrap();
        assert_eq!(jobs.len(), 5);
        db.with(|c| {
            c.execute("UPDATE codex_entry SET asset_rev = 'changed' WHERE entry_id = 'artist_nai5_personal_demo_0001'", [])?;
            Ok(())
        })
        .unwrap();
        let jobs = db.with(|c| build_sync_queue(c, dir.path(), &media, &["artist_nai5_personal"])).unwrap();
        assert_eq!(jobs.len(), 6);
        let entry_jobs = db
            .with(|c| build_entry_queue(c, dir.path(), &media, "artist_nai5_personal_combo_v1_0001", ImageKind::Original))
            .unwrap();
        assert_eq!(entry_jobs.len(), 2);
        assert!(entry_jobs[0].url.contains("/originals/"));
    }
}
