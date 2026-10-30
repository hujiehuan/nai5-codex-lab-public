//! 建索引（CONTRACT §12.2）：对当前模型缺向量或 `image_sha256` 与 `entry_image.sha256` 不一致的展示图（ordinal=0、已下载、未下架）
//! 16 张一批解码 + 推理（`spawn_blocking`），每批落一次库；单张失败记 warn `app_event` 并继续；批与批之间看取消令牌；
//! 结束（含取消）重算均值缓存。模型未下载 → `model_missing`，由调用方发「画风模型未下载，跳过索引」事件。
//! 同步的 embedding 阶段（`sync::sync_inner`）与 `style_index_rebuild`（`run_rebuild`）都走 `build_index`。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::db::{now_iso, Db};
use crate::error::{AppError, AppResult};
use crate::models::{SyncEvent, SyncPhase};
use crate::state::SyncController;

use super::model::{self, Encoder};
use super::store::{self, PendingImage};
use super::StyleState;

/// §12.2：模型未下载时那条事件的 message（原文）。
pub const SKIP_MESSAGE: &str = "画风模型未下载，跳过索引";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexOutcome {
    pub model: String,
    pub total: u64,
    pub done: u64,
    pub failed: u64,
    pub model_missing: bool,
}

impl IndexOutcome {
    /// 拼进同步结束消息 / 重建的 Done 事件的一句话。
    pub fn summary(&self) -> String {
        if self.model_missing {
            SKIP_MESSAGE.to_string()
        } else if self.total == 0 {
            "画风索引已是最新".to_string()
        } else {
            format!("画风索引 {}/{}，失败 {}", self.done, self.total, self.failed)
        }
    }
}

/// 进度回调 `(total, done, failed)`。
pub type Progress<'a> = &'a (dyn Fn(u64, u64, u64) + Send + Sync);

/// 一批：逐张解码（解不开的单张记错），能解的合成一批推理；推理本身失败整批记错。纯 CPU，调用方放 `spawn_blocking`。
pub fn embed_jobs(enc: &Encoder, jobs: &[PendingImage]) -> Vec<(PendingImage, Result<Vec<f32>, String>)> {
    let mut out: Vec<(PendingImage, Result<Vec<f32>, String>)> = jobs.iter().map(|j| (j.clone(), Err("未计算".to_string()))).collect();
    let mut images = Vec::with_capacity(jobs.len());
    let mut positions = Vec::with_capacity(jobs.len());
    for (i, job) in jobs.iter().enumerate() {
        match image::open(&job.local_path) {
            Ok(img) => {
                images.push(img);
                positions.push(i);
            }
            Err(e) => out[i].1 = Err(format!("解码失败: {e}")),
        }
    }
    if images.is_empty() {
        return out;
    }
    match enc.embed_batch(&images) {
        Ok(vecs) => {
            for (pos, v) in positions.into_iter().zip(vecs) {
                out[pos].1 = Ok(v);
            }
        }
        Err(e) => {
            for pos in positions {
                out[pos].1 = Err(format!("推理失败: {}", e.message));
            }
        }
    }
    out
}

/// 建索引主流程。`force=true` 先删掉当前模型的全部向量。返回 Err 只有取消（`cancelled`）、加载模型失败、落库失败。
pub async fn build_index(db: &Arc<Db>, data_dir: &Path, style: &Arc<StyleState>, cancel: &CancellationToken, force: bool, on_progress: Progress<'_>) -> AppResult<IndexOutcome> {
    let model = db.with(|c| store::current_model(c))?;
    build_index_for_model(db, data_dir, style, cancel, force, &model, on_progress).await
}

pub async fn build_index_for_model(db: &Arc<Db>, data_dir: &Path, style: &Arc<StyleState>, cancel: &CancellationToken, force: bool, model_id: &str, on_progress: Progress<'_>) -> AppResult<IndexOutcome> {
    let _inference = tokio::select! {
        guard = style.inference_lock.lock() => guard,
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
    };
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    super::classifier::unload();
    let spec = super::catalog::get(model_id)?;
    let model = spec.id.to_string();
    let dir = model::model_dir(data_dir, &model);
    if !spec.present(&dir) {
        return Ok(IndexOutcome { model, model_missing: true, ..Default::default() });
    }
    // 002 之前下载的图先补 sha256（与完整池导出同一函数），向量行才有哈希可比
    let backfilled = crate::export::backfill_image_sha256(db)?;
    if backfilled > 0 {
        tracing::info!(backfilled, "entry_image.sha256 已补算（画风索引前）");
    }
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    if force {
        let deleted = db.with(|c| store::delete_model(c, &model))?;
        db.log_event("info", "style", "已清空画风索引，全量重算", json!({ "model": model, "deleted": deleted }));
    }
    let jobs = db.with(|c| store::pending(c, &model))?;
    let total = jobs.len() as u64;
    let mut outcome = IndexOutcome { model: model.clone(), total, ..Default::default() };
    on_progress(total, 0, 0);
    if jobs.is_empty() {
        db.with(|c| store::recompute_mean(c, &model))?;
        return Ok(outcome);
    }
    let preference = crate::state::load_settings(db)?.style_device;
    if cancel.is_cancelled() { return Err(AppError::cancelled()); }
    let encoder = style.encoder(&dir, preference).await?;
    db.log_event("info", "style", "画风索引使用的计算设备", json!({
        "preference": preference, "device": encoder.device_kind(), "fallback": encoder.fallback_reason()
    }));
    for chunk in jobs.chunks(spec.batch_size) {
        if cancel.is_cancelled() {
            let _ = db.with(|c| store::recompute_mean(c, &model));
            return Err(AppError::cancelled());
        }
        let enc = encoder.clone();
        let batch = chunk.to_vec();
        let results = tokio::task::spawn_blocking(move || embed_jobs(&enc, &batch)).await.map_err(|e| AppError::new("internal", e.to_string()))?;
        let now = now_iso();
        db.with(|c| {
            let tx = c.transaction()?;
            for (job, r) in &results {
                if let Ok(v) = r {
                    store::upsert_vector(&tx, &job.entry_id, job.ordinal, &model, v, job.sha256.as_deref(), &now)?;
                }
            }
            tx.commit()?;
            Ok(())
        })?;
        for (job, r) in &results {
            match r {
                Ok(_) => outcome.done += 1,
                Err(e) => {
                    outcome.failed += 1;
                    db.log_event(
                        "warn",
                        "style",
                        "画风向量计算失败",
                        json!({ "entry_id": job.entry_id, "ordinal": job.ordinal, "path": job.local_path, "error": e }),
                    );
                }
            }
        }
        on_progress(total, outcome.done, outcome.failed);
    }
    db.with(|c| store::recompute_mean(c, &model))?;
    Ok(outcome)
}

/// `style_index_rebuild` 的后台任务：占用 `SyncController`（与同步互斥，`sync_cancel` 可取消），事件只有 `embedding` 相与 `done` / `error`。
pub async fn run_rebuild(
    db: Arc<Db>,
    data_dir: PathBuf,
    style: Arc<StyleState>,
    controller: Arc<SyncController>,
    cancel: CancellationToken,
    force: bool,
    model: String,
    emit: Arc<dyn Fn(SyncEvent) + Send + Sync>,
) {
    let event = |phase: SyncPhase, message: String, (t, d, f): (u64, u64, u64)| SyncEvent {
        phase,
        message,
        release: None,
        added: 0,
        changed: 0,
        images_total: 0,
        images_done: 0,
        images_failed: 0,
        embed_total: t,
        embed_done: d,
        embed_failed: f,
    };
    let send = |ev: SyncEvent| {
        controller.update(|s| {
            s.phase = ev.phase;
            s.message = ev.message.clone();
        });
        emit(ev);
    };
    db.log_event("info", "style", if force { "重建画风索引（全量）" } else { "重建画风索引" }, json!({ "force": force }));
    send(event(SyncPhase::Embedding, "准备画风索引".into(), (0, 0, 0)));
    let counters = parking_lot::Mutex::new((0u64, 0u64, 0u64));
    let on_progress = |t: u64, d: u64, f: u64| {
        *counters.lock() = (t, d, f);
        send(event(SyncPhase::Embedding, format!("画风向量 {d}/{t}"), (t, d, f)));
    };
    let result = build_index_for_model(&db, &data_dir, &style, &cancel, force, &model, &on_progress).await;
    let counts = *counters.lock();
    match result {
        Ok(o) => {
            let level = if o.failed > 0 { "warn" } else { "info" };
            db.log_event(level, "style", &format!("画风索引结束：{}", o.summary()), json!({ "model": o.model, "total": o.total, "done": o.done, "failed": o.failed, "model_missing": o.model_missing }));
            send(event(SyncPhase::Done, o.summary(), (o.total, o.done, o.failed)));
        }
        Err(e) if e.code == "cancelled" => {
            db.log_event("info", "style", "画风索引已取消", json!({ "done": counts.1, "total": counts.0 }));
            send(event(SyncPhase::Done, "已取消".into(), counts));
        }
        Err(e) => {
            db.log_event("error", "style", &format!("画风索引失败: {}", e.message), json!({}));
            send(event(SyncPhase::Error, e.message, counts));
        }
    }
    controller.finish();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::Decision;
    use candle_transformers::models::clip::vision_model::ClipVisionConfig;
    use std::sync::Mutex;

    fn seed(db: &Db, dir: &Path) {
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO codex_release(release_id, published_at, content_hash, fetched_at, raw_dir) VALUES ('r-1','p','h','f','d');
                 INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('artist_nai5_personal','artist','画师','1','[]');
                 INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('nai5_community_pack','pack','图包','1','[]');",
            )?;
            for (id, book, order, tags, file) in [
                ("a1", "artist_nai5_personal", 0, "artist:foo", "a1.png"),
                ("a2", "artist_nai5_personal", 1, "artist:bar", "a2.png"),
                ("p1", "nai5_community_pack", 0, "1girl, smile", "p1.png"),
                ("bad", "artist_nai5_personal", 2, "artist:bad", "bad.png"),
            ] {
                c.execute(
                    "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release) VALUES (?1,?2,?3,?1,'[]',?4,'safe','r-1','r-1')",
                    rusqlite::params![id, book, order, tags],
                )?;
                let p = dir.join(file);
                if id == "bad" {
                    std::fs::write(&p, b"not an image").unwrap();
                } else {
                    let seed = order as u32 * 40 + if book == "artist_nai5_personal" { 0 } else { 100 };
                    image::RgbImage::from_fn(48, 40, |x, y| image::Rgb([((x + seed) % 256) as u8, ((y * 3 + seed) % 256) as u8, (seed % 256) as u8])).save(&p).unwrap();
                }
                c.execute(
                    "INSERT INTO entry_image(entry_id, ordinal, image_file, local_path) VALUES (?1, 0, ?2, ?3)",
                    rusqlite::params![id, file, p.to_string_lossy().to_string()],
                )?;
            }
            // 未下载的展示图与已下架的条目：不在待算里
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release) VALUES ('nd','nai5_community_pack',2,'nd','[]','x','safe','r-1','r-1')",
                [],
            )?;
            c.execute("INSERT INTO entry_image(entry_id, ordinal, image_file) VALUES ('nd', 0, 'nd.png')", [])?;
            Ok(())
        })
        .unwrap();
        let _ = Decision::Include;
    }

    /// 假模型目录（只让 `is_present` 为真）；真正的编码器由测试塞进 `StyleState` 缓存（随机权重、2 层小模型）。
    fn fake_model_dir(data_dir: &Path) -> PathBuf {
        let mdir = model::model_dir(data_dir, model::MODEL_NAME);
        std::fs::create_dir_all(&mdir).unwrap();
        std::fs::write(mdir.join("config.json"), b"{}").unwrap();
        std::fs::write(mdir.join("model.safetensors"), b"fake").unwrap();
        mdir
    }

    fn small_encoder() -> Encoder {
        let cfg = ClipVisionConfig { num_hidden_layers: 2, projection_dim: 16, ..ClipVisionConfig::vit_base_patch32() };
        Encoder::random(cfg).unwrap()
    }

    #[tokio::test]
    async fn cancelling_queued_force_rebuild_preserves_existing_vectors() {
        let db = Arc::new(Db::open_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        seed(&db, dir.path());
        db.with(|c| store::upsert_now(c, "a1", 0, model::MODEL_NAME, &[1.0, 0.0], None)).unwrap();
        fake_model_dir(dir.path());
        let style = Arc::new(StyleState::new());
        let _busy = style.inference_lock.lock().await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = tokio::time::timeout(std::time::Duration::from_secs(1), build_index_for_model(&db, dir.path(), &style, &cancel, true, model::MODEL_NAME, &|_, _, _| {})).await.unwrap().unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert_eq!(db.with(|c| store::get_vector(c, "a1", 0, model::MODEL_NAME)).unwrap(), Some(vec![1.0, 0.0]));
    }

    /// §12.2：模型未下载 → model_missing、不动库；有模型 → 三张能解的图入库（sha256 先补算并写回）、坏图记 warn 事件、
    /// 进度回调 total/done/failed、均值缓存就绪；再跑一次 total=0；force 清空重算；坏图每次都重试并再记一次 warn。
    #[tokio::test]
    async fn build_index_embeds_pending_images_and_reports_failures() {
        let db = Arc::new(Db::open_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        seed(&db, dir.path());
        let style = Arc::new(StyleState::new());
        let cancel = CancellationToken::new();
        let progress: Mutex<Vec<(u64, u64, u64)>> = Mutex::new(Vec::new());
        let on_progress = |t, d, f| progress.lock().unwrap().push((t, d, f));

        let o = build_index(&db, &data_dir, &style, &cancel, false, &on_progress).await.unwrap();
        assert!(o.model_missing);
        assert_eq!(o.summary(), SKIP_MESSAGE);
        assert!(progress.lock().unwrap().is_empty());
        assert_eq!(db.with(|c| store::count_indexed(c, model::MODEL_NAME)).unwrap(), 0);

        let mdir = fake_model_dir(&data_dir);
        let enc = Arc::new(small_encoder());
        *style.encoder.lock() = Some((mdir.clone(), crate::models::StyleDevicePreference::Auto, enc.clone()));
        assert!(style.cached(&mdir, crate::models::StyleDevicePreference::Auto).is_some());
        assert!(style.cached(&mdir, crate::models::StyleDevicePreference::Cpu).is_none(), "切换偏好不能复用旧设备缓存");
        assert!(style.cached(dir.path(), crate::models::StyleDevicePreference::Auto).is_none(), "模型目录也须匹配");
        let o = build_index(&db, &data_dir, &style, &cancel, false, &on_progress).await.unwrap();
        assert_eq!((o.total, o.done, o.failed, o.model_missing), (3, 2, 1, false));
        assert_eq!(o.summary(), "画风索引 2/3，失败 1");
        let seen = progress.lock().unwrap().clone();
        assert_eq!(seen.first(), Some(&(3, 0, 0)));
        assert_eq!(seen.last(), Some(&(3, 2, 1)));
        db.with(|c| {
            assert_eq!(store::count_indexed(c, model::MODEL_NAME)?, 2);
            assert_eq!(store::count_total(c)?, 3, "坏图也是「应索引」");
            let v = store::get_vector(c, "a1", 0, model::MODEL_NAME)?.unwrap();
            assert_eq!(v.len(), 16);
            let sha: Option<String> = c.query_row("SELECT sha256 FROM entry_image WHERE entry_id = 'a1' AND ordinal = 0", [], |r| r.get(0))?;
            assert!(sha.is_some(), "索引前补算了 entry_image.sha256");
            let stored: Option<String> = c.query_row("SELECT image_sha256 FROM image_embedding WHERE entry_id = 'a1' AND ordinal = 0", [], |r| r.get(0))?;
            assert_eq!(stored, sha, "向量行记的是当时的图片哈希");
            assert!(store::get_vector(c, "bad", 0, model::MODEL_NAME)?.is_none());
            assert!(store::get_vector(c, "p1", 0, model::MODEL_NAME)?.is_none(), "图包不建画风索引");
            assert!(store::cached_mean(c, model::MODEL_NAME)?.is_some(), "均值缓存就绪");
            let warns: i64 = c.query_row("SELECT COUNT(*) FROM app_event WHERE level = 'warn' AND area = 'style' AND message = '画风向量计算失败'", [], |r| r.get(0))?;
            assert_eq!(warns, 1);
            assert!(store::pending(c, model::MODEL_NAME)?.iter().all(|p| p.entry_id == "bad"), "只剩坏图待算");
            Ok(())
        })
        .unwrap();
        // 再跑：只有坏图会再试一次（total=1，仍失败），好图不重算
        let o = build_index(&db, &data_dir, &style, &cancel, false, &on_progress).await.unwrap();
        assert_eq!((o.total, o.done, o.failed), (1, 0, 1));
        // 图片换了（sha256 变）→ 只重算那一张
        db.with(|c| {
            c.execute("UPDATE entry_image SET sha256 = 'changed' WHERE entry_id = 'a2' AND ordinal = 0", [])?;
            Ok(())
        })
        .unwrap();
        let o = build_index(&db, &data_dir, &style, &cancel, false, &on_progress).await.unwrap();
        assert_eq!((o.total, o.done, o.failed), (2, 1, 1));
        // force：清空重算
        let o = build_index(&db, &data_dir, &style, &cancel, true, &on_progress).await.unwrap();
        assert_eq!((o.total, o.done, o.failed), (3, 2, 1));
        // 取消：批前就取消 → cancelled，库不动
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        db.with(|c| {
            c.execute("DELETE FROM image_embedding WHERE entry_id = 'a1'", [])?;
            Ok(())
        })
        .unwrap();
        let err = build_index(&db, &data_dir, &style, &cancelled, false, &on_progress).await.unwrap_err();
        assert_eq!(err.code, "cancelled");
        assert_eq!(db.with(|c| store::count_indexed(c, model::MODEL_NAME)).unwrap(), 1);
        // 说明汇总
        assert_eq!(IndexOutcome { total: 0, ..Default::default() }.summary(), "画风索引已是最新");

        // run_rebuild：事件序列 embedding… → done；controller 结束后 running=false
        let controller = Arc::new(SyncController::new());
        assert!(controller.try_start());
        let events: Arc<Mutex<Vec<SyncEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let ev2 = events.clone();
        let emit: Arc<dyn Fn(SyncEvent) + Send + Sync> = Arc::new(move |e| ev2.lock().unwrap().push(e));
        run_rebuild(db.clone(), data_dir.clone(), style.clone(), controller.clone(), CancellationToken::new(), false, model::MODEL_NAME.to_string(), emit).await;
        let evs = events.lock().unwrap().clone();
        assert_eq!(evs[0].phase, SyncPhase::Embedding);
        assert!(evs.iter().filter(|e| e.phase == SyncPhase::Embedding).count() >= 2);
        let last = evs.last().unwrap();
        assert_eq!(last.phase, SyncPhase::Done);
        assert_eq!((last.embed_total, last.embed_done, last.embed_failed), (2, 1, 1), "a1 补回 + 坏图");
        assert_eq!(last.message, "画风索引 1/2，失败 1");
        assert!(!controller.is_running());
        assert_eq!(controller.snapshot().phase, SyncPhase::Done);
    }
}
