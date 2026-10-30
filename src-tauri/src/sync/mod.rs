//! 同步编排（设计 §1.3、CONTRACT §0/§2）：data-source → current → manifest → codexes/media/各本书
//! （两本 V5 词典必需；v0.5.16 起加所长 N5 法典，可选：单本失败只记一条、不写 latest_release，下次重试），
//! 每个 JSON 校验 sha256+size 并原文落盘到 `codex_raw/<release>/`；按书 `file_sha256` 跳过；
//! upsert 全部完成后才写 `app_state('latest_release')`；随后跑图片队列；结束写 `sync_log` 与 `app_event`。
//! release 未变且未强制时只做图片补缺（noop / resume）。任何一步失败不回滚已落地内容。
//! v0.3.0（CONTRACT §12.2）：图片阶段之后多一个 `embedding` 阶段（`style::index::build_index`）给展示图算画风向量；
//! 模型未下载则发一条「画风模型未下载，跳过索引」事件；索引出错不算同步失败，只写 app_event 并拼进结束消息。
//! 子模块：`upstream`（形状/规范化）、`upsert`（条目入库）、`images`（下载/续传/缩略图）。

pub mod images;
pub mod nax;
pub mod upsert;
pub mod upstream;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::params;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::db::{now_iso, Db};
use crate::error::{AppError, AppResult};
use crate::models::{SyncEvent, SyncFailure, SyncPhase};
use crate::state::{InflightSet, SyncController, Throttle};
use crate::style::{index as style_index, StyleState};

use images::{ImageKind, ImageJob};
use upstream::{Media, BOOK_WHITELIST};

pub const DATA_SOURCE_URL: &str = "https://novelai.quicktagcloud.com/data-source.json";

pub type Emitter = Arc<dyn Fn(SyncEvent) + Send + Sync>;

pub struct SyncCtx {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    pub throttle: Arc<Throttle>,
    pub data_dir: PathBuf,
    pub cancel: CancellationToken,
    pub concurrency: usize,
    pub controller: Arc<SyncController>,
    /// 主同步更新 `SyncController.status`；按需下载只发事件。
    pub update_status: bool,
    pub emit: Emitter,
    /// 与 `AppState.inflight` 同一份：主同步与按需下载不会同时下同一张图。
    pub inflight: InflightSet,
    /// v0.3.0：画风模型缓存（`AppState.style` 同一份），embedding 阶段用。
    pub style: Arc<StyleState>,
}

#[derive(Debug, Default, Clone)]
struct Progress {
    release: Option<String>,
    added: u64,
    changed: u64,
    images_total: u64,
    images_done: u64,
    images_failed: u64,
    embed_total: u64,
    embed_done: u64,
    embed_failed: u64,
}

struct Outcome {
    kind: &'static str,
    message: String,
    images_ok: u64,
    images_failed: u64,
    failures: Vec<SyncFailure>,
    error: Option<AppError>,
}

impl SyncCtx {
    fn emit(&self, phase: SyncPhase, message: impl Into<String>, p: &Progress) {
        let message = message.into();
        if self.update_status {
            self.controller.update(|s| {
                s.phase = phase;
                s.message = message.clone();
                s.images_total = p.images_total;
                s.images_done = p.images_done;
                s.images_failed = p.images_failed;
                if p.release.is_some() {
                    s.release_remote = p.release.clone();
                }
            });
        }
        (self.emit)(SyncEvent {
            phase,
            message,
            release: p.release.clone(),
            added: p.added,
            changed: p.changed,
            images_total: p.images_total,
            images_done: p.images_done,
            images_failed: p.images_failed,
            embed_total: p.embed_total,
            embed_done: p.embed_done,
            embed_failed: p.embed_failed,
        });
    }

    /// manifest 阶段的 GET：请求与取消令牌一起等，`sync_cancel` 立即生效，不必等 60 s 超时（v0.1.1 #5）。
    async fn fetch_bytes(&self, url: &str, throttled: bool) -> AppResult<Vec<u8>> {
        let _permit = if throttled {
            Some(tokio::select! {
                permit = self.throttle.acquire() => permit,
                _ = self.cancel.cancelled() => return Err(AppError::cancelled()),
            })
        } else { None };
        self.check_cancel()?;
        let started = std::time::Instant::now();
        let resp = tokio::select! {
            r = self.http.get(url).timeout(Duration::from_secs(60)).send() => r.map_err(|e| AppError::network(format!("{url}: {e}")))?,
            _ = self.cancel.cancelled() => return Err(AppError::cancelled()),
        };
        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::upstream(format!("{url}: HTTP {status}")));
        }
        let bytes = tokio::select! {
            b = resp.bytes() => b.map_err(|e| AppError::network(format!("{url}: {e}")))?.to_vec(),
            _ = self.cancel.cancelled() => return Err(AppError::cancelled()),
        };
        tracing::info!(url, status = status.as_u16(), bytes = bytes.len(), ms = started.elapsed().as_millis() as u64, "GET");
        Ok(bytes)
    }

    fn check_cancel(&self) -> AppResult<()> {
        if self.cancel.is_cancelled() {
            Err(AppError::cancelled())
        } else {
            Ok(())
        }
    }
}

fn rel_dir_of_manifest(manifest_path: &str) -> String {
    match manifest_path.rfind('/') {
        Some(i) => manifest_path[..i].to_string(),
        None => String::new(),
    }
}

fn join_url(base: &str, rel: &str, name: &str) -> String {
    let mut s = base.trim_end_matches('/').to_string();
    if !rel.is_empty() {
        s.push('/');
        s.push_str(rel.trim_matches('/'));
    }
    s.push('/');
    s.push_str(name);
    s
}

fn load_media(db: &Db) -> AppResult<Option<Media>> {
    Ok(db.app_state_get("media_json")?.and_then(|s| serde_json::from_str(&s).ok()))
}

/// 主同步入口：由命令层 `spawn`，自身负责收尾（sync_log / app_event / done|error 事件 / 释放 running）。
pub async fn run_sync(ctx: Arc<SyncCtx>, force: bool) {
    let started_at = now_iso();
    let progress = Mutex::new(Progress::default());
    ctx.db.log_event("info", "sync", if force { "同步开始（强制）" } else { "同步开始" }, json!({}));
    let outcome = match sync_inner(&ctx, force, &progress).await {
        Ok(o) => o,
        Err(e) => {
            let p = progress.lock().clone();
            let cancelled = e.code == "cancelled";
            Outcome {
                kind: if cancelled { "partial" } else { "failed" },
                message: if cancelled { "已取消".into() } else { e.message.clone() },
                images_ok: p.images_done,
                images_failed: p.images_failed,
                failures: vec![],
                error: if cancelled { None } else { Some(e) },
            }
        }
    };
    let p = progress.lock().clone();
    let finished_at = now_iso();
    let _ = ctx.db.with(|c| {
        c.execute(
            "INSERT INTO sync_log(started_at, finished_at, kind, release_id, added, changed, images_ok, images_failed, failures_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                started_at,
                finished_at,
                outcome.kind,
                p.release,
                p.added as i64,
                p.changed as i64,
                outcome.images_ok as i64,
                outcome.images_failed as i64,
                serde_json::to_string(&outcome.failures).unwrap_or_else(|_| "[]".into()),
            ],
        )?;
        Ok(())
    });
    let data = json!({
        "kind": outcome.kind, "release": p.release, "added": p.added, "changed": p.changed,
        "images_ok": outcome.images_ok, "images_failed": outcome.images_failed,
    });
    match &outcome.error {
        Some(e) => {
            ctx.db.log_event("error", "sync", &format!("同步失败: {}", e.message), data);
            ctx.emit(SyncPhase::Error, outcome.message.clone(), &p);
        }
        None => {
            ctx.db.log_event(if outcome.images_failed > 0 || outcome.kind == "partial" { "warn" } else { "info" }, "sync", &format!("同步结束（{}）", outcome.kind), data);
            ctx.emit(SyncPhase::Done, outcome.message.clone(), &p);
        }
    }
    if ctx.update_status {
        ctx.controller.update(|s| {
            s.release_local = ctx.db.app_state_get("latest_release").ok().flatten();
        });
        ctx.controller.finish();
    }
}

async fn sync_codex_entries(ctx: &SyncCtx, force: bool, progress: &Mutex<Progress>) -> AppResult<(Media, bool, Vec<String>, Vec<SyncFailure>)> {
    let snapshot = |p: &Mutex<Progress>| p.lock().clone();
    ctx.emit(SyncPhase::Manifest, "读取上游索引", &snapshot(progress));

    // 1. data-source.json（本主机每次同步只打这一枪）→ current.json
    let ds = upstream::parse_data_source(&String::from_utf8_lossy(&ctx.fetch_bytes(DATA_SOURCE_URL, false).await?))?;
    let current_url = join_url(&ds.base_url, "", &ds.pointer);
    let current_raw = ctx.fetch_bytes(&current_url, true).await?;
    let current = upstream::parse_current(&String::from_utf8_lossy(&current_raw))?;
    let rel_dir = rel_dir_of_manifest(&current.manifest);
    let local_release = ctx.db.app_state_get("latest_release")?;
    {
        let mut p = progress.lock();
        p.release = Some(current.release.clone());
    }
    if ctx.update_status {
        ctx.controller.update(|s| {
            s.release_local = local_release.clone();
            s.release_remote = Some(current.release.clone());
        });
    }
    ctx.check_cancel()?;

    let mut entries_done = false;
    let mut notes: Vec<String> = Vec::new();
    let mut optional_failures: Vec<SyncFailure> = Vec::new();
    let media: Media;
    // 白名单里还没进过库的书（例如刚升级、第一次同步所长法典）：release 没变也要走完整流程把它拉下来。
    let missing_books: Vec<&str> = ctx.db.with(|c| {
        let mut missing = Vec::new();
        for book in BOOK_WHITELIST {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM codex_book WHERE codex_id = ?1", params![book], |r| r.get(0))?;
            if n == 0 {
                missing.push(*book);
            }
        }
        Ok(missing)
    })?;

    if local_release.as_deref() == Some(current.release.as_str()) && !force && missing_books.is_empty() {
        // 已是最新：直接图片补缺
        ctx.emit(SyncPhase::Manifest, format!("已是最新 release {}，检查图片补缺", current.release), &snapshot(progress));
        media = match load_media(&ctx.db)? {
            Some(m) => m,
            None => {
                let raw = ctx.fetch_bytes(&join_url(&ds.base_url, &rel_dir, "media.json"), true).await?;
                let m = upstream::parse_media(&String::from_utf8_lossy(&raw))?;
                ctx.db.app_state_set("media_json", &String::from_utf8_lossy(&raw))?;
                m
            }
        };
    } else {
        // 2. 同步前备份
        match ctx.db.backup_into(&ctx.data_dir.join("backups"), Some("presync"), 10) {
            Ok(p) => tracing::info!(path = %p.display(), "presync backup"),
            Err(e) => ctx.db.log_event("warn", "backup", &format!("同步前备份失败: {e}"), json!({})),
        }
        // 3. manifest
        ctx.emit(SyncPhase::Manifest, format!("读取 release {} 清单", current.release), &snapshot(progress));
        let manifest_raw = ctx.fetch_bytes(&join_url(&ds.base_url, "", &current.manifest), true).await?;
        let manifest = upstream::parse_manifest(&String::from_utf8_lossy(&manifest_raw))?;
        let raw_dir_rel = format!("codex_raw/{}", current.release);
        let raw_dir = ctx.data_dir.join(&raw_dir_rel);
        std::fs::create_dir_all(&raw_dir)?;
        std::fs::write(raw_dir.join("current.json"), &current_raw)?;
        std::fs::write(raw_dir.join("manifest.json"), &manifest_raw)?;

        let fetch_verified = |name: &str| {
            let name = name.to_string();
            let url = join_url(&ds.base_url, &rel_dir, &name);
            let meta = manifest.files.get(&name).cloned();
            let raw_dir = raw_dir.clone();
            async move {
                let bytes = ctx.fetch_bytes(&url, true).await?;
                match &meta {
                    Some(m) => upstream::verify_file(&name, &bytes, m)?,
                    None => tracing::warn!(name, "manifest 未列出该文件，跳过校验"),
                }
                std::fs::write(raw_dir.join(&name), &bytes)?;
                Ok::<Vec<u8>, AppError>(bytes)
            }
        };
        let codexes_raw = fetch_verified("codexes.json").await?;
        let codexes: serde_json::Value = serde_json::from_slice(&codexes_raw).map_err(|e| AppError::upstream(format!("codexes.json: {e}")))?;
        let listed = |book: &str| codexes.as_array().map(|a| a.iter().any(|b| b.get("id").and_then(|i| i.as_str()) == Some(book))).unwrap_or(false);
        for book in BOOK_WHITELIST {
            if !upstream::is_codex_book(book) && !listed(book) {
                return Err(AppError::upstream(format!("codexes.json 里没有 {book}")));
            }
        }
        let media_raw = fetch_verified("media.json").await?;
        media = upstream::parse_media(&String::from_utf8_lossy(&media_raw))?;
        ctx.db.app_state_set("media_json", &String::from_utf8_lossy(&media_raw))?;

        let now = now_iso();
        ctx.db.with(|c| upsert::ensure_release(c, &current.release, &current.published_at, &current.content_hash, &now, &raw_dir_rel))?;

        // 4. 各本书：文件名一律 `<codex_id>.json`。必需的两本出错就中止；所长法典单本出错记一条跳过。
        for book_id in BOOK_WHITELIST {
            ctx.check_cancel()?;
            if upstream::is_codex_book(book_id) && !listed(book_id) {
                notes.push(format!("{book_id} 不在本次 release 的书目里，已跳过"));
                continue;
            }
            let file_name = format!("{book_id}.json");
            let book_result: AppResult<()> = async {
                let meta = manifest.files.get(&file_name).ok_or_else(|| AppError::upstream(format!("manifest 缺 {file_name}")))?.clone();
                let local_sha: Option<String> = ctx.db.with(|c| {
                    Ok(c.query_row("SELECT file_sha256 FROM codex_book WHERE codex_id = ?1", params![book_id], |r| r.get::<_, Option<String>>(0))
                        .ok()
                        .flatten())
                })?;
                if !force && local_sha.as_deref().map(|s| s.eq_ignore_ascii_case(&meta.sha256)).unwrap_or(false) {
                    tracing::info!(book = book_id, "file_sha256 未变，跳过该书");
                    notes.push(format!("{book_id} 未变化"));
                    ctx.db.with(|c| {
                        c.execute("UPDATE codex_book SET release_id = ?2 WHERE codex_id = ?1", params![book_id, current.release])?;
                        Ok(())
                    })?;
                    return Ok(());
                }
                ctx.emit(SyncPhase::Entries, format!("下载 {file_name}"), &snapshot(progress));
                let bytes = fetch_verified(&file_name).await?;
                let text = String::from_utf8(bytes).map_err(|e| AppError::upstream(format!("{file_name} 非 UTF-8: {e}")))?;
                let book = tokio::task::spawn_blocking(move || upstream::parse_book(&text)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
                if book.id != *book_id {
                    return Err(AppError::upstream(format!("{file_name} 的 id 是 {}，期望 {book_id}", book.id)));
                }
                ctx.emit(SyncPhase::Entries, format!("写入 {}（{} 条）", book.title, book.entries.len()), &snapshot(progress));
                let release = current.release.clone();
                let sha = meta.sha256.clone();
                let now = now_iso();
                let stats = ctx.db.with(|c| upsert::upsert_book(c, &release, &book, Some(&sha), &now))?;
                {
                    let mut p = progress.lock();
                    p.added += stats.added;
                    p.changed += stats.changed;
                }
                ctx.db.log_event(
                    "info",
                    "sync",
                    &format!("{} 条目入库", book.title),
                    json!({ "codex_id": book_id, "added": stats.added, "changed": stats.changed, "unchanged": stats.unchanged, "removed": stats.removed, "missing": stats.missing }),
                );
                if stats.removed_skipped {
                    let msg = format!("{}：本次 release 消失 {} 条（≥ 10%），已暂停自动标记 removed_at，请人工核对", book.title, stats.missing);
                    ctx.db.log_event("warn", "sync", &msg, json!({ "codex_id": book_id, "missing": stats.missing }));
                    notes.push(msg);
                }
                ctx.emit(SyncPhase::Entries, format!("{} 完成：新增 {} 变更 {}", book.title, stats.added, stats.changed), &snapshot(progress));
                Ok(())
            }
            .await;
            match book_result {
                Ok(()) => {}
                Err(e) if e.code == "cancelled" || !upstream::is_codex_book(book_id) => return Err(e),
                Err(e) => {
                    let msg = format!("{book_id} 同步失败：{}；已跳过，其他书照常，下次同步重试", e.message);
                    ctx.db.log_event("warn", "sync", &msg, json!({ "codex_id": book_id, "code": e.code }));
                    notes.push(msg);
                    optional_failures.push(SyncFailure { url: join_url(&ds.base_url, &rel_dir, &file_name), error: e.message.clone(), code: e.code.clone() });
                }
            }
        }
        entries_done = true;
        // 有书没同步成功就不记 latest_release：下次同步不会被「已是最新」挡掉，没变的书按 sha256 跳过，只重试失败那本。
        if optional_failures.is_empty() {
            ctx.db.app_state_set("latest_release", &current.release)?;
            if ctx.update_status {
                ctx.controller.update(|s| s.release_local = Some(current.release.clone()));
            }
        }
    }

    Ok((media, entries_done, notes, optional_failures))
}

fn record_source_failure(ctx: &SyncCtx, source: &str, url: &str, e: &AppError, notes: &mut Vec<String>, failures: &mut Vec<SyncFailure>) {
    let message = format!("{source} 同步失败：{}；已保留本地条目和筛选，其他来源继续", e.message);
    ctx.db.log_event("warn", "sync", &message, json!({ "source": source, "code": e.code }));
    notes.push(message);
    failures.push(SyncFailure { url: url.into(), error: e.message.clone(), code: e.code.clone() });
}

fn parse_nax_page(bytes: &[u8]) -> AppResult<nax::GalleryPage> {
    let html = std::str::from_utf8(bytes).map_err(|e| AppError::upstream(format!("NAX HTML 不是有效 UTF-8：{e}")))?;
    nax::parse_gallery_page(html)
}

async fn sync_nax_entries(ctx: &SyncCtx, force: bool, progress: &Mutex<Progress>, already_backed_up: bool) -> AppResult<bool> {
    ctx.emit(SyncPhase::Manifest, "检查 NAX · V5 画师测试库", &progress.lock().clone());
    let first_raw = ctx.fetch_bytes(nax::GALLERY_URL, true).await?;
    let first = parse_nax_page(&first_raw)?;
    let urls = nax::page_urls(&first);
    let mut pages = vec![first.clone()];
    let mut raw_pages = vec![first_raw];
    let total = urls.len() + 1;
    for (i, url) in urls.iter().enumerate() {
        ctx.check_cancel()?;
        ctx.emit(SyncPhase::Entries, format!("读取 NAX 画师目录 {}/{} 页", i + 2, total), &progress.lock().clone());
        let raw = ctx.fetch_bytes(url, true).await?;
        pages.push(parse_nax_page(&raw)?);
        raw_pages.push(raw);
    }
    // A live paginated gallery can grow while being read. Do not publish a moving snapshot.
    let check_raw = ctx.fetch_bytes(nax::GALLERY_URL, true).await?;
    let check = parse_nax_page(&check_raw)?;
    if !nax::same_inventory(&first, &check) {
        return Err(AppError::upstream("NAX 目录在同步期间发生变化，请重试；本地筛选已保留"));
    }
    let mut snapshot = nax::assemble_book(&pages)?;
    ctx.check_cancel()?;
    let previous = ctx.db.with(|c| {
        Ok(c.query_row("SELECT file_sha256 FROM codex_book WHERE codex_id = ?1", params![nax::BOOK_ID], |r| r.get::<_, Option<String>>(0)).ok().flatten())
    })?;
    if !force && previous.as_deref() == Some(snapshot.content_hash.as_str()) {
        ctx.emit(SyncPhase::Entries, format!("NAX 未变化（{} 条），检查图片补缺", snapshot.book.entries.len()), &progress.lock().clone());
        return Ok(false);
    }
    if !already_backed_up {
        match ctx.db.backup_into(&ctx.data_dir.join("backups"), Some("presync"), 10) {
            Ok(path) => tracing::info!(path = %path.display(), "NAX presync backup"),
            Err(e) => return Err(AppError::new("backup", format!("NAX 同步前备份失败：{e}"))),
        }
    }
    let release = format!("nax-v5-{}", &snapshot.content_hash[..16]);
    snapshot.book.update_filters = Some(json!([{ "id": release, "label": "NAX 当前同步", "latest": true }]));
    let first_seen = ctx.db.with(|c| {
        let mut statement = c.prepare("SELECT entry_id, first_seen_release FROM codex_entry WHERE codex_id = ?1")?;
        let rows = statement.query_map(params![nax::BOOK_ID], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<Result<std::collections::HashMap<_, _>, _>>()?)
    })?;
    for entry in &mut snapshot.book.entries {
        entry.update_batches = Some(vec![release.clone()]);
        entry.is_new = first_seen.get(&entry.id).map_or(true, |seen| seen == &release);
    }
    let raw_dir_rel = format!("codex_raw/{release}");
    let raw_dir = ctx.data_dir.join(&raw_dir_rel);
    std::fs::create_dir_all(&raw_dir)?;
    for (i, raw) in raw_pages.iter().enumerate() {
        std::fs::write(raw_dir.join(format!("page-{}.html", i + 1)), raw)?;
    }
    std::fs::write(raw_dir.join("snapshot.json"), &snapshot.raw)?;
    let now = now_iso();
    ctx.emit(SyncPhase::Entries, format!("写入 NAX · V5 画师测试库（{} 条）", snapshot.book.entries.len()), &progress.lock().clone());
    let stats = ctx.db.with(|c| {
        upsert::ensure_release(c, &release, &now, &snapshot.content_hash, &now, &raw_dir_rel)?;
        let stats = upsert::upsert_book(c, &release, &snapshot.book, Some(&snapshot.content_hash), &now)?;
        c.execute("UPDATE codex_entry SET is_new = (first_seen_release = ?2) WHERE codex_id = ?1", params![nax::BOOK_ID, release])?;
        Ok(stats)
    })?;
    {
        let mut p = progress.lock();
        p.added += stats.added;
        p.changed += stats.changed;
    }
    ctx.db.log_event("info", "sync", "NAX 条目入库", json!({
        "codex_id": nax::BOOK_ID, "release": release, "added": stats.added,
        "changed": stats.changed, "unchanged": stats.unchanged, "retained_missing": stats.missing,
    }));
    ctx.emit(SyncPhase::Entries, format!("NAX 完成：新增 {} 变更 {}；保留 {} 条暂未出现的旧条目", stats.added, stats.changed, stats.missing), &progress.lock().clone());
    Ok(true)
}

async fn sync_inner(ctx: &SyncCtx, force: bool, progress: &Mutex<Progress>) -> AppResult<Outcome> {
    let snapshot = |p: &Mutex<Progress>| p.lock().clone();
    let mut entries_done = false;
    let mut notes = Vec::new();
    let mut source_failures = Vec::new();
    // Each source owns its version. An unchanged 法典 release must still check NAX.
    let media = match sync_codex_entries(ctx, force, progress).await {
        Ok((media, changed, source_notes, book_failures)) => {
            entries_done |= changed;
            notes.extend(source_notes);
            source_failures.extend(book_failures);
            Some(media)
        }
        Err(e) if e.code == "cancelled" => return Err(e),
        Err(e) => {
            record_source_failure(ctx, "法典", DATA_SOURCE_URL, &e, &mut notes, &mut source_failures);
            load_media(&ctx.db)?
        }
    };
    match sync_nax_entries(ctx, force, progress, entries_done).await {
        Ok(changed) => entries_done |= changed,
        Err(e) if e.code == "cancelled" => return Err(e),
        Err(e) => record_source_failure(ctx, "NAX", nax::GALLERY_URL, &e, &mut notes, &mut source_failures),
    }

    // 5. 图片
    ctx.check_cancel()?;
    let jobs: Vec<ImageJob> = {
        let data_dir = ctx.data_dir.clone();
        ctx.db.with(|c| {
            let mut jobs = if let Some(media) = &media {
                images::build_sync_queue(c, &data_dir, media, BOOK_WHITELIST)?
            } else {
                Vec::new()
            };
            jobs.extend(images::build_sync_queue(c, &data_dir, &nax::media(), &[nax::BOOK_ID])?);
            Ok(jobs)
        })?
    };
    {
        let mut p = progress.lock();
        p.images_total = jobs.len() as u64;
    }
    ctx.emit(SyncPhase::Images, format!("图片队列 {} 张", jobs.len()), &snapshot(progress));
    let outcome = images::run_queue(ctx, jobs, |done, failed| {
        let p = {
            let mut p = progress.lock();
            p.images_done = done;
            p.images_failed = failed;
            p.clone()
        };
        ctx.emit(SyncPhase::Images, format!("图片 {}/{}", done, p.images_total), &p);
    })
    .await;
    // 6. 画风索引（§12.2）：模型未下载 → 跳过并发一条事件；单张失败记 warn 继续；取消令牌同一个；索引出错不算同步失败
    if !ctx.cancel.is_cancelled() {
        ctx.emit(SyncPhase::Embedding, "准备画风索引", &snapshot(progress));
        let on_progress = |total: u64, done: u64, failed: u64| {
            let p = {
                let mut p = progress.lock();
                p.embed_total = total;
                p.embed_done = done;
                p.embed_failed = failed;
                p.clone()
            };
            ctx.emit(SyncPhase::Embedding, format!("画风向量 {done}/{total}"), &p);
        };
        match style_index::build_index(&ctx.db, &ctx.data_dir, &ctx.style, &ctx.cancel, false, &on_progress).await {
            Ok(o) if o.model_missing => {
                ctx.emit(SyncPhase::Embedding, style_index::SKIP_MESSAGE, &snapshot(progress));
                notes.push(o.summary());
            }
            Ok(o) => {
                if o.total > 0 {
                    ctx.db.log_event(
                        if o.failed > 0 { "warn" } else { "info" },
                        "style",
                        &format!("画风索引结束：{}", o.summary()),
                        json!({ "model": o.model, "total": o.total, "done": o.done, "failed": o.failed }),
                    );
                    notes.push(o.summary());
                }
            }
            Err(e) if e.code == "cancelled" => {}
            Err(e) => {
                ctx.db.log_event("error", "style", &format!("画风索引失败: {}", e.message), json!({}));
                notes.push(format!("画风索引失败: {}", e.message));
            }
        }
    }
    let cancelled = ctx.cancel.is_cancelled();
    let p = snapshot(progress);
    let kind: &'static str = if !source_failures.is_empty() {
        "partial"
    } else if cancelled {
        "partial"
    } else if entries_done {
        if outcome.failed > 0 { "partial" } else { "full" }
    } else if outcome.failed > 0 {
        "partial"
    } else if outcome.ok > 0 {
        "resume"
    } else {
        "noop"
    };
    let mut message = if cancelled {
        "已取消".to_string()
    } else if entries_done {
        format!("本次新增 {} 条、变更 {} 条；图片 {}/{}，失败 {}", p.added, p.changed, outcome.ok, p.images_total, outcome.failed)
    } else if !source_failures.is_empty() {
        format!("部分来源未完成；补齐图片 {} 张，失败 {}", outcome.ok, outcome.failed)
    } else if outcome.ok + outcome.failed == 0 {
        format!("法典与 NAX 已是最新（{}），无需更新", p.release.clone().unwrap_or_default())
    } else {
        format!("补齐图片 {} 张，失败 {}", outcome.ok, outcome.failed)
    };
    if outcome.skipped > 0 {
        notes.push(format!("{} 张图另一任务正在下载，本次跳过", outcome.skipped));
    }
    if !notes.is_empty() {
        message.push_str("；");
        message.push_str(&notes.join("；"));
    }
    source_failures.extend(outcome.failures);
    Ok(Outcome { kind, message, images_ok: outcome.ok, images_failed: outcome.failed, failures: source_failures, error: None })
}

/// 按需下载（CONTRACT §1 `download_images`）：记下「下载全部」标记，跑该条目的队列，与同步共用节流与在飞集合。
pub async fn run_entry_download(ctx: Arc<SyncCtx>, entry_id: String, kind: ImageKind) {
    let progress = Mutex::new(Progress::default());
    let result: AppResult<(u64, u64, u64)> = async {
        let codex_id: String = ctx.db.with(|c| {
            Ok(c.query_row("SELECT codex_id FROM codex_entry WHERE entry_id = ?1", params![entry_id], |r| r.get(0))?)
        })?;
        let media = if codex_id == nax::BOOK_ID {
            nax::media()
        } else {
            load_media(&ctx.db)?.ok_or_else(|| AppError::bad_request("尚未同步过法典，没有 media.json"))?
        };
        let data_dir = ctx.data_dir.clone();
        let jobs = ctx.db.with(|c| {
            let mut all = images::load_download_all(c);
            let flags = all.entry(entry_id.clone()).or_default();
            match kind {
                ImageKind::Display => flags.display_all = true,
                ImageKind::Original => flags.originals = true,
            }
            images::save_download_all(c, &all)?;
            images::build_entry_queue(c, &data_dir, &media, &entry_id, kind)
        })?;
        {
            let mut p = progress.lock();
            p.images_total = jobs.len() as u64;
        }
        ctx.emit(SyncPhase::Images, format!("队列 {} 张", jobs.len()), &progress.lock().clone());
        let outcome = images::run_queue(&ctx, jobs, |done, failed| {
            let p = {
                let mut p = progress.lock();
                p.images_done = done;
                p.images_failed = failed;
                p.clone()
            };
            ctx.emit(SyncPhase::Images, format!("图片 {}/{}", done, p.images_total), &p);
        })
        .await;
        Ok((outcome.ok, outcome.failed, outcome.skipped))
    }
    .await;
    let p = progress.lock().clone();
    match result {
        Ok((ok, failed, skipped)) => {
            ctx.db.log_event("info", "download", "按需下载结束", json!({ "entry_id": entry_id, "ok": ok, "failed": failed, "skipped": skipped }));
            let mut msg = format!("下载完成 {ok} 张，失败 {failed}");
            if skipped > 0 {
                msg.push_str(&format!("，{skipped} 张同步正在下载已跳过"));
            }
            ctx.emit(SyncPhase::Done, msg, &p);
        }
        Err(e) => {
            ctx.db.log_event("error", "download", &format!("按需下载失败: {}", e.message), json!({ "entry_id": entry_id }));
            ctx.emit(SyncPhase::Error, e.message, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nax_rejects_invalid_utf8_before_it_can_change_artist_identity() {
        let mut raw = include_bytes!("fixtures/nax_home.html").to_vec();
        raw.extend_from_slice(&[0xff]);
        let error = parse_nax_page(&raw).unwrap_err();
        assert_eq!(error.code, "upstream");
        assert!(error.message.contains("UTF-8"));
    }

    #[test]
    fn url_helpers() {
        assert_eq!(rel_dir_of_manifest("releases/r-1/manifest.json"), "releases/r-1");
        assert_eq!(rel_dir_of_manifest("manifest.json"), "");
        assert_eq!(join_url("https://a/data/", "releases/r-1", "media.json"), "https://a/data/releases/r-1/media.json");
        assert_eq!(join_url("https://a/data", "", "current.json"), "https://a/data/current.json");
    }
}
