//! 联网冒烟测试（默认跳过）：`LIVE_SMOKE=1 cargo test --test live_smoke -- --nocapture`
//!
//! 走真实的法典图鉴 release 链：data-source → current → manifest → media + 白名单各本书（sha256 校验）→
//! upsert 进临时库 → 构建图片队列 → 只下载前 4 张（含缩略图与 ETag 落库）。用来验证上游形状与下载代码，
//! 不做全量图片同步。v0.5.16 起白名单含所长 N5 法典，下载的 4 张里含一张法典图。

use std::{sync::Arc, time::Duration};

use nai5_codex_lab_lib::db::{now_iso, Db};
use nai5_codex_lab_lib::state::{SyncController, Throttle};
use nai5_codex_lab_lib::sync::images::{build_sync_queue, run_queue};
use nai5_codex_lab_lib::sync::upsert::{ensure_release, upsert_book};
use nai5_codex_lab_lib::sync::upstream::{is_codex_book, parse_book, parse_current, parse_data_source, parse_manifest, parse_media, verify_file, BOOK_WHITELIST};
use nai5_codex_lab_lib::sync::SyncCtx;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn live_sync_smoke() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("LIVE_SMOKE").is_err() {
        eprintln!("LIVE_SMOKE 未设置，跳过联网冒烟");
        return Ok(());
    }
    let tmp = tempfile::tempdir()?;
    let data_dir = tmp.path().to_path_buf();
    for d in ["images", "originals", "thumbs", "codex_raw", "backups", "exports", "logs"] {
        std::fs::create_dir_all(data_dir.join(d))?;
    }
    let db = Arc::new(Db::open(&data_dir.join("library.db"))?);
    let http = reqwest::Client::builder().user_agent("nai5-codex-lab/smoke").build()?;

    let ds = parse_data_source(&http.get("https://novelai.quicktagcloud.com/data-source.json").send().await?.text().await?)?;
    let base = ds.base_url.trim_end_matches('/').to_string();
    let cur = parse_current(&http.get(format!("{base}/{}", ds.pointer)).send().await?.text().await?)?;
    let rel_dir = cur.manifest.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let rel_base = format!("{base}/{rel_dir}");
    eprintln!("release {} @ {rel_base}", cur.release);

    let man = parse_manifest(&http.get(format!("{rel_base}/manifest.json")).send().await?.text().await?)?;
    let media_raw = http.get(format!("{rel_base}/media.json")).send().await?.bytes().await?;
    verify_file("media.json", &media_raw, man.files.get("media.json").expect("manifest 有 media.json"))?;
    let media = parse_media(std::str::from_utf8(&media_raw)?)?;

    let now = now_iso();
    db.with(|c| ensure_release(c, &cur.release, &cur.published_at, &cur.content_hash, &now, "smoke"))?;

    for book in BOOK_WHITELIST {
        let name = format!("{book}.json");
        let bytes = http.get(format!("{rel_base}/{name}")).send().await?.bytes().await?;
        let meta = man.files.get(&name).expect("manifest 有该书");
        verify_file(&name, &bytes, meta)?;
        let parsed = parse_book(std::str::from_utf8(&bytes)?)?;
        // 所长 N5 常规只有几百条，两本 V5 词典都是上千条
        let floor = if is_codex_book(book) { 200 } else { 1000 };
        assert!(parsed.entries.len() > floor, "{book} 条目数异常: {}", parsed.entries.len());
        let sha = meta.sha256.clone();
        db.with(|c| upsert_book(c, &cur.release, &parsed, Some(&sha), &now))?;
        eprintln!("{book}: v{} entries {}", parsed.version, parsed.entries.len());
    }

    let (entries, images, parses): (i64, i64, i64) = db.with(|c| {
        let e: i64 = c.query_row("SELECT COUNT(*) FROM codex_entry", [], |r| r.get(0))?;
        let i: i64 = c.query_row("SELECT COUNT(*) FROM entry_image", [], |r| r.get(0))?;
        let p: i64 = c.query_row("SELECT COUNT(*) FROM artist_parse", [], |r| r.get(0))?;
        Ok((e, i, p))
    })?;
    eprintln!("codex_entry {entries} entry_image {images} artist_parse {parses}");
    assert!(entries >= 3000 && images >= 4000 && parses >= 1000);

    let mut jobs = db.with(|c| build_sync_queue(c, &data_dir, &media, BOOK_WHITELIST))?;
    eprintln!("图片队列 {} 张（只取前 4 张，最后一张换成法典图）", jobs.len());
    assert!(jobs.len() >= 3000);
    let codex_job = jobs.iter().position(|j| j.entry_id.starts_with("suozhang_nai5")).expect("队列里有法典图");
    let codex_job = jobs.remove(codex_job);
    jobs.truncate(3);
    jobs.push(codex_job);

    let ctx = Arc::new(SyncCtx {
        db: db.clone(),
        http,
        throttle: Arc::new(Throttle::new(4, Duration::from_millis(100))),
        data_dir: data_dir.clone(),
        cancel: CancellationToken::new(),
        concurrency: 2,
        controller: Arc::new(SyncController::new()),
        update_status: false,
        emit: Arc::new(|_| {}),
        inflight: Arc::new(parking_lot::Mutex::new(std::collections::HashSet::new())),
        style: Arc::new(nai5_codex_lab_lib::style::StyleState::new()),
    });
    let out = run_queue(&ctx, jobs, |d, f| eprintln!("  图片 done {d} failed {f}")).await;
    eprintln!("下载 ok {} failed {} {:?}", out.ok, out.failed, out.failures);
    assert_eq!(out.failed, 0);
    assert_eq!(out.ok, 4);

    let rows: Vec<(String, Option<String>, Option<String>, Option<String>)> = db.with(|c| {
        let mut st = c.prepare("SELECT entry_id, local_path, thumb_path, etag FROM entry_image WHERE local_path IS NOT NULL")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    assert_eq!(rows.len(), 4);
    for (id, local, thumb, etag) in &rows {
        let local = local.as_deref().unwrap();
        let thumb = thumb.as_deref().unwrap();
        assert!(std::path::Path::new(local).exists(), "{id} 展示图未落地");
        assert!(std::path::Path::new(thumb).exists(), "{id} 缩略图未生成");
        assert!(etag.as_deref().map(|e| e.len() == 32).unwrap_or(false), "{id} etag 未记录");
        eprintln!("  {id}: {} bytes, thumb {} bytes", std::fs::metadata(local)?.len(), std::fs::metadata(thumb)?.len());
    }
    Ok(())
}
