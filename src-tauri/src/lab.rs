//! 画图实验室（CONTRACT §9.4，v0.2.0）：出图、组装、清洗、评分全在插件里跑，这里只做三件事——
//! 1. **转发**：`LabRunRequest` 原样 POST 到插件 `/page/lab/run/preview|start`，`status / list / image / rate`
//!    按 §9.3 路由；传输层复用 `plugin::plugin_request`（JWT + `X-Library-Token`，401 重登一次）。
//!    连不上 / 超时 → `network`；非 2xx（含 409 运行中、400 组装失败）→ `plugin_rejected` 带正文前 500 字。
//! 2. **镜像**：`lab_run_status` 拿到 `done|error` 时把整份 `LabRun` 落进本地 `lab_run` 表（003 迁移）；
//!    `lab_run_list` 把本地行与插件 `run/list` 按 run_id 合并（本地优先，`created_at` 倒序，截到 limit），
//!    插件不可达 / 未配置时只回本地，不报错。
//! 3. **图片缓存**：`labruns/<run_id>/<idx>.png`（全图）/ `<idx>.thumb.png`（缩略图），本地已有就直接给相对数据目录的路径，
//!    否则 GET 插件 `run/image` 解 `data_base64` 落盘（.part → rename）。文件名固定 .png，内容以插件给的 mime 为准
//!    （插件 `page_thumb` 同款逻辑出的是 JPEG，浏览器按内容识别，前端 `convertFileSrc` 照常显示）。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::plugin::{join_err, plugin_conn, plugin_data, plugin_payload, plugin_request, rejected, truncate, PluginConn, PLUGIN_PAGE_PATH};
use crate::state::AppState;

/// 数据目录下的图片缓存子目录（`state::SUBDIRS` 首启会建）。
pub const LAB_DIR: &str = "labruns";
const LIST_LIMIT_DEFAULT: i64 = 50;
const LIST_LIMIT_MAX: i64 = 500;

fn lab_route(route: &str) -> String {
    format!("{PLUGIN_PAGE_PATH}/lab/{route}")
}

// ---------- 参数校验 ----------

/// 只校验契约写明范围的两个数（§9.1 `count: 1..8`、`artist_ratio: 0..1`）；角色 / 图包 / scope 由插件裁定（400 → `plugin_rejected`）。
pub fn validate_request(req: &LabRunRequest) -> AppResult<()> {
    if !(1..=8).contains(&req.count) {
        return Err(AppError::bad_request(format!("count 必须在 1..8，收到 {}", req.count)));
    }
    // 写成两个比较而不是 `!(0..=1).contains`，NaN 也会落进错误分支
    if !(req.artist_ratio >= 0.0 && req.artist_ratio <= 1.0) {
        return Err(AppError::bad_request(format!("artist_ratio 必须在 0..1，收到 {}", req.artist_ratio)));
    }
    Ok(())
}

/// run_id 会拼进本地路径 `labruns/<run_id>/`，只拦真正的路径隐患（空、`.`/`..`、以点开头、分隔符、NUL），不猜插件的 id 格式。
pub fn validate_run_id(run_id: &str) -> AppResult<()> {
    let bad = run_id.is_empty()
        || run_id.len() > 200
        || run_id.starts_with('.')
        || run_id.chars().any(|c| matches!(c, '/' | '\\' | '\0'));
    if bad {
        return Err(AppError::bad_request(format!("run_id 不合法: {run_id:?}")));
    }
    Ok(())
}

fn validate_idx(idx: i64) -> AppResult<()> {
    if idx < 0 {
        return Err(AppError::bad_request(format!("idx 不能为负: {idx}")));
    }
    Ok(())
}

/// `labruns/<run_id>/<idx>.png` 或 `<idx>.thumb.png`，相对数据目录、正斜杠（§9.4）。
pub fn image_rel_path(run_id: &str, idx: i64, full: bool) -> String {
    if full {
        format!("{LAB_DIR}/{run_id}/{idx}.png")
    } else {
        format!("{LAB_DIR}/{run_id}/{idx}.thumb.png")
    }
}

// ---------- 本地镜像（lab_run 表） ----------

/// 整份 `LabRun` 写进 `lab_run`（有则覆盖）。返回 status 是否与库里原来的不同（首次落盘也算不同），调用方据此只记一次 `app_event`。
pub fn upsert_run(conn: &Connection, run: &LabRun) -> AppResult<bool> {
    let prev: Option<String> = conn
        .query_row("SELECT status FROM lab_run WHERE run_id = ?1", params![run.run_id], |r| r.get(0))
        .optional()?;
    let json = serde_json::to_string(run)?;
    conn.execute(
        "INSERT INTO lab_run(run_id, mode, created_at, status, json) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(run_id) DO UPDATE SET mode = excluded.mode, created_at = excluded.created_at, status = excluded.status, json = excluded.json",
        params![run.run_id, run.mode.as_str(), run.created_at, run.status.as_str(), json],
    )?;
    Ok(prev.as_deref() != Some(run.status.as_str()))
}

pub fn local_run(conn: &Connection, run_id: &str) -> AppResult<Option<LabRun>> {
    let raw: Option<String> = conn.query_row("SELECT json FROM lab_run WHERE run_id = ?1", params![run_id], |r| r.get(0)).optional()?;
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(&s).map_err(|e| AppError::db(format!("lab_run.json 解析失败（{run_id}）: {e}")))?)),
        None => Ok(None),
    }
}

/// 本地行 → `LabRunSummary`，`created_at` 倒序、同秒按 run_id 倒序。解析不了的行记 warn 跳过，不拖垮整页。
pub fn local_summaries(conn: &Connection, limit: i64) -> AppResult<Vec<LabRunSummary>> {
    let mut stmt = conn.prepare("SELECT run_id, json FROM lab_run ORDER BY created_at DESC, run_id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (run_id, raw) = row?;
        match serde_json::from_str::<LabRun>(&raw) {
            Ok(run) => out.push(summary_of(&run)),
            Err(e) => tracing::warn!(run_id, "lab_run.json 解析失败，已跳过: {e}"),
        }
    }
    Ok(out)
}

/// 从整份 `LabRun` 推导 §9.1 `LabRunSummary`：`count` = 图片数（还没排到图时用 `request.count`）、`done` = `status=done` 的张数、
/// `text` = `request.text`、`pack_title` = 第一张带图包标题的、`best_score` = 各张 `score` 最大值。
pub fn summary_of(run: &LabRun) -> LabRunSummary {
    let count = if run.images.is_empty() { run.request.count } else { run.images.len() as i64 };
    let done = run.images.iter().filter(|i| i.status == LabImageStatus::Done).count() as i64;
    let pack_title = run.images.iter().find_map(|i| i.plan.pack_title.clone());
    let best_score = run.images.iter().filter_map(|i| i.score).filter(|s| !s.is_nan()).fold(None, |acc: Option<f64>, s| match acc {
        Some(m) if m >= s => Some(m),
        _ => Some(s),
    });
    LabRunSummary {
        run_id: run.run_id.clone(),
        mode: run.mode,
        status: run.status,
        created_at: run.created_at.clone(),
        count,
        done,
        text: run.request.text.clone(),
        pack_title,
        best_score,
    }
}

/// 按 run_id 合并：本地覆盖插件，`created_at` 倒序（同秒 run_id 倒序），截到 `limit`。
pub fn merge_summaries(local: Vec<LabRunSummary>, remote: Vec<LabRunSummary>, limit: usize) -> Vec<LabRunSummary> {
    let mut map: HashMap<String, LabRunSummary> = HashMap::with_capacity(local.len() + remote.len());
    for s in remote {
        map.insert(s.run_id.clone(), s);
    }
    for s in local {
        map.insert(s.run_id.clone(), s);
    }
    let mut out: Vec<LabRunSummary> = map.into_values().collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.run_id.cmp(&a.run_id)));
    out.truncate(limit);
    out
}

/// 评分写回本地镜像：按 `idx` 找那张图（找不到再按位置兜底），行不存在 / 图不存在返回 false（不算错：还没到终态就没有本地行）。
pub fn rate_local(conn: &Connection, run_id: &str, idx: i64, rating: Option<LabRating>) -> AppResult<bool> {
    let Some(mut run) = local_run(conn, run_id)? else { return Ok(false) };
    let pos = run
        .images
        .iter()
        .position(|i| i.plan.idx == idx)
        .or_else(|| usize::try_from(idx).ok().filter(|&p| p < run.images.len()));
    let Some(pos) = pos else { return Ok(false) };
    run.images[pos].rating = rating;
    upsert_run(conn, &run)?;
    Ok(true)
}

/// 先写 .part 再 rename，半个文件不会被当成缓存命中（反推重画图 `reverse::render` 也用它）。
pub(crate) fn write_file_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!("{}.part", path.file_name().and_then(|n| n.to_str()).unwrap_or("image")));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------- 插件调用 ----------

fn shape_err(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::plugin_rejected(format!("插件返回的 {what} 形状不对: {e}"))
}

/// 一次实验室路由调用：非 2xx → `plugin_rejected`；正文按 JSON 解析（原样返回，`data` 拆包交给调用方）。
async fn call(
    state: &AppState,
    conn: &PluginConn,
    method: reqwest::Method,
    route: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
    timeout: Duration,
) -> AppResult<Value> {
    let (status, text) = plugin_request(&state.http, conn, method, &lab_route(route), query, body, timeout).await?;
    if !status.is_success() {
        return Err(rejected(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| AppError::plugin_rejected(format!("响应不是 JSON: {e}: {}", truncate(&text, 200))))
}

/// §9.3 `POST /page/lab/run/preview` → `{ plans }`。LLM 路径会真的调 LLM，超时给足 180 s。
pub async fn preview(state: &AppState, req: LabRunRequest) -> AppResult<Vec<LabPlan>> {
    validate_request(&req)?;
    let conn = plugin_conn(state).await?;
    let body = serde_json::to_value(&req)?;
    let parsed = call(state, &conn, reqwest::Method::POST, "run/preview", &[], Some(&body), Duration::from_secs(180)).await?;
    let plans = plugin_data(&parsed).get("plans").cloned().unwrap_or(Value::Null);
    serde_json::from_value::<Vec<LabPlan>>(plans).map_err(|e| shape_err("plans", e))
}

/// §9.3 `POST /page/lab/run/start` → `{ run_id, plans }`；插件运行中 409 → `plugin_rejected`。写一条 `app_event(lab)`。
pub async fn start(state: &AppState, req: LabRunRequest) -> AppResult<LabRunStart> {
    validate_request(&req)?;
    let conn = plugin_conn(state).await?;
    let body = serde_json::to_value(&req)?;
    let parsed = call(state, &conn, reqwest::Method::POST, "run/start", &[], Some(&body), Duration::from_secs(180)).await?;
    let started: LabRunStart = serde_json::from_value(plugin_data(&parsed).clone()).map_err(|e| shape_err("run/start", e))?;
    state.db.log_event(
        "info",
        "lab",
        "实验室任务已提交",
        json!({ "run_id": started.run_id, "mode": req.mode.as_str(), "count": req.count, "text": req.text, "entry_id": req.entry_id, "plans": started.plans.len() }),
    );
    Ok(started)
}

/// §9.3 `GET /page/lab/run/status?run_id=` → `LabRun`；`done|error` 时整份落进 `lab_run`（§9.4），状态首次到终态记一条 `app_event`。
pub async fn status(state: &AppState, run_id: String) -> AppResult<LabRun> {
    validate_run_id(&run_id)?;
    let conn = plugin_conn(state).await?;
    let parsed = call(state, &conn, reqwest::Method::GET, "run/status", &[("run_id", run_id.clone())], None, Duration::from_secs(30)).await?;
    let run: LabRun = serde_json::from_value(plugin_data(&parsed).clone()).map_err(|e| shape_err("LabRun", e))?;
    if run.status.is_terminal() {
        let db = state.db.clone();
        let snapshot = run.clone();
        let changed = tokio::task::spawn_blocking(move || db.with(|c| upsert_run(c, &snapshot))).await.map_err(join_err)??;
        if changed {
            let summary = summary_of(&run);
            let (level, msg) = match run.status {
                LabRunStatus::Error => ("warn", "实验室任务失败"),
                _ => ("info", "实验室任务完成"),
            };
            state.db.log_event(level, "lab", msg, json!({ "run_id": run.run_id, "mode": run.mode.as_str(), "count": summary.count, "done": summary.done, "best_score": summary.best_score, "error": run.error }));
        }
    }
    Ok(run)
}

/// §9.4 `lab_run_list`：本地 `lab_run` ∪ 插件 `GET /page/lab/run/list?limit=`，本地优先、`created_at` 倒序、截到 limit。
/// 插件地址 / 密码未配置、连不上、超时 → 只回本地（不报错）；插件明确拒绝（401/403 等非 2xx）仍抛 `plugin_rejected`。
pub async fn list(state: &AppState, limit: i64) -> AppResult<Vec<LabRunSummary>> {
    let limit = if limit <= 0 { LIST_LIMIT_DEFAULT } else { limit.min(LIST_LIMIT_MAX) };
    let db = state.db.clone();
    let local = tokio::task::spawn_blocking(move || db.with(|c| local_summaries(c, limit))).await.map_err(join_err)??;

    let conn = match plugin_conn(state).await {
        Ok(c) => c,
        Err(e) if e.code == "bad_request" || e.code == "secret" => {
            tracing::warn!("实验室列表只用本地记录（插件未配置）: {}", e.message);
            return Ok(merge_summaries(local, Vec::new(), limit as usize));
        }
        Err(e) => return Err(e),
    };
    let remote = match call(state, &conn, reqwest::Method::GET, "run/list", &[("limit", limit.to_string())], None, Duration::from_secs(30)).await {
        Ok(parsed) => serde_json::from_value::<Vec<LabRunSummary>>(plugin_payload(&parsed).clone()).map_err(|e| shape_err("LabRunSummary[]", e))?,
        Err(e) if e.code == "network" => {
            tracing::warn!("实验室列表只用本地记录（插件不可达）: {}", e.message);
            Vec::new()
        }
        Err(e) => return Err(e),
    };
    Ok(merge_summaries(local, remote, limit as usize))
}

/// §9.4 `lab_run_image`：本地 `labruns/<run_id>/<idx>[.thumb].png` 有就直接返回相对路径；
/// 否则 GET `/page/lab/run/image?run_id=&idx=&full=` 解 `data_base64` 落盘再返回。
pub async fn image(state: &AppState, run_id: String, idx: i64, full: bool) -> AppResult<String> {
    validate_run_id(&run_id)?;
    validate_idx(idx)?;
    let rel = image_rel_path(&run_id, idx, full);
    let abs = state.data_dir.join(&rel);
    if tokio::fs::metadata(&abs).await.map(|m| m.is_file() && m.len() > 0).unwrap_or(false) {
        return Ok(rel);
    }
    let conn = plugin_conn(state).await?;
    let query = [("run_id", run_id.clone()), ("idx", idx.to_string()), ("full", if full { "1" } else { "0" }.to_string())];
    let parsed = call(state, &conn, reqwest::Method::GET, "run/image", &query, None, Duration::from_secs(60)).await?;
    let data = plugin_data(&parsed);
    let b64 = data
        .get("data_base64")
        .and_then(Value::as_str)
        .ok_or_else(|| shape_err("run/image", "没有 data_base64"))?;
    // 容忍 data URL 形式（`data:image/jpeg;base64,...`），只取逗号后的正文
    let b64 = if b64.starts_with("data:") { b64.rsplit(',').next().unwrap_or("") } else { b64 };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| shape_err("run/image", format!("data_base64 解码失败: {e}")))?;
    if bytes.is_empty() {
        return Err(shape_err("run/image", "图片为空"));
    }
    let mime = data.get("mime").and_then(|m| m.as_str()).unwrap_or("").to_string();
    tracing::debug!(run_id, idx, full, mime, bytes = bytes.len(), "lab image cached");
    tokio::task::spawn_blocking(move || write_file_atomic(&abs, &bytes)).await.map_err(join_err)??;
    Ok(rel)
}

/// §9.4 `lab_run_rate`：先 POST `/page/lab/run/rate`，插件接受后再改本地镜像（本地没有这条也算成功）。
pub async fn rate(state: &AppState, run_id: String, idx: i64, rating: Option<LabRating>) -> AppResult<()> {
    validate_run_id(&run_id)?;
    validate_idx(idx)?;
    let conn = plugin_conn(state).await?;
    let body = json!({ "run_id": run_id, "idx": idx, "rating": rating });
    let parsed = call(state, &conn, reqwest::Method::POST, "run/rate", &[], Some(&body), Duration::from_secs(30)).await?;
    if plugin_data(&parsed).get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(AppError::plugin_rejected(format!("插件未接受评分: {}", truncate(&parsed.to_string(), 500))));
    }
    let db = state.db.clone();
    let rid = run_id.clone();
    let updated = tokio::task::spawn_blocking(move || db.with(|c| rate_local(c, &rid, idx, rating))).await.map_err(join_err)??;
    tracing::debug!(run_id, idx, ?rating, updated, "lab rating saved");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// 按 CONTRACT §9.1 逐字段手写的插件输出：一次画图运行，两张图，第一张成功带评分与收藏，第二张出图失败。
    /// 数字按插件的自然类型写（点数百分比是整数、评分是浮点），字段名与顺序照契约。
    const RUN_FIXTURE: &str = r#"{
  "run_id": "lab_draw_20260907-120000_a1b2",
  "mode": "draw",
  "request": {
    "mode": "draw",
    "text": "薇拉 海边日落",
    "entry_id": null,
    "scope": [["图包", "示例甲"], ["图包", "示例乙", "日常"]],
    "artist_ratio": 0.5,
    "count": 2,
    "score": true,
    "seed": 123456
  },
  "status": "done",
  "created_at": "2026-09-07T12:00:00Z",
  "finished_at": "2026-09-07T12:01:30Z",
  "stamina_before": 64,
  "stamina_after": 62,
  "images": [
    {
      "idx": 0,
      "character": "薇拉",
      "pack_entry_id": "nai5_community_pack:sample-0012",
      "pack_title": "海边写真",
      "pack_path": ["图包", "示例甲", "海边"],
      "tags_raw": "1girl, artist:foo, beach, sunset, umbrella, rating:general",
      "tags_used": "beach, sunset, umbrella",
      "dropped": ["artist:foo", "rating:general"],
      "extra_tags": "1.4::golden hour::",
      "prompt": "vera (character), 1girl, beach, sunset, umbrella, 1.4::golden hour::, amazing quality, very aesthetic",
      "negative": "lowres, bad anatomy, worst quality",
      "artist": "artist:bar, 0.8::artist:baz::",
      "artist_source": "pool",
      "size": "832x1216",
      "seed": 123456,
      "model": "nai-diffusion-4-5-full",
      "prose": false,
      "status": "done",
      "file": "lab_draw_20260907-120000_1.png",
      "error": null,
      "score": 8.5,
      "notes": ["构图完整，手部正常", "光影合理"],
      "rating": "fav"
    },
    {
      "idx": 1,
      "character": "薇拉",
      "pack_entry_id": "nai5_community_pack:sample-0300",
      "pack_title": "日常·午后",
      "pack_path": ["图包", "示例乙", "日常"],
      "tags_raw": "1girl, indoors, sitting, rating:general",
      "tags_used": "indoors, sitting",
      "dropped": ["rating:general"],
      "extra_tags": null,
      "prompt": "vera (character), 1girl, indoors, sitting, amazing quality, very aesthetic",
      "negative": "lowres, bad anatomy, worst quality",
      "artist": "",
      "artist_source": "none",
      "size": "832x1216",
      "seed": 123457,
      "model": "nai-diffusion-4-5-full",
      "prose": false,
      "status": "error",
      "file": null,
      "error": "NovelAI 500: internal error",
      "score": null,
      "notes": [],
      "rating": null
    }
  ],
  "error": null,
  "plugin_version": "1.2.0"
}"#;

    fn fixture_run() -> LabRun {
        serde_json::from_str(RUN_FIXTURE).expect("fixture 应按 §9.1 反序列化")
    }

    /// 数值容忍比较：`64` 与 `64.0` 视为同一个数（f64 字段序列化会带 `.0`），其余键 / 结构必须逐一相同。
    fn same_json(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q)),
            (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w))),
            _ => a == b,
        }
    }

    fn run_with(run_id: &str, created_at: &str, status: LabRunStatus) -> LabRun {
        let mut run = fixture_run();
        run.run_id = run_id.to_string();
        run.created_at = created_at.to_string();
        run.status = status;
        run
    }

    #[test]
    fn migration_003_creates_lab_run() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            let v: i64 = c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
            assert!(v >= 3);
            let cols: Vec<String> = c
                .prepare("SELECT name FROM pragma_table_info('lab_run') ORDER BY cid")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            assert_eq!(cols, ["run_id", "mode", "created_at", "status", "json"]);
            let pk: i64 = c.query_row("SELECT pk FROM pragma_table_info('lab_run') WHERE name = 'run_id'", [], |r| r.get(0))?;
            assert_eq!(pk, 1, "run_id 是主键");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn lab_run_fixture_round_trips_and_is_flat() {
        let run = fixture_run();
        assert_eq!(run.mode, LabMode::Draw);
        assert_eq!(run.status, LabRunStatus::Done);
        assert_eq!(run.request.scope, vec![vec!["图包".to_string(), "示例甲".to_string()], vec!["图包".to_string(), "示例乙".to_string(), "日常".to_string()]]);
        assert_eq!(run.stamina_before, Some(64.0));
        assert_eq!(run.images.len(), 2);
        assert_eq!(run.images[0].plan.artist_source, LabArtistSource::Pool);
        assert_eq!(run.images[0].rating, Some(LabRating::Fav));
        assert_eq!(run.images[0].score, Some(8.5));
        assert_eq!(run.images[1].status, LabImageStatus::Error);
        assert_eq!(run.images[1].plan.artist_source, LabArtistSource::None);
        assert_eq!(run.images[1].rating, None);
        assert_eq!(run.plugin_version, "1.2.0");

        // 序列化回去与手写 fixture 逐键相同（数值 64 == 64.0），键集合不多不少
        let back = serde_json::to_value(&run).unwrap();
        let expected: Value = serde_json::from_str(RUN_FIXTURE).unwrap();
        assert!(same_json(&back, &expected), "round-trip 不一致:\n{back:#}");

        // LabImage 是扁平的：LabPlan 的字段与自己的字段在同一层，没有 "plan" 键
        let img0 = &back["images"][0];
        assert!(img0.get("plan").is_none());
        for key in ["idx", "character", "pack_entry_id", "pack_title", "pack_path", "tags_raw", "tags_used", "dropped", "extra_tags", "prompt", "negative", "artist", "artist_source", "size", "seed", "model", "prose", "status", "file", "error", "score", "notes", "rating"] {
            assert!(img0.get(key).is_some(), "LabImage 缺 {key}");
        }
        assert_eq!(img0.as_object().unwrap().len(), 23, "LabPlan 17 字段（含 §11.3 prose）+ LabImage 6 字段");
        let top = back.as_object().unwrap();
        assert_eq!(top.len(), 11, "LabRun 11 字段");
        assert_eq!(back["request"].as_object().unwrap().len(), 8, "LabRunRequest 8 字段");

        // 缺键容忍：可空 / 数组字段缺失按 null / 空，必填字段缺失整体报错
        let mut lenient: Value = serde_json::from_str(RUN_FIXTURE).unwrap();
        lenient["images"][1].as_object_mut().unwrap().remove("notes");
        lenient["images"][1].as_object_mut().unwrap().remove("rating");
        lenient.as_object_mut().unwrap().remove("finished_at");
        let run2: LabRun = serde_json::from_value(lenient).unwrap();
        assert!(run2.images[1].notes.is_empty() && run2.images[1].rating.is_none() && run2.finished_at.is_none());
        let mut strict: Value = serde_json::from_str(RUN_FIXTURE).unwrap();
        strict["images"][0].as_object_mut().unwrap().remove("prompt");
        assert!(serde_json::from_value::<LabRun>(strict).is_err(), "prompt 是必填");
        let mut bad_enum: Value = serde_json::from_str(RUN_FIXTURE).unwrap();
        bad_enum["images"][0]["artist_source"] = json!("llm");
        assert!(serde_json::from_value::<LabRun>(bad_enum).is_err(), "artist_source 只有四个值");
    }

    #[test]
    fn request_and_summary_shapes() {
        // LabRunRequest：前端最小形状（可空键缺失）也能收；count / artist_ratio 越界拒绝
        let req: LabRunRequest = serde_json::from_value(json!({
            "mode": "draw", "text": "薇拉", "artist_ratio": 1, "count": 1, "score": false
        }))
        .unwrap();
        assert_eq!((req.entry_id.as_deref(), req.scope.len(), req.seed), (None, 0, None));
        assert!(validate_request(&req).is_ok());
        let mut bad = req.clone();
        bad.count = 9;
        assert_eq!(validate_request(&bad).unwrap_err().code, "bad_request");
        bad.count = 0;
        assert!(validate_request(&bad).is_err());
        let mut bad = req.clone();
        bad.artist_ratio = 1.5;
        assert!(validate_request(&bad).is_err());
        bad.artist_ratio = f64::NAN;
        assert!(validate_request(&bad).is_err());
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 8, "转发给插件的请求带齐 8 个键");
        assert_eq!(v["mode"], "draw");

        // LabRunSummary 直接照 §9.1 收
        let s: LabRunSummary = serde_json::from_value(json!({
            "run_id": "r1", "mode": "draw", "status": "running", "created_at": "2026-09-07T10:00:00Z",
            "count": 4, "done": 1, "text": "薇拉 咖啡馆", "pack_title": null, "best_score": null
        }))
        .unwrap();
        assert_eq!((s.count, s.done, s.best_score), (4, 1, None));

        // run_id / idx / 路径
        assert!(validate_run_id("lab_draw_20260907-120000_a1b2").is_ok());
        for bad in ["", ".", "..", ".hidden", "a/b", "a\\b", "x\0y"] {
            assert_eq!(validate_run_id(bad).unwrap_err().code, "bad_request", "{bad:?}");
        }
        assert_eq!(image_rel_path("r1", 0, true), "labruns/r1/0.png");
        assert_eq!(image_rel_path("r1", 3, false), "labruns/r1/3.thumb.png");
        assert!(validate_idx(-1).is_err());
    }

    #[test]
    fn summary_of_derives_from_run() {
        let run = fixture_run();
        let s = summary_of(&run);
        assert_eq!(s.run_id, run.run_id);
        assert_eq!(s.mode, LabMode::Draw);
        assert_eq!(s.status, LabRunStatus::Done);
        assert_eq!(s.created_at, "2026-09-07T12:00:00Z");
        assert_eq!((s.count, s.done), (2, 1));
        assert_eq!(s.text, "薇拉 海边日落");
        assert_eq!(s.pack_title.as_deref(), Some("海边写真"));
        assert_eq!(s.best_score, Some(8.5));

        // 还没排到图（插件在组装阶段就失败）：count 用 request.count，其余为空
        let mut early = run.clone();
        early.images.clear();
        early.status = LabRunStatus::Error;
        let s = summary_of(&early);
        assert_eq!((s.count, s.done, s.pack_title, s.best_score), (2, 0, None, None));
    }

    #[test]
    fn upsert_rate_and_local_list() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            let run = fixture_run();
            assert!(upsert_run(c, &run)?, "首次落盘算状态变化");
            assert!(!upsert_run(c, &run)?, "同状态再写不算变化");
            let (mode, status): (String, String) = c.query_row("SELECT mode, status FROM lab_run WHERE run_id = ?1", params![run.run_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            assert_eq!((mode.as_str(), status.as_str()), ("draw", "done"));
            assert_eq!(local_run(c, &run.run_id)?.unwrap(), run);
            assert!(local_run(c, "nope")?.is_none());

            // 评分写回：按 idx 找图；改完整份 json 仍是完整 LabRun
            assert!(rate_local(c, &run.run_id, 1, Some(LabRating::Dis))?);
            let after = local_run(c, &run.run_id)?.unwrap();
            assert_eq!(after.images[1].rating, Some(LabRating::Dis));
            assert_eq!(after.images[0].rating, Some(LabRating::Fav), "别的图不动");
            assert!(rate_local(c, &run.run_id, 0, None)?);
            assert_eq!(local_run(c, &run.run_id)?.unwrap().images[0].rating, None);
            assert!(!rate_local(c, &run.run_id, 7, Some(LabRating::Fav))?, "没这张图 → false");
            assert!(!rate_local(c, "nope", 0, Some(LabRating::Fav))?, "没这条 run → false");

            // 本地列表倒序、limit 生效
            upsert_run(c, &run_with("r-old", "2026-09-06T08:00:00Z", LabRunStatus::Error))?;
            upsert_run(c, &run_with("r-new", "2026-09-07T13:00:00Z", LabRunStatus::Done))?;
            let ids: Vec<String> = local_summaries(c, 10)?.into_iter().map(|s| s.run_id).collect();
            assert_eq!(ids, ["r-new", "lab_draw_20260907-120000_a1b2", "r-old"]);
            assert_eq!(local_summaries(c, 2)?.len(), 2);

            // 坏行跳过不拖垮整页
            c.execute("INSERT INTO lab_run(run_id, mode, created_at, status, json) VALUES ('broken', 'draw', '2026-09-08T00:00:00Z', 'done', '{not json')", [])?;
            let ids: Vec<String> = local_summaries(c, 10)?.into_iter().map(|s| s.run_id).collect();
            assert_eq!(ids.len(), 3);
            assert!(!ids.contains(&"broken".to_string()));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn merge_local_wins_sorted_desc_and_truncated() {
        let local = vec![
            summary_of(&run_with("r-b", "2026-09-07T12:00:00Z", LabRunStatus::Done)),
            summary_of(&run_with("r-a", "2026-09-06T09:00:00Z", LabRunStatus::Error)),
        ];
        let remote_b = LabRunSummary { run_id: "r-b".into(), mode: LabMode::Draw, status: LabRunStatus::Running, created_at: "2026-09-07T12:00:00Z".into(), count: 2, done: 0, text: "插件视角".into(), pack_title: None, best_score: None };
        let remote_c = LabRunSummary { run_id: "r-c".into(), mode: LabMode::Draw, status: LabRunStatus::Running, created_at: "2026-09-07T14:00:00Z".into(), count: 1, done: 0, text: "只在插件".into(), pack_title: None, best_score: None };
        let remote_d = LabRunSummary { run_id: "r-d".into(), mode: LabMode::Draw, status: LabRunStatus::Done, created_at: "2026-09-05T00:00:00Z".into(), count: 1, done: 1, text: "最旧".into(), pack_title: Some("x".into()), best_score: Some(7.0) };

        let merged = merge_summaries(local.clone(), vec![remote_b.clone(), remote_c.clone(), remote_d.clone()], 10);
        let ids: Vec<&str> = merged.iter().map(|s| s.run_id.as_str()).collect();
        assert_eq!(ids, ["r-c", "r-b", "r-a", "r-d"], "created_at 倒序，同 run_id 只出一条");
        let b = merged.iter().find(|s| s.run_id == "r-b").unwrap();
        assert_eq!(b.status, LabRunStatus::Done, "本地优先");
        assert_eq!(b.text, "薇拉 海边日落");
        assert_eq!(b.done, 1);

        assert_eq!(merge_summaries(local.clone(), vec![remote_c.clone(), remote_d.clone()], 2).len(), 2, "截到 limit");
        let only_local = merge_summaries(local, Vec::new(), 10);
        assert_eq!(only_local.iter().map(|s| s.run_id.as_str()).collect::<Vec<_>>(), ["r-b", "r-a"], "插件不可达时只有本地");
        // 同秒按 run_id 倒序，结果稳定
        let same = merge_summaries(Vec::new(), vec![remote_d.clone(), LabRunSummary { run_id: "r-e".into(), ..remote_d.clone() }], 10);
        assert_eq!(same.iter().map(|s| s.run_id.as_str()).collect::<Vec<_>>(), ["r-e", "r-d"]);
    }

    #[test]
    fn image_cache_write_is_atomic_and_nested() {
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join(image_rel_path("run-x", 2, false));
        write_file_atomic(&abs, b"\x89PNG fake").unwrap();
        assert_eq!(std::fs::read(&abs).unwrap(), b"\x89PNG fake");
        let leftovers: Vec<_> = std::fs::read_dir(abs.parent().unwrap()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".part")).collect();
        assert!(leftovers.is_empty(), "不留 .part");
        write_file_atomic(&abs, b"again").unwrap();
        assert_eq!(std::fs::read(&abs).unwrap(), b"again", "覆盖写");
    }
}
