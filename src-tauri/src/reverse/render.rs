//! 反推重画对照（CONTRACT §11.3 / §11.4，v0.2.4）：一次 run 解析成功后，把清洗后的标签交给插件按同一角色重画一张
//! （`POST {PLUGIN_PAGE_PATH}/lab/reverse/render`：不注入画师、`judge=true`、原图随 body 一起送去给成对评委，总超时 240 s），
//! 插件回 `{ file, seed, prompt, negative, size, latency_ms, quality_score, quality_notes, similarity_score, similarity_reason }`；
//! 桌面端再经 `GET {PLUGIN_PAGE_PATH}/thumb?name=<file>&full=1`（`page_thumb`：响应 `{ status, data: { b64 } }`，JPEG q90 原尺寸）
//! 把图拉到 `labruns/reverse/<run_id>.jpg`，结果写进 `reverse_run` 的 render_* 列（005 迁移）。任何一步失败只写 `render_error`，
//! 已写入的出图结果保留。批次流程（`batch::run`）与单条重画命令 `reverse_render_run`（`rerender`）走同一条 `render_and_store`。
//! v0.3.0（§12.5）：`RenderPlan.artist` = 重画时注入的画风配方，请求体多一个 `artist` 键，
//! 落库写 `reverse_run.render_artist`（006 迁移）；`reverse_render_run` 的 `artist` 参数以本次为准（null = 不注入）。
//! §12.6（重画与 插件 `/反推` 逐条对齐）：请求体再多 `original_width` / `original_height`（缩放前原图宽高，插件按它选尺寸）、
//! `artist_mode`（`pool | given | none`，缺省 pool = 与线上同一概率抽画风池）、`count`（张数，null = 插件默认，上限 4）；插件多回 `size_source`、
//! `artist`、`artist_source`、`images[]`（全部候选的 file / seed / 分数）、`best_idx`、`plugin_dropped`、`plugin_would_reject`，落进 006 迁移的六列；
//! 最优那张仍拉到 `labruns/reverse/<run_id>.jpg`，全部候选另拉到 `<run_id>_<idx>.jpg`（最多 4 张，失败只记日志、`local_path` 留空）。
//! v0.3.1（§12.7 精准反推）：`artist_mode="style"` 时先 `resolve_style`——用画风索引检索题目图片（词典题 entry/ordinal、本地题 path），
//! 取第一条完整配方可用的画风词典命中（原始 `style_prompt`），分数低于 `style_min_score`
//! 或无命中 → 按 `style_fallback` 退到 pool / none；否则翻译成对插件的 `artist_mode=given` + `artist`（插件不改）。命中记进 007 迁移的
//! 五列 `render_style_*`，用到时 `render_artist_source="style"`。检索本身出错只写 `render_error`、不碰插件。

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::lab::write_file_atomic;
use crate::models::{
    default_render_artist_mode, default_style_fallback, default_style_top_k, default_true, Decision, RenderCandidate, RunBatchRequest, RunRow, StyleHit,
    StyleSearchRequest, StyleSearchSource, RENDER_ARTIST_MODES, RENDER_ARTIST_MODE_GIVEN, RENDER_ARTIST_MODE_POOL, RENDER_ARTIST_MODE_STYLE, RENDER_COUNT_MAX,
    STYLE_FALLBACKS, STYLE_FALLBACK_POOL,
};
use crate::plugin::{join_err, plugin_conn, plugin_data, rejected, truncate, PluginSession, PLUGIN_PAGE_PATH};
use crate::state::AppState;
use crate::style::{search as style_search, StyleState};
use crate::sync::upstream::library_of;

use super::client::{self, PreparedImage};
use super::store;

/// 插件端单张同步生成 + 两个评委的总超时（§11.3「总超时 240 s」）。
pub const RENDER_TIMEOUT_SECS: u64 = 240;
/// 取图超时：一张 JPEG 的 base64，与画图实验室取图一致取 60 s。
pub const THUMB_TIMEOUT_SECS: u64 = 60;
/// `character` 缺省（§11.3）。
pub const DEFAULT_CHARACTER: &str = "薇拉";
/// 重画图缓存目录（数据目录下），文件 `<run_id>.jpg`；§12.6 候选 `<run_id>_<idx>.jpg`。
pub const RENDER_DIR: &str = "labruns/reverse";
/// §12.6：候选图最多拉几张（= `RENDER_COUNT_MAX`，与线上 `reverse_draw.candidates` 上限相同）。
pub const MAX_CANDIDATES: usize = RENDER_COUNT_MAX as usize;

pub fn render_route() -> String {
    format!("{PLUGIN_PAGE_PATH}/lab/reverse/render")
}

pub fn thumb_route() -> String {
    format!("{PLUGIN_PAGE_PATH}/thumb")
}

/// `labruns/reverse/<run_id>.jpg`，相对数据目录；落库与给前端的 `render_path` 是拼上数据目录后的绝对路径（同 `thumb_path`，§0）。
pub fn render_rel_path(run_id: i64) -> String {
    format!("{RENDER_DIR}/{run_id}.jpg")
}

/// §12.6：第 idx 张候选 `labruns/reverse/<run_id>_<idx>.jpg`（相对数据目录）。
pub fn candidate_rel_path(run_id: i64, idx: usize) -> String {
    format!("{RENDER_DIR}/{run_id}_{idx}.jpg")
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// §12.7 精准反推的检索参数：`RunBatchRequest.style_*` / `reverse_render_run` 的同名参数，键名与请求字段逐字相同
/// （铺平进 `RenderPlan` 记到 `request_json.render`）。只在 `artist_mode=style` 时用得上。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleParams {
    /// 本次使用的向量模型；写入重画计划，避免切换全局模型后改变同一批次的含义。
    #[serde(default, rename = "style_model")]
    pub model: Option<String>,
    /// 检索候选数（1..50），取第一条可用的；缺省 5
    #[serde(default = "default_style_top_k")]
    pub style_top_k: i64,
    /// 决策范围（`include | exclude | undecided`），空 = 全部；黑名单命中的检索本来就不返回
    #[serde(default)]
    pub style_decisions: Vec<String>,
    /// 相似度门槛：命中分数低于它按 `style_fallback` 处理；None = 不设
    #[serde(default)]
    pub style_min_score: Option<f64>,
    /// 兜底 `pool | none`；缺省 pool
    #[serde(default = "default_style_fallback")]
    pub style_fallback: String,
    /// 检索时减库均值（§12.0）；缺省 true
    #[serde(default = "default_true")]
    pub style_center: bool,
}

impl Default for StyleParams {
    fn default() -> Self {
        StyleParams { model: None, style_top_k: default_style_top_k(), style_decisions: vec![], style_min_score: None, style_fallback: default_style_fallback(), style_center: true }
    }
}

impl StyleParams {
    /// 归一：`style_fallback` 去首尾空白小写（空 → pool）、`style_decisions` 去首尾空白小写并丢掉空项；数值原样（校验在 `validate_style_params`）。
    pub fn normalized(top_k: i64, decisions: &[String], min_score: Option<f64>, fallback: &str, center: bool) -> StyleParams {
        StyleParams {
            model: None,
            style_top_k: top_k,
            style_decisions: decisions.iter().map(|d| d.trim().to_ascii_lowercase()).filter(|d| !d.is_empty()).collect(),
            style_min_score: min_score,
            style_fallback: normalize_style_fallback(fallback),
            style_center: center,
        }
    }

    pub fn from_request(req: &RunBatchRequest) -> StyleParams {
        let mut params = Self::normalized(req.style_top_k, &req.style_decisions, req.style_min_score, &req.style_fallback, req.style_center);
        params.model = req.style_model.clone();
        params
    }

    /// `reverse_render_run` 的五个可选参数：null → 缺省（5 / 全部 / 不设门槛 / pool / true），老前端不传也能调。
    pub fn from_opts(top_k: Option<i64>, decisions: Option<Vec<String>>, min_score: Option<f64>, fallback: Option<String>, center: Option<bool>) -> StyleParams {
        Self::normalized(top_k.unwrap_or_else(default_style_top_k), &decisions.unwrap_or_default(), min_score, fallback.as_deref().unwrap_or(""), center.unwrap_or(true))
    }

    /// 决策范围 → `Decision`（已校验；不认识的跳过）。
    pub fn decisions(&self) -> Vec<Decision> {
        self.style_decisions.iter().filter_map(|d| Decision::parse(d)).collect()
    }

    /// 对应的 `style_search` 请求：画风词典全量、指定模型与决策范围照参数。
    pub fn search_request(&self, source: StyleSearchSource) -> StyleSearchRequest {
        StyleSearchRequest { source, model: self.model.clone(), top_k: self.style_top_k, codex_ids: vec![], decisions: self.decisions(), center: self.style_center }
    }
}

/// §12.7：`style_fallback` 归一——去首尾空白、小写；空 → `pool`。
pub fn normalize_style_fallback(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        STYLE_FALLBACK_POOL.to_string()
    } else {
        t
    }
}

/// §12.7 参数校验（只在 `artist_mode=style` 时调；批次 `prepare` 与单条 `rerender` 共用，都在碰插件之前）：`style_top_k` 1..=50、
/// `style_decisions` 只认三个值、`style_fallback` 只有 pool / none、`style_min_score` 是 -1..=1 的有限数或 null。
pub fn validate_style_params(p: &StyleParams) -> AppResult<()> {
    if let Some(id) = &p.model { crate::style::catalog::get(id)?; }
    if !(1..=style_search::MAX_TOP_K).contains(&p.style_top_k) {
        return Err(AppError::bad_request(format!("style_top_k 只能是 1..{}: {}", style_search::MAX_TOP_K, p.style_top_k)));
    }
    if let Some(bad) = p.style_decisions.iter().find(|d| Decision::parse(d).is_none()) {
        return Err(AppError::bad_request(format!("style_decisions 只能是 include / exclude / undecided: {bad}")));
    }
    if !STYLE_FALLBACKS.contains(&p.style_fallback.as_str()) {
        return Err(AppError::bad_request(format!("style_fallback 只能是 pool / none: {}", p.style_fallback)));
    }
    if let Some(m) = p.style_min_score {
        if !m.is_finite() || !(-1.0..=1.0).contains(&m) {
            return Err(AppError::bad_request(format!("style_min_score 只能是 -1..1 之间的数或 null: {m}")));
        }
    }
    Ok(())
}

/// 一个批次的重画参数（§11.4 `RunBatchRequest.render_character / render_seed / judge_provider_id`）；
/// 也写进每条 run 的 `request_json.render`，单条重画时从那里读回角色与固定种子。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderPlan {
    pub character: String,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub judge_provider_id: Option<String>,
    /// §12.5：注入的画师串；None = 不注入（§12.6 起只在 `artist_mode=given` 时用得上）。
    #[serde(default)]
    pub artist: Option<String>,
    /// §12.6：画师来源 `pool | given | none`（老记录缺键 → pool）。
    #[serde(default = "default_render_artist_mode")]
    pub artist_mode: String,
    /// §12.6：张数；None = 插件配置的默认。
    #[serde(default)]
    pub count: Option<i64>,
    /// §12.7：精准反推的检索参数（铺平成 `style_*` 五键；只在 `artist_mode=style` 时用；老记录缺键 → 缺省）。
    #[serde(flatten)]
    pub style: StyleParams,
}

impl RenderPlan {
    /// 缺省计划：薇拉、随机种子、插件默认评委、不给画师、`artist_mode=pool`、插件默认张数、§12.7 缺省检索参数。
    pub fn defaults() -> RenderPlan {
        RenderPlan {
            character: DEFAULT_CHARACTER.to_string(),
            seed: None,
            judge_provider_id: None,
            artist: None,
            artist_mode: default_render_artist_mode(),
            count: None,
            style: StyleParams::default(),
        }
    }

    /// `render=false` → None；角色留空 → 薇拉；`judge_provider_id` 空串当 null（插件默认评委）；
    /// §12.6 `render_artist_mode` 经 `normalize_artist_mode`（空 → pool），`render_count` 原样（校验在 `validate_render_params`）。
    pub fn from_request(req: &RunBatchRequest) -> Option<RenderPlan> {
        if !req.render {
            return None;
        }
        Some(RenderPlan {
            character: non_empty(req.render_character.as_deref()).unwrap_or_else(|| DEFAULT_CHARACTER.to_string()),
            seed: req.render_seed,
            judge_provider_id: non_empty(req.judge_provider_id.as_deref()),
            artist: non_empty(req.artist.as_deref()),
            artist_mode: normalize_artist_mode(&req.render_artist_mode),
            count: req.render_count,
            style: StyleParams::from_request(req),
        })
    }

    /// 从 `request_json.render` 读回（沿用批次当时的角色与固定种子）；没有 / 坏形状 → `defaults()`。
    pub fn from_request_log(request: &Value) -> RenderPlan {
        request
            .get("render")
            .and_then(|v| serde_json::from_value::<RenderPlan>(v.clone()).ok())
            .filter(|p| !p.character.trim().is_empty())
            .unwrap_or_else(RenderPlan::defaults)
    }
}

/// §12.6：`artist_mode` 归一——去首尾空白、小写；空 → `pool`。
pub fn normalize_artist_mode(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        RENDER_ARTIST_MODE_POOL.to_string()
    } else {
        t
    }
}

/// §12.6 参数校验（批次 `prepare` 与单条 `rerender` 共用，都在碰插件之前）：`artist_mode` 只有 pool / given / none / style（§12.7）；
/// `given` 必须带非空 `artist`（否则白耗点数画一张不知道用了什么画师的图）；`count` 是 1..=4 或 null（上限与线上 `reverse_draw.candidates` 相同）。
/// `style` 的检索参数另走 `validate_style_params`。
pub fn validate_render_params(artist_mode: &str, artist: Option<&str>, count: Option<i64>) -> AppResult<()> {
    let mode = normalize_artist_mode(artist_mode);
    if !RENDER_ARTIST_MODES.contains(&mode.as_str()) {
        return Err(AppError::bad_request(format!("render_artist_mode 只能是 pool / given / none / style: {}", artist_mode.trim())));
    }
    if mode == RENDER_ARTIST_MODE_GIVEN && non_empty(artist).is_none() {
        return Err(AppError::bad_request("render_artist_mode=given 需要非空的 artist"));
    }
    if let Some(n) = count {
        if !(1..=RENDER_COUNT_MAX).contains(&n) {
            return Err(AppError::bad_request(format!("render_count 只能是 1..{RENDER_COUNT_MAX} 或 null: {n}")));
        }
    }
    Ok(())
}

/// §11.3 请求体，键名照契约：`tags` = cleaned 用 ", " 连接；`framing` 空串当 null；`negative_extra` 恒 null；`judge` 恒 true；
/// 原图 = 题目图片经 `client::prepare_image` 的同一份字节（base64 + mime），没有原图时两个键为 null（插件只打质量分）。
/// §12.5 追加 `artist`（`artist_mode=given` 时插件用它）。§12.6 再追加 `original_width` / `original_height`（缩放前原图宽高；
/// 没原图或解码失败（宽高 0）时都为 null → 插件退到 `size_for_tags`）、`artist_mode`、`count`（null = 插件默认张数）。
pub fn build_render_body(cleaned: &[String], framing: Option<&str>, plan: &RenderPlan, original: Option<&PreparedImage>) -> Value {
    let dims = original.filter(|i| i.original_width > 0 && i.original_height > 0);
    json!({
        "tags": cleaned.join(", "),
        "framing": non_empty(framing),
        "character": plan.character,
        "seed": plan.seed,
        "negative_extra": Value::Null,
        "judge": true,
        "original_base64": original.map(client::image_base64),
        "original_mime": original.map(|i| i.mime.clone()),
        "judge_provider_id": plan.judge_provider_id,
        "artist": plan.artist,
        "original_width": dims.map(|i| i.original_width),
        "original_height": dims.map(|i| i.original_height),
        "artist_mode": plan.artist_mode,
        "count": plan.count,
    })
}

/// §11.3 响应（`data` 拆包后）。`file` 必需；其余缺键 / null 按空处理（评委失败时分数与理由就是 null）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderOutcome {
    pub file: String,
    pub seed: Option<i64>,
    pub prompt: String,
    pub negative: String,
    pub size: String,
    pub latency_ms: Option<i64>,
    pub quality_score: Option<f64>,
    pub quality_notes: Vec<String>,
    pub similarity_score: Option<f64>,
    pub similarity_reason: Option<String>,
    /// §12.6：尺寸来源（按原图宽高比 / 退到 `tags`）；老插件缺键 → None
    pub size_source: Option<String>,
    /// §12.6：插件实际注入的画师串（pool 抽中的 / given 回显）；空 → None
    pub artist: Option<String>,
    /// §12.6：`pool | given | none`；老插件缺键 → None（此时 `render_artist` 退回 Mac 送去的 `artist`）
    pub artist_source: Option<String>,
    /// §12.6：全部候选（顶层 file / seed / quality_* 是其中最优那张）；老插件缺键 → 空
    pub images: Vec<RenderCandidate>,
    /// §12.6：最优那张在 `images` 里的下标（先按 file 对上，其次信插件给的 `best_idx`）；对不上 → None
    pub best_idx: Option<usize>,
    /// §12.6：线上 `_clean_tags` 口径会多剔的 token
    pub plugin_dropped: Vec<String>,
    /// §12.6：线上口径下清洗后 < 5 条
    pub plugin_would_reject: bool,
}

fn shape_err(what: &str, raw: &Value) -> AppError {
    AppError::plugin_rejected(format!("render 响应{what}: {}", truncate(&raw.to_string(), 300)))
}

pub fn parse_render_response(raw: &Value) -> AppResult<RenderOutcome> {
    if let Some(e) = crate::plugin::plugin_error(raw) {
        return Err(e);
    }
    let d = plugin_data(raw);
    let file = d
        .get("file")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| shape_err("没有 file", raw))?
        .to_string();
    // 字符串字段：null / 缺键 → None；不是字符串（插件把 size 写成数组之类）→ 原样 JSON 文本，不炸
    let text = |k: &str| d.get(k).filter(|v| !v.is_null()).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
    let str_vec = |k: &str| str_vec_of(d.get(k));
    // §12.6：全部候选；最优下标先按 file 对（顶层 file 就是最优那张），对不上再信插件给的 best_idx（要在范围内）
    let images: Vec<RenderCandidate> = d.get("images").and_then(Value::as_array).map(|a| a.iter().filter_map(parse_candidate).collect()).unwrap_or_default();
    let best_idx = images
        .iter()
        .position(|c| c.file == file)
        .or_else(|| d.get("best_idx").and_then(Value::as_u64).map(|i| i as usize).filter(|i| *i < images.len()));
    Ok(RenderOutcome {
        file,
        seed: d.get("seed").and_then(Value::as_i64),
        prompt: text("prompt").unwrap_or_default(),
        negative: text("negative").unwrap_or_default(),
        size: text("size").unwrap_or_default(),
        latency_ms: d.get("latency_ms").and_then(Value::as_i64),
        quality_score: d.get("quality_score").and_then(Value::as_f64),
        quality_notes: str_vec("quality_notes"),
        similarity_score: d.get("similarity_score").and_then(Value::as_f64),
        similarity_reason: text("similarity_reason").filter(|s| !s.trim().is_empty()),
        size_source: text("size_source").filter(|s| !s.trim().is_empty()),
        artist: text("artist").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        artist_source: text("artist_source").filter(|s| !s.trim().is_empty()),
        images,
        best_idx,
        plugin_dropped: str_vec("plugin_dropped"),
        plugin_would_reject: d.get("plugin_would_reject").map(|v| v.as_bool().unwrap_or_else(|| v.as_i64().unwrap_or(0) != 0)).unwrap_or(false),
    })
}

/// 字符串数组字段：缺键 / 不是数组 → 空；非字符串的项跳过。
fn str_vec_of(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// §12.6 `images[]` 的一项：`file` 必需（没有的项跳过），其余缺键 / 非字符串按空；`local_path` 拉图后再填。
fn parse_candidate(v: &Value) -> Option<RenderCandidate> {
    let file = v.get("file").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())?.to_string();
    Some(RenderCandidate {
        file,
        seed: v.get("seed").and_then(Value::as_i64),
        quality_score: v.get("quality_score").and_then(Value::as_f64),
        quality_notes: str_vec_of(v.get("quality_notes")),
        local_path: None,
    })
}

/// `GET /page/thumb` 响应 `{ status, data: { b64 } }` → 字节；也容忍 `data_base64` 键、`data:...;base64,` 前缀与换行。
pub fn parse_thumb_response(raw: &Value) -> AppResult<Vec<u8>> {
    let d = plugin_data(raw);
    let b64 = d
        .get("b64")
        .or_else(|| d.get("data_base64"))
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::plugin_rejected(format!("thumb 响应没有 b64: {}", truncate(&raw.to_string(), 200))))?;
    let b64 = if b64.starts_with("data:") { b64.rsplit(',').next().unwrap_or("") } else { b64 };
    let compact: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|e| AppError::plugin_rejected(format!("thumb b64 解码失败: {e}")))?;
    if bytes.is_empty() {
        return Err(AppError::plugin_rejected("thumb 图片为空"));
    }
    Ok(bytes)
}

async fn call_render(http: &reqwest::Client, session: &PluginSession, body: &Value) -> AppResult<RenderOutcome> {
    let (status, text) = session
        .request(http, reqwest::Method::POST, &render_route(), &[], Some(body), Duration::from_secs(RENDER_TIMEOUT_SECS))
        .await?;
    if !status.is_success() {
        return Err(rejected(status, &text));
    }
    let raw: Value = serde_json::from_str(&text).map_err(|e| AppError::plugin_rejected(format!("render 响应不是 JSON: {e}: {}", truncate(&text, 200))))?;
    parse_render_response(&raw)
}

async fn fetch_full_image(http: &reqwest::Client, session: &PluginSession, file: &str) -> AppResult<Vec<u8>> {
    let query = [("name", file.to_string()), ("full", "1".to_string())];
    let (status, text) = session
        .request(http, reqwest::Method::GET, &thumb_route(), &query, None, Duration::from_secs(THUMB_TIMEOUT_SECS))
        .await?;
    if !status.is_success() {
        return Err(rejected(status, &text));
    }
    let raw: Value = serde_json::from_str(&text).map_err(|e| AppError::plugin_rejected(format!("thumb 响应不是 JSON: {e}: {}", truncate(&text, 200))))?;
    parse_thumb_response(&raw)
}

// ---------- §12.7 画风检索 ----------

/// §12.7 一次重画的画风检索结果。`entry_id` / `title` / `score` / `decision` 是用到的那条命中——相似度不够被兜底时也记最像的那条
/// （`fallback=true` 说明没用它）；无命中时四个都 None。`artist` = 实际送给插件的画师串（兜底时 None）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleChoice {
    pub entry_id: Option<String>,
    pub title: Option<String>,
    pub score: Option<f64>,
    pub decision: Option<String>,
    pub fallback: bool,
    pub artist: Option<String>,
}

impl StyleChoice {
    /// 用到了检索画风（有命中且没兜底）→ `render_artist_source="style"`、计入 `style_hit_n`。
    pub fn used(&self) -> bool {
        !self.fallback && self.artist.is_some()
    }
}

/// 一条命中可用的完整画风配方。只接受画风词典，正负权重、质量词和线条词保持原样。
/// `core` / `pack_tags` 是兼容元数据，不再作为重画配方。
pub fn style_artist_of(hit: &StyleHit) -> Option<String> {
    if library_of(&hit.codex_id) == "artist" {
        non_empty(Some(&hit.style_prompt))
    } else {
        None
    }
}

/// §12.7 选命中：`hits` 已按相似度降序、黑名单已被检索挡掉、决策范围已在检索时筛过；取第一条画师串可用的；它的分数低于 `min_score`
/// → 兜底（仍记下那条，`artist` 空）；一条都不可用 → 兜底、四个字段空。
pub fn pick_style_hit(hits: &[StyleHit], min_score: Option<f64>) -> StyleChoice {
    for h in hits {
        let Some(artist) = style_artist_of(h) else { continue };
        let below = min_score.map_or(false, |m| h.score < m);
        return StyleChoice {
            entry_id: Some(h.entry_id.clone()),
            title: Some(h.title.clone()),
            score: Some(h.score),
            decision: Some(h.decision.as_str().to_string()),
            fallback: below,
            artist: if below { None } else { Some(artist) },
        };
    }
    StyleChoice { fallback: true, ..StyleChoice::default() }
}

/// §12.7：跑一次画风检索并选出画师串。检索本身出错（模型未下载 / 索引空 / 题目图片读不到 / 库错）→ Err(文字)，调用方写
/// `render_error`、不碰插件——与「无命中」不同，无命中是正常兜底。
pub async fn resolve_style(db: &Arc<Db>, data_dir: &Path, style: &StyleState, source: StyleSearchSource, params: &StyleParams) -> Result<StyleChoice, String> {
    let result = style_search::search_with(db.clone(), data_dir, style, params.search_request(source)).await.map_err(|e| format!("画风检索失败: {}", e.message))?;
    Ok(pick_style_hit(&result.hits, params.style_min_score))
}

/// §12.7「插件不改」：把 `artist_mode=style` 的计划翻译成插件认识的形状（插件只认 pool / given / none）：有可用命中 → `given` + 命中串；
/// 兜底 → `artist_mode = style_fallback`（pool / none）、不带画师。其余字段（角色、种子、评委、张数、style_* 记录）原样。
pub fn plan_for_plugin(plan: &RenderPlan, choice: &StyleChoice) -> RenderPlan {
    let mut p = plan.clone();
    match choice.artist.clone() {
        Some(a) => {
            p.artist_mode = RENDER_ARTIST_MODE_GIVEN.to_string();
            p.artist = Some(a);
        }
        None => {
            p.artist_mode = plan.style.style_fallback.clone();
            p.artist = None;
        }
    }
    p
}

// ---------- 落库 ----------

/// 出图结果写进 render_* 列：`render_local` 先清空（取到图再填）、`render_error` 清空；`render_prompt` 空串存 NULL；
/// `render_artist` = 这次实际注入的画师串（§12.5 / §12.6）：插件回了 `artist_source`（新插件）就以它回传的 `artist` 为准
/// （pool 抽中的串、none 时为空），老插件没回则退回 Mac 送去的 `artist`；None 存 NULL。
/// §12.6 六列一并写：`render_candidates` 先存没有 `local_path` 的候选，拉完图由 `save_candidates` 覆盖。
/// §12.7 `style`：五列 `render_style_*`（非 style 模式传 None → 五列清空）；用到检索画风时 `render_artist_source` 记 `"style"`
/// （插件只知道自己收到的是 given），兜底时照插件回传的 pool / none。
pub fn save_outcome(conn: &Connection, run_id: i64, out: &RenderOutcome, artist: Option<&str>, style: Option<&StyleChoice>) -> AppResult<()> {
    let actual_artist = if out.artist_source.is_some() { out.artist.as_deref() } else { artist };
    let style_used = style.map_or(false, StyleChoice::used);
    let artist_source: Option<String> = if style_used { Some(RENDER_ARTIST_MODE_STYLE.to_string()) } else { non_empty(out.artist_source.as_deref()) };
    let n = conn.execute(
        "UPDATE reverse_run SET render_file = ?2, render_local = NULL, render_seed = ?3, render_prompt = ?4, quality_score = ?5, quality_notes = ?6,
           similarity_score = ?7, similarity_reason = ?8, render_error = NULL, render_artist = ?9,
           render_artist_source = ?10, render_size = ?11, render_size_source = ?12, plugin_dropped = ?13, plugin_would_reject = ?14, render_candidates = ?15,
           render_style_entry_id = ?16, render_style_title = ?17, render_style_score = ?18, render_style_decision = ?19, render_style_fallback = ?20
         WHERE run_id = ?1",
        params![
            run_id,
            out.file,
            out.seed,
            non_empty(Some(&out.prompt)),
            out.quality_score,
            serde_json::to_string(&out.quality_notes)?,
            out.similarity_score,
            out.similarity_reason,
            non_empty(actual_artist),
            artist_source,
            non_empty(Some(&out.size)),
            non_empty(out.size_source.as_deref()),
            serde_json::to_string(&out.plugin_dropped)?,
            out.plugin_would_reject as i64,
            serde_json::to_string(&out.images)?,
            style.and_then(|s| s.entry_id.clone()),
            style.and_then(|s| s.title.clone()),
            style.and_then(|s| s.score),
            style.and_then(|s| s.decision.clone()),
            style.map(|s| s.fallback as i64),
        ],
    )?;
    if n == 0 {
        return Err(AppError::not_found(format!("run 不存在: {run_id}")));
    }
    Ok(())
}

pub fn save_local(conn: &Connection, run_id: i64, local: &str) -> AppResult<()> {
    conn.execute("UPDATE reverse_run SET render_local = ?2, render_error = NULL WHERE run_id = ?1", params![run_id, local])?;
    Ok(())
}

/// §12.6：拉完候选图后把带 `local_path` 的候选列表写回（不动 `render_error`）。
pub fn save_candidates(conn: &Connection, run_id: i64, candidates: &[RenderCandidate]) -> AppResult<()> {
    conn.execute("UPDATE reverse_run SET render_candidates = ?2 WHERE run_id = ?1", params![run_id, serde_json::to_string(candidates)?])?;
    Ok(())
}

pub fn save_error(conn: &Connection, run_id: i64, err: &str) -> AppResult<()> {
    conn.execute("UPDATE reverse_run SET render_error = ?2 WHERE run_id = ?1", params![run_id, err])?;
    Ok(())
}

/// 只写 `render_error`（落库自身失败只记日志，不再往外抛）。
pub fn record_error(db: &Db, run_id: i64, err: &str) {
    if let Err(e) = db.with(|c| save_error(c, run_id, err)) {
        tracing::error!(run_id, "render_error 写入失败: {e}");
    }
}

/// 一次重画要用到的东西（批次流程与单条重画共用）。
pub struct RenderJob<'a> {
    pub run_id: i64,
    pub cleaned: &'a [String],
    pub framing: Option<&'a str>,
    pub plan: &'a RenderPlan,
    pub original: Option<&'a PreparedImage>,
    /// §12.7：题目图片的检索来源（词典题 entry/ordinal、本地题 path）；只在 `plan.artist_mode=style` 时用
    pub style_source: Option<StyleSearchSource>,
}

/// `render_and_store` 的结果：插件响应 + §12.7 的画风检索结果（非 style 模式为 None）。
#[derive(Debug, Clone)]
pub struct RenderDone {
    pub outcome: RenderOutcome,
    pub style: Option<StyleChoice>,
}

/// 完整一次重画：（§12.7 style 模式先检索画风）→ 调路由 → 写 render_* 列（含 §12.6 六列、§12.7 五列）→ 拉最优那张落盘
/// `labruns/reverse/<run_id>.jpg` → 写 `render_local` → 全部候选另拉到 `<run_id>_<idx>.jpg` 并回写 `render_candidates` 的 `local_path`。
/// 前几步任何一步失败只写 `render_error`（返回 Err 里的文字就是写进去的那句），已写入的出图结果保留；候选图失败只记日志；不抛 AppError。
pub async fn render_and_store(
    db: &Arc<Db>,
    http: &reqwest::Client,
    session: Option<&PluginSession>,
    data_dir: &Path,
    style: &StyleState,
    job: RenderJob<'_>,
) -> Result<RenderDone, String> {
    let run_id = job.run_id;
    let fail = |msg: String| -> Result<RenderDone, String> {
        record_error(db, run_id, &msg);
        db.log_event("warn", "reverse", "重画失败", json!({ "run_id": run_id, "error": msg.as_str() }));
        Err(msg)
    };
    if let Err(e) = db.with(|c| {
        c.execute("UPDATE reverse_run SET request_json=json_set(request_json, '$.last_render_request', json(?2)) WHERE run_id=?1", params![run_id, serde_json::to_string(job.plan)?])?;
        Ok(())
    }) { return fail(format!("记录本次重画参数失败：{}", e.message)); }
    let Some(session) = session else {
        return fail("插件连接未初始化（render=true 需要插件地址与密码）".to_string());
    };
    // §12.7：style 模式先检索，翻译成 given + 命中串，或按 style_fallback 退到 pool / none；检索出错只写 render_error，不碰插件
    let (plan, style_choice): (Cow<'_, RenderPlan>, Option<StyleChoice>) = if job.plan.artist_mode == RENDER_ARTIST_MODE_STYLE {
        let Some(source) = job.style_source.clone() else {
            return fail("精准反推缺少题目图片来源".to_string());
        };
        let choice = match resolve_style(db, data_dir, style, source, &job.plan.style).await {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let p = plan_for_plugin(job.plan, &choice);
        (Cow::Owned(p), Some(choice))
    } else {
        (Cow::Borrowed(job.plan), None)
    };
    let body = build_render_body(job.cleaned, job.framing, &plan, job.original);
    let outcome = match call_render(http, session, &body).await {
        Ok(o) => o,
        Err(e) => return fail(e.message),
    };
    if let Err(e) = db.with(|c| save_outcome(c, run_id, &outcome, plan.artist.as_deref(), style_choice.as_ref())) {
        return fail(format!("落库失败: {}", e.message));
    }
    let bytes = match fetch_full_image(http, session, &outcome.file).await {
        Ok(b) => Arc::new(b),
        Err(e) => return fail(format!("取图失败（{}）: {}", outcome.file, e.message)),
    };
    let abs = data_dir.join(render_rel_path(run_id));
    if let Err(e) = write_image(abs.clone(), bytes.clone()).await {
        return fail(format!("写图失败: {e}"));
    }
    let local = abs.to_string_lossy().to_string();
    if let Err(e) = db.with(|c| save_local(c, run_id, &local)) {
        return fail(format!("落库失败: {}", e.message));
    }
    // §12.6：全部候选另拉到 <run_id>_<idx>.jpg；单张失败只记日志、local_path 留空，不写 render_error（最优那张已经到手）
    let candidates = fetch_candidates(db, http, session, data_dir, run_id, &outcome, &bytes).await;
    if let Err(e) = db.with(|c| save_candidates(c, run_id, &candidates)) {
        tracing::warn!(run_id, "render_candidates 写入失败: {}", e.message);
    }
    db.log_event(
        "info",
        "reverse",
        "重画完成",
        json!({
            "run_id": run_id, "file": outcome.file, "seed": outcome.seed, "size": outcome.size, "size_source": outcome.size_source, "latency_ms": outcome.latency_ms,
            "quality_score": outcome.quality_score, "similarity_score": outcome.similarity_score, "local": local,
            "artist_mode": job.plan.artist_mode, "artist_mode_sent": plan.artist_mode, "artist": plan.artist, "artist_used": outcome.artist, "artist_source": outcome.artist_source,
            "candidates": candidates.len(), "candidates_local": candidates.iter().filter(|c| c.local_path.is_some()).count(),
            "plugin_dropped": outcome.plugin_dropped, "plugin_would_reject": outcome.plugin_would_reject,
            "style": style_choice, "style_model": job.plan.style.model,
        }),
    );
    Ok(RenderDone { outcome, style: style_choice })
}

/// 在阻塞线程里原子写一张图；错误转成文字（`render_and_store` 只写 `render_error`）。
async fn write_image(abs: PathBuf, bytes: Arc<Vec<u8>>) -> Result<(), String> {
    match tokio::task::spawn_blocking(move || write_file_atomic(&abs, &bytes)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.message),
        Err(e) => Err(e.to_string()),
    }
}

/// §12.6：把 `images[]` 逐张拉到 `labruns/reverse/<run_id>_<idx>.jpg`（最多 `MAX_CANDIDATES` 张；与最优那张同名的不重复下载，直接复用字节）。
/// 先清掉上次重画留下的候选文件（张数可能变少）。单张失败：warn 日志 + `app_event`，该项 `local_path` 留空；返回带 `local_path` 的候选列表。
async fn fetch_candidates(
    db: &Db,
    http: &reqwest::Client,
    session: &PluginSession,
    data_dir: &Path,
    run_id: i64,
    outcome: &RenderOutcome,
    best_bytes: &Arc<Vec<u8>>,
) -> Vec<RenderCandidate> {
    let dir = data_dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        for i in 0..MAX_CANDIDATES {
            let _ = std::fs::remove_file(dir.join(candidate_rel_path(run_id, i)));
        }
    })
    .await;
    let mut candidates: Vec<RenderCandidate> = outcome.images.iter().take(MAX_CANDIDATES).cloned().collect();
    for (idx, cand) in candidates.iter_mut().enumerate() {
        let bytes = if cand.file == outcome.file {
            Ok(best_bytes.clone())
        } else {
            fetch_full_image(http, session, &cand.file).await.map(Arc::new).map_err(|e| e.message)
        };
        let abs = data_dir.join(candidate_rel_path(run_id, idx));
        let written = match bytes {
            Ok(b) => write_image(abs.clone(), b).await,
            Err(e) => Err(e),
        };
        match written {
            Ok(()) => cand.local_path = Some(abs.to_string_lossy().to_string()),
            Err(e) => {
                tracing::warn!(run_id, idx, "候选图取图失败（{}）: {e}", cand.file);
                db.log_event("warn", "reverse", "候选图取图失败", json!({ "run_id": run_id, "idx": idx, "file": cand.file, "error": e }));
            }
        }
    }
    candidates
}

/// §11.4 `reverse_render_run`：单条重画，覆盖旧结果。角色与固定种子沿用批次当时的（`request_json.render`），
/// `judge_provider_id`、§12.5 的 `artist`、§12.6 的 `render_artist_mode`（null / 空 = pool）与 `render_count`（null = 插件默认）、
/// §12.7 的 `style`（五个 `style_*` 参数，null → 缺省）都以本次参数为准（不从记录里回退）。原图找不到（本地文件已删）时不送原图，
/// 只打质量分、尺寸退到 `tags`。`style` 模式在碰插件之前先查索引就绪（`bad_request`）。
/// 重画失败不抛：`render_error` 写进行里，返回的 `RunRow` 带着它；参数非法、插件地址 / 密码未配置、run 不存在、没有标签才抛错。
pub async fn rerender(
    state: &AppState,
    run_id: i64,
    judge_provider_id: Option<String>,
    artist: Option<String>,
    render_artist_mode: Option<String>,
    render_count: Option<i64>,
    mut style: StyleParams,
) -> AppResult<RunRow> {
    // §12.6：先校验，不碰库也不碰插件
    let artist_mode = normalize_artist_mode(render_artist_mode.as_deref().unwrap_or(""));
    validate_render_params(&artist_mode, artist.as_deref(), render_count)?;
    let is_style = artist_mode == RENDER_ARTIST_MODE_STYLE;
    if is_style {
        validate_style_params(&style)?;
    }
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let requested_model = style.model.clone();
    let (target, fixed_model) = tokio::task::spawn_blocking(move || {
        db.with(|c| {
            // §12.7：索引未建 / 模型未下载 → 先报 bad_request，不碰插件
            let fixed_model = if is_style {
                Some(style_search::ensure_ready_for_model(c, &data_dir, requested_model.as_deref())?.0)
            } else { requested_model };
            Ok((store::render_target(c, run_id)?, fixed_model))
        })
    })
    .await
    .map_err(join_err)??;
    if target.cleaned.is_empty() {
        return Err(AppError::bad_request(format!("run {run_id} 没有可用的标签（status={}），无法重画", target.status)));
    }
    let mut plan = RenderPlan::from_request_log(&target.request);
    plan.judge_provider_id = non_empty(judge_provider_id.as_deref());
    plan.artist = non_empty(artist.as_deref());
    plan.artist_mode = artist_mode;
    plan.count = render_count;
    style.model = fixed_model;
    plan.style = style;
    let session = PluginSession::new(plugin_conn(state).await?);
    let original = match target.image_path.clone() {
        Some(p) => match tokio::task::spawn_blocking(move || client::prepare_image(Path::new(&p))).await.map_err(join_err)? {
            Ok(img) => Some(img),
            Err(e) => {
                tracing::warn!(run_id, "重画：原图读不到，不送成对评委: {}", e.message);
                None
            }
        },
        None => None,
    };
    let job = RenderJob { run_id, cleaned: &target.cleaned, framing: target.framing.as_deref(), plan: &plan, original: original.as_ref(), style_source: Some(target.source.clone()) };
    let _ = render_and_store(&state.db, &state.http, Some(&session), &state.data_dir, &state.style, job).await;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.with(|c| store::run_row(c, run_id))).await.map_err(join_err)?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> RenderPlan {
        RenderPlan {
            character: "薇拉".into(),
            seed: Some(42),
            judge_provider_id: Some("gpt-4o".into()),
            artist: Some("1.2::artist:foo::, artist:bar".into()),
            artist_mode: "given".into(),
            count: Some(3),
            style: StyleParams::default(),
        }
    }

    /// §11.3 请求体：九个键 + §12.5 的 artist + §12.6 的四个键不多不少、tags 用 ", " 连接、framing 空串当 null、原图 base64 与 mime 来自 prepare_image、
    /// judge 恒 true、原图宽高是缩放前的、artist_mode / count 照计划。
    #[test]
    fn render_body_matches_contract() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.png");
        image::RgbImage::from_pixel(6, 4, image::Rgb([5, 6, 7])).save(&p).unwrap();
        let img = client::prepare_image(&p).unwrap();
        let cleaned = vec!["looking at viewer".to_string(), "smile".to_string(), "1.2::sunlight::".to_string()];
        let body = build_render_body(&cleaned, Some("cowboy shot"), &plan(), Some(&img));
        let mut keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "artist", "artist_mode", "character", "count", "framing", "judge", "judge_provider_id", "negative_extra", "original_base64", "original_height",
                "original_mime", "original_width", "seed", "tags"
            ],
            "十键 + §12.6 四键"
        );
        assert_eq!(body["artist"], "1.2::artist:foo::, artist:bar");
        assert_eq!((body["artist_mode"].as_str(), body["count"].as_i64()), (Some("given"), Some(3)));
        assert_eq!((body["original_width"].as_u64(), body["original_height"].as_u64()), (Some(6), Some(4)), "缩放前原图宽高");
        assert_eq!(body["tags"], "looking at viewer, smile, 1.2::sunlight::");
        assert_eq!(body["framing"], "cowboy shot");
        assert_eq!(body["character"], "薇拉");
        assert_eq!(body["seed"], 42);
        assert!(body["negative_extra"].is_null());
        assert_eq!(body["judge"], true);
        assert_eq!(body["original_mime"], "image/png");
        assert_eq!(body["original_base64"], client::image_base64(&img));
        assert_eq!(body["judge_provider_id"], "gpt-4o");
        // 没有 framing / 原图 / 评委 / 种子 → null，键仍在
        let no = build_render_body(&cleaned, Some("  "), &RenderPlan { character: "x".into(), ..RenderPlan::defaults() }, None);
        assert!(no["framing"].is_null() && no["seed"].is_null() && no["original_base64"].is_null() && no["original_mime"].is_null() && no["judge_provider_id"].is_null());
        assert!(no["artist"].is_null(), "不注入画师时 artist 为 null，键仍在");
        assert!(no["original_width"].is_null() && no["original_height"].is_null(), "没原图 → 宽高 null（插件退到 size_for_tags）");
        assert_eq!((no["artist_mode"].as_str(), no["count"].is_null()), (Some("pool"), true), "缺省 pool / 插件默认张数");
        assert_eq!(no["character"], "x");
        assert_eq!(no.as_object().unwrap().len(), 14);
        // 原图解码失败（宽高 0）→ 两个键都 null，base64 仍送（成对评委照跑）
        let zero = PreparedImage { original_width: 0, original_height: 0, ..img.clone() };
        let z = build_render_body(&cleaned, None, &plan(), Some(&zero));
        assert!(z["original_width"].is_null() && z["original_height"].is_null() && z["original_base64"].is_string());
        assert_eq!(candidate_rel_path(17, 2), "labruns/reverse/17_2.jpg");
        assert_eq!(MAX_CANDIDATES, 4);
        assert_eq!(render_route(), "/api/plug/astrbot_plugin_vivian/page/lab/reverse/render");
        assert_eq!(thumb_route(), "/api/plug/astrbot_plugin_vivian/page/thumb");
        assert_eq!(render_rel_path(17), "labruns/reverse/17.jpg");
    }

    /// `RenderPlan`：render=false → None；角色留空 → 薇拉；评委空串 → null；`request_json.render` 读回，坏形状回缺省。
    #[test]
    fn render_plan_from_request_and_log() {
        let base = RunBatchRequest {
            question_ids: vec![],
            template_id: 1,
            ruleset_id: 1,
            provider_ids: vec![],
            note: None,
            render: false,
            render_character: None,
            render_seed: None,
            judge_provider_id: None,
            artist: None,
            render_artist_mode: "pool".into(),
            render_count: None,
            style_model: None,
            style_top_k: 5,
            style_decisions: vec![],
            style_min_score: None,
            style_fallback: "pool".into(),
            style_center: true,
        };
        assert!(RenderPlan::from_request(&base).is_none());
        let on = RunBatchRequest { render: true, render_character: Some("  ".into()), render_seed: Some(7), judge_provider_id: Some(" ".into()), ..base.clone() };
        let p = RenderPlan::from_request(&on).unwrap();
        assert_eq!((p.character.as_str(), p.seed, p.judge_provider_id.as_deref()), ("薇拉", Some(7), None));
        let named = RunBatchRequest { render_character: Some(" 阿莉亚 ".into()), judge_provider_id: Some("gpt".into()), ..on.clone() };
        let p2 = RenderPlan::from_request(&named).unwrap();
        assert_eq!((p2.character.as_str(), p2.judge_provider_id.as_deref()), ("阿莉亚", Some("gpt")));
        assert_eq!(p2.artist, None);
        // §12.5：artist 去首尾空白，空白当 null
        let with_artist = RunBatchRequest { artist: Some("  artist:foo  ".into()), ..on.clone() };
        assert_eq!(RenderPlan::from_request(&with_artist).unwrap().artist.as_deref(), Some("artist:foo"));
        assert_eq!(RenderPlan::from_request(&RunBatchRequest { artist: Some(" ".into()), ..on.clone() }).unwrap().artist, None);
        let log = json!({ "model": "m", "render": serde_json::to_value(&p2).unwrap() });
        assert_eq!(RenderPlan::from_request_log(&log), p2);
        assert_eq!(RenderPlan::from_request_log(&json!({ "model": "m" })), RenderPlan::defaults());
        assert_eq!((p2.artist_mode.as_str(), p2.count), ("pool", None), "§12.6 缺省");
        // §12.6：mode 归一（空白 → pool、大小写与空白剥掉）、count 原样；老记录缺键 → pool / None
        let mode = RunBatchRequest { render_artist_mode: "  Given ".into(), artist: Some("artist:a".into()), render_count: Some(2), ..on.clone() };
        let pm = RenderPlan::from_request(&mode).unwrap();
        assert_eq!((pm.artist_mode.as_str(), pm.count), ("given", Some(2)));
        assert_eq!(RenderPlan::from_request(&RunBatchRequest { render_artist_mode: " ".into(), ..on.clone() }).unwrap().artist_mode, "pool");
        assert_eq!(
            serde_json::to_value(&pm).unwrap(),
            json!({ "character": "薇拉", "seed": 7, "judge_provider_id": null, "artist": "artist:a", "artist_mode": "given", "count": 2,
                    "style_model": null, "style_top_k": 5, "style_decisions": [], "style_min_score": null, "style_fallback": "pool", "style_center": true }),
            "六键 + §12.7 铺平的五键都序列化"
        );
        let old_log = RenderPlan::from_request_log(&json!({ "render": { "character": "x", "seed": 1, "artist": "artist:a" } }));
        assert_eq!((old_log.artist_mode.as_str(), old_log.count, old_log.artist.as_deref()), ("pool", None, Some("artist:a")), "老记录缺 artist_mode / count 也能收");
        assert_eq!(RenderPlan::from_request_log(&json!({ "render": { "character": "x", "artist_mode": "none", "count": 4 } })).count, Some(4));
        // validate_render_params：四个值 + 空；given 要带画师；style 不要画师；count 1..=4 或 null
        for m in ["pool", "given", "none", "style", "", " NONE ", " Style "] {
            assert!(validate_render_params(m, Some("artist:a"), None).is_ok(), "{m:?}");
        }
        assert!(validate_render_params("style", None, None).is_ok(), "§12.7 style 不需要 artist");
        assert_eq!(validate_render_params("llm", None, None).unwrap_err().code, "bad_request");
        assert_eq!(validate_render_params("given", None, None).unwrap_err().code, "bad_request");
        assert_eq!(validate_render_params("given", Some("  "), None).unwrap_err().code, "bad_request");
        assert!(validate_render_params("pool", None, None).is_ok() && validate_render_params("none", Some(" "), Some(1)).is_ok());
        for n in [0, -1, 5] {
            assert_eq!(validate_render_params("pool", None, Some(n)).unwrap_err().code, "bad_request", "{n}");
        }
        assert!(validate_render_params("pool", None, Some(4)).is_ok());
        assert_eq!(normalize_artist_mode("  Pool "), "pool");
        assert_eq!(RenderPlan::from_request_log(&json!({ "render": { "character": "x", "artist": "artist:a" } })).artist.as_deref(), Some("artist:a"), "老记录缺 artist 也能收、有则读回");
        assert_eq!(RenderPlan::from_request_log(&json!({ "render": { "character": "" } })).character, "薇拉");
        assert_eq!(RenderPlan::from_request_log(&json!({ "render": { "character": "x", "seed": 3 } })).seed, Some(3), "judge_provider_id 缺键也能收");
    }

    /// §12.7 `StyleParams`：从请求归一（fallback 去空白小写、空 → pool；decisions 去空白小写、丢空项）、`from_opts` 的 null → 缺省、
    /// `search_request` 画风词典全量且模型 / 决策范围 / 去内容照参数；`request_json.render` 老记录缺 style_* → 缺省、有则读回；`validate_style_params` 各分支。
    #[test]
    fn style_params_normalize_validate_and_round_trip() {
        let base = RunBatchRequest {
            question_ids: vec![],
            template_id: 1,
            ruleset_id: 1,
            provider_ids: vec![],
            note: None,
            render: true,
            render_character: None,
            render_seed: None,
            judge_provider_id: None,
            artist: None,
            render_artist_mode: "style".into(),
            render_count: None,
            style_model: Some("clip-vit-base-patch32".into()),
            style_top_k: 3,
            style_decisions: vec![" Include ".into(), "".into(), "UNDECIDED".into()],
            style_min_score: Some(0.4),
            style_fallback: " None ".into(),
            style_center: false,
        };
        let p = RenderPlan::from_request(&base).unwrap();
        assert_eq!(p.artist_mode, "style");
        assert_eq!(p.style, StyleParams { model: Some("clip-vit-base-patch32".into()), style_top_k: 3, style_decisions: vec!["include".into(), "undecided".into()], style_min_score: Some(0.4), style_fallback: "none".into(), style_center: false });
        assert_eq!(p.style.decisions(), [Decision::Include, Decision::Undecided]);
        let sr = p.style.search_request(StyleSearchSource::Path { path: "/q.png".into() });
        assert_eq!((sr.top_k, sr.codex_ids.len(), sr.decisions.clone(), sr.center), (3, 0, vec![Decision::Include, Decision::Undecided], false), "画风词典全量、决策范围与去内容照参数");
        assert_eq!(sr.source, StyleSearchSource::Path { path: "/q.png".into() });
        assert_eq!(sr.model.as_deref(), Some("clip-vit-base-patch32"));
        assert_eq!(StyleParams::default(), StyleParams { model: None, style_top_k: 5, style_decisions: vec![], style_min_score: None, style_fallback: "pool".into(), style_center: true });
        assert_eq!(StyleParams::from_opts(None, None, None, None, None), StyleParams::default(), "reverse_render_run 五个都 null → 缺省");
        assert_eq!(
            StyleParams::from_opts(Some(7), Some(vec!["Exclude".into()]), Some(-0.2), Some(" pool ".into()), Some(false)),
            StyleParams { model: None, style_top_k: 7, style_decisions: vec!["exclude".into()], style_min_score: Some(-0.2), style_fallback: "pool".into(), style_center: false }
        );
        assert_eq!(normalize_style_fallback("  "), "pool");
        assert_eq!(normalize_style_fallback(" NONE "), "none");
        // request_json.render 往返：铺平的五键；老记录缺键 → 缺省
        let log = json!({ "render": serde_json::to_value(&p).unwrap() });
        assert_eq!(RenderPlan::from_request_log(&log), p);
        assert_eq!(log["render"]["style_fallback"], "none");
        assert_eq!(log["render"]["style_model"], "clip-vit-base-patch32");
        let old = RenderPlan::from_request_log(&json!({ "render": { "character": "x", "artist_mode": "style" } }));
        assert_eq!((old.artist_mode.as_str(), old.style), ("style", StyleParams::default()), "老记录缺 style_* → 缺省");
        // validate_style_params
        assert!(validate_style_params(&p.style).is_ok());
        assert!(validate_style_params(&StyleParams::default()).is_ok());
        for k in [0, -1, 51] {
            assert_eq!(validate_style_params(&StyleParams { style_top_k: k, ..StyleParams::default() }).unwrap_err().code, "bad_request", "top_k {k}");
        }
        assert!(validate_style_params(&StyleParams { style_top_k: 50, ..StyleParams::default() }).is_ok());
        let bad_dec = validate_style_params(&StyleParams { style_decisions: vec!["include".into(), "maybe".into()], ..StyleParams::default() }).unwrap_err();
        assert!(bad_dec.code == "bad_request" && bad_dec.message.contains("maybe"), "{bad_dec:?}");
        assert_eq!(validate_style_params(&StyleParams { style_fallback: "given".into(), ..StyleParams::default() }).unwrap_err().code, "bad_request");
        for m in [1.5, -1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(validate_style_params(&StyleParams { style_min_score: Some(m), ..StyleParams::default() }).unwrap_err().code, "bad_request", "min_score {m}");
        }
        for m in [-1.0, 0.0, 1.0] {
            assert!(validate_style_params(&StyleParams { style_min_score: Some(m), ..StyleParams::default() }).is_ok(), "min_score {m}");
        }
    }

    fn hit(entry_id: &str, codex_id: &str, score: f64, core: Option<&str>, pack_tags: Option<&str>, prose: bool, decision: Decision) -> StyleHit {
        StyleHit {
            entry_id: entry_id.into(),
            codex_id: codex_id.into(),
            title: format!("title-{entry_id}"),
            path: vec![],
            decision,
            blacklisted: false,
            score,
            style_prompt: core.unwrap_or_default().to_string(),
            core: core.map(str::to_string),
            pack_tags: pack_tags.map(str::to_string),
            thumb_path: None,
            prose,
        }
    }

    /// 高分图包不能进入画风重画；完整配方不因 core 缺失或解析结果不同而截断。
    #[test]
    fn style_hit_selection_preserves_complete_recipe_and_excludes_packs() {
        let mut full = hit("a_full", "artist_nai5_personal", 0.70, Some("artist:freng"), None, false, Decision::Include);
        full.style_prompt = format!("  {}\n", crate::artist::FULL_STYLE_RECIPE);
        let hits = vec![
            hit("p_prose", "nai5_community_pack", 0.95, Some("artist:wrong"), Some("<scene>A girl.</scene>"), true, Decision::Undecided),
            hit("a_empty", "artist_nai5_personal", 0.93, Some("  "), None, false, Decision::Include),
            hit("p_pack", "nai5_community_pack", 0.80, Some("artist:wrong"), Some("sunlight, smile"), false, Decision::Exclude),
            full,
        ];
        let c = pick_style_hit(&hits, None);
        assert_eq!(
            c,
            StyleChoice {
                entry_id: Some("a_full".into()), title: Some("title-a_full".into()), score: Some(0.70),
                decision: Some("include".into()), fallback: false, artist: Some(crate::artist::FULL_STYLE_RECIPE.into()),
            },
            "图包及空配方跳过，保留画风原文的正负权重、质量和线条词"
        );
        assert!(c.used());
        let fb = pick_style_hit(&hits, Some(0.9));
        assert_eq!((fb.entry_id.as_deref(), fb.score, fb.fallback, fb.artist.as_deref()), (Some("a_full"), Some(0.70), true, None));
        assert!(!fb.used());
        assert!(pick_style_hit(&hits, Some(0.70)).used(), "score == min_score 不兜底");
        assert_eq!(pick_style_hit(&hits[..3], None), StyleChoice { fallback: true, ..StyleChoice::default() });
        assert_eq!(pick_style_hit(&[], Some(0.1)), StyleChoice { fallback: true, ..StyleChoice::default() });
        assert_eq!(style_artist_of(&hits[0]), None, "散文图包也不能成为画风候选");
        assert_eq!(style_artist_of(&hits[2]), None, "旧图包命中即使带 artist/core 也要挡住");
        let mut without_core = hits[3].clone();
        without_core.core = None;
        assert_eq!(style_artist_of(&without_core).as_deref(), Some(crate::artist::FULL_STYLE_RECIPE), "完整配方不依赖画师解析");
        let p = plan_for_plugin(&RenderPlan { artist_mode: "style".into(), ..plan() }, &c);
        let body = build_render_body(&["smile".into()], None, &p, None);
        assert_eq!(body["artist_mode"], "given");
        assert_eq!(body["artist"], crate::artist::FULL_STYLE_RECIPE, "实际插件请求也不能截断或改权重");
    }

    /// §12.7「插件不改」：`artist_mode=style` 的计划经 `plan_for_plugin` 后，请求体里的 `artist_mode` 只会是 given / pool / none，永远不是 "style"——
    /// 有命中 → given + 命中串；兜底 → style_fallback（pool / none）且 artist null；其余字段原样。
    #[test]
    fn style_plan_is_translated_before_reaching_plugin() {
        let style_plan = RenderPlan {
            artist_mode: "style".into(),
            artist: None,
            style: StyleParams { style_fallback: "none".into(), style_min_score: Some(0.5), ..StyleParams::default() },
            ..plan()
        };
        let used = StyleChoice { entry_id: Some("a_foo".into()), title: Some("foo".into()), score: Some(0.83), decision: Some("include".into()), fallback: false, artist: Some("1.2::artist:foo::".into()) };
        let p = plan_for_plugin(&style_plan, &used);
        assert_eq!((p.artist_mode.as_str(), p.artist.as_deref()), ("given", Some("1.2::artist:foo::")));
        assert_eq!((p.character.as_str(), p.seed, p.count, &p.style), ("薇拉", Some(42), Some(3), &style_plan.style), "其余字段原样");
        let below = StyleChoice { artist: None, fallback: true, ..used.clone() };
        let p = plan_for_plugin(&style_plan, &below);
        assert_eq!((p.artist_mode.as_str(), p.artist), ("none", None), "兜底按 style_fallback");
        let pool_plan = RenderPlan { style: StyleParams::default(), ..style_plan.clone() };
        let miss = StyleChoice { fallback: true, ..StyleChoice::default() };
        let p = plan_for_plugin(&pool_plan, &miss);
        assert_eq!((p.artist_mode.as_str(), p.artist), ("pool", None), "无命中兜底缺省 pool");
        // 请求体里 artist_mode 永远是插件认识的三个值之一
        for (pl, ch) in [(&style_plan, &used), (&style_plan, &below), (&pool_plan, &miss)] {
            let body = build_render_body(&["smile".to_string()], None, &plan_for_plugin(pl, ch), None);
            let mode = body["artist_mode"].as_str().unwrap();
            assert!(["pool", "given", "none"].contains(&mode), "插件只认 pool / given / none: {mode}");
            assert_ne!(mode, RENDER_ARTIST_MODE_STYLE);
        }
        assert_eq!(build_render_body(&[], None, &style_plan, None)["artist_mode"], "style", "未经翻译的计划才带 style——render_and_store 不会这样送");
    }

    /// §11.3 响应解析：AstrBot `{ status, data }` 包装、缺键 / null 容忍、file 必需；thumb 的 `{ data: { b64 } }` 解码。
    #[test]
    fn render_and_thumb_responses_parse() {
        let raw = json!({ "status": "ok", "data": {
            "file": "lab_rerender_20260907-120000.png", "seed": 123, "prompt": "薇拉, 1girl, smile", "negative": "lowres", "size": "832x1216",
            "latency_ms": 9800, "quality_score": 7.5, "quality_notes": ["手部正常", 3, "光影合理"], "similarity_score": 8, "similarity_reason": "构图一致"
        } });
        let out = parse_render_response(&raw).unwrap();
        assert_eq!(out.file, "lab_rerender_20260907-120000.png");
        assert_eq!((out.seed, out.latency_ms), (Some(123), Some(9800)));
        assert_eq!((out.prompt.as_str(), out.negative.as_str(), out.size.as_str()), ("薇拉, 1girl, smile", "lowres", "832x1216"));
        assert_eq!(out.quality_notes, ["手部正常", "光影合理"], "非字符串的 note 跳过");
        assert_eq!((out.quality_score, out.similarity_score, out.similarity_reason.as_deref()), (Some(7.5), Some(8.0), Some("构图一致")));
        // 评委失败：分数 null、notes 缺、reason 空串 → None；size 不是字符串也不炸；无包装对象（没有 data 包装）也收
        let lenient = parse_render_response(&json!({ "file": "x.png", "quality_score": null, "similarity_score": null, "similarity_reason": "", "size": [832, 1216] })).unwrap();
        assert_eq!((lenient.quality_score, lenient.similarity_score, lenient.similarity_reason), (None, None, None));
        assert!(lenient.quality_notes.is_empty() && lenient.prompt.is_empty() && lenient.seed.is_none());
        assert_eq!(lenient.size, "[832,1216]");
        assert_eq!(parse_render_response(&json!({ "status": "ok", "data": { "seed": 1 } })).unwrap_err().code, "plugin_rejected");
        assert_eq!(parse_render_response(&json!({ "status": "ok", "data": { "file": "  " } })).unwrap_err().code, "plugin_rejected");
        // thumb
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"\xff\xd8jpeg");
        assert_eq!(parse_thumb_response(&json!({ "status": "ok", "data": { "b64": b64 } })).unwrap(), b"\xff\xd8jpeg");
        assert_eq!(parse_thumb_response(&json!({ "data_base64": format!("data:image/jpeg;base64,{b64}\n") })).unwrap(), b"\xff\xd8jpeg");
        assert_eq!(parse_thumb_response(&json!({ "status": "ok", "data": { "b64": "" } })).unwrap_err().code, "plugin_rejected");
        assert_eq!(parse_thumb_response(&json!({ "status": "error", "message": "not found" })).unwrap_err().code, "plugin_rejected");
        // v0.2.4 形状（老插件）：§12.6 的字段全空
        assert_eq!((out.size_source, out.artist, out.artist_source, out.best_idx), (None, None, None, None));
        assert!(out.images.is_empty() && out.plugin_dropped.is_empty() && !out.plugin_would_reject);
    }

    /// §12.6 响应：三张候选 + best_idx、size_source / artist / artist_source / plugin_dropped / plugin_would_reject；老插件（没有这些键）→ 空 / false / None。
    #[test]
    fn render_response_with_candidates_parses() {
        let raw = json!({ "status": "ok", "data": {
            "file": "lab_rerender_b.png", "seed": 101, "prompt": "p", "negative": "n", "size": "1216x832", "size_source": "ratio", "latency_ms": 30000,
            "quality_score": 8.5, "quality_notes": ["构图完整"], "similarity_score": 7, "similarity_reason": "姿势一致",
            "artist": "artist:foo, 0.8::artist:bar::", "artist_source": "pool",
            "images": [
                { "file": "lab_rerender_a.png", "seed": 100, "quality_score": 6.0, "quality_notes": ["手指粘连"] },
                { "file": "lab_rerender_b.png", "seed": 101, "quality_score": 8.5, "quality_notes": ["构图完整"] },
                { "file": "lab_rerender_c.png", "seed": 102, "quality_score": null, "quality_notes": [] }
            ],
            "best_idx": 1, "plugin_dropped": ["long hair", "realistic"], "plugin_would_reject": true
        } });
        let out = parse_render_response(&raw).unwrap();
        assert_eq!((out.file.as_str(), out.seed, out.size.as_str(), out.size_source.as_deref()), ("lab_rerender_b.png", Some(101), "1216x832", Some("ratio")));
        assert_eq!((out.artist.as_deref(), out.artist_source.as_deref()), (Some("artist:foo, 0.8::artist:bar::"), Some("pool")));
        assert_eq!(out.images.len(), 3);
        assert_eq!(out.images[0], RenderCandidate { file: "lab_rerender_a.png".into(), seed: Some(100), quality_score: Some(6.0), quality_notes: vec!["手指粘连".into()], local_path: None });
        assert_eq!((out.images[2].seed, out.images[2].quality_score, out.images[2].quality_notes.len()), (Some(102), None, 0));
        assert_eq!(out.best_idx, Some(1));
        assert_eq!(out.plugin_dropped, ["long hair", "realistic"]);
        assert!(out.plugin_would_reject);
        assert_eq!((out.quality_score, out.similarity_score), (Some(8.5), Some(7.0)), "顶层仍是最优那张的分");
        // best_idx 与 file 对不上时以 file 为准；file 不在 images 里才信 best_idx（且要在范围内）；没有 file 的候选跳过；0/1 也当布尔
        let mut d = raw["data"].clone();
        d["best_idx"] = json!(0);
        assert_eq!(parse_render_response(&d).unwrap().best_idx, Some(1), "顶层 file 就是最优那张");
        d["file"] = json!("elsewhere.png");
        d["best_idx"] = json!(2);
        d["plugin_would_reject"] = json!(0);
        let o = parse_render_response(&d).unwrap();
        assert_eq!((o.best_idx, o.plugin_would_reject), (Some(2), false));
        d["best_idx"] = json!(7);
        assert_eq!(parse_render_response(&d).unwrap().best_idx, None, "越界 best_idx 不信");
        d["best_idx"] = json!(-1);
        assert_eq!(parse_render_response(&d).unwrap().best_idx, None);
        d["images"] = json!([{ "seed": 1 }, { "file": " " }, { "file": "ok.png", "quality_notes": "not array" }]);
        d["best_idx"] = json!(0);
        let o = parse_render_response(&d).unwrap();
        assert_eq!(o.images.len(), 1, "没有 file 的候选跳过");
        assert!(o.images[0].quality_notes.is_empty());
        assert_eq!(o.best_idx, Some(0));
        d["images"] = json!("not array");
        d["plugin_dropped"] = json!(null);
        let o = parse_render_response(&d).unwrap();
        assert!(o.images.is_empty() && o.best_idx.is_none() && o.plugin_dropped.is_empty());
        // 老插件（v0.2.4 形状）：§12.6 字段全空；artist 空串当 None
        let old = parse_render_response(&json!({ "status": "ok", "data": { "file": "x.png", "seed": 1, "size": "832x1216", "artist": "  " } })).unwrap();
        assert_eq!((old.size_source, old.artist, old.artist_source, old.best_idx), (None, None, None, None));
        assert!(old.images.is_empty() && old.plugin_dropped.is_empty() && !old.plugin_would_reject);
    }

    #[tokio::test]
    async fn rerender_attempt_records_actual_model_without_rewriting_batch_plan() {
        let db = Arc::new(Db::open_memory().unwrap());
        db.with(|c| {
            c.execute_batch("INSERT INTO reverse_batch(batch_id, created_at, template_id, ruleset_id, provider_ids_json, question_count, status) VALUES ('b','t',1,1,'[1]',1,'done');")?;
            c.execute("INSERT INTO reverse_run(run_id,batch_id,created_at,source_kind,source_ref,image_sha256,template_id,ruleset_id,provider_id,request_json,status) VALUES (1,'b','t','local_file','/tmp/q.png','x',1,1,1,?1,'ok')", params![json!({"render":{"style_model":"clip-vit-base-patch32"}}).to_string()])?;
            Ok(())
        }).unwrap();
        let mut plan = RenderPlan::defaults();
        plan.artist_mode = RENDER_ARTIST_MODE_STYLE.into();
        plan.style.model = Some("megastyle-siglip-so400m".into());
        let error = render_and_store(&db, &reqwest::Client::new(), None, Path::new("/tmp"), &StyleState::new(), RenderJob { run_id: 1, cleaned: &[], framing: None, plan: &plan, original: None, style_source: None }).await.unwrap_err();
        assert!(error.contains("插件连接未初始化"));
        let request: String = db.with(|c| Ok(c.query_row("SELECT request_json FROM reverse_run WHERE run_id=1", [], |r| r.get(0))?)).unwrap();
        let request: Value = serde_json::from_str(&request).unwrap();
        assert_eq!(request["render"]["style_model"], "clip-vit-base-patch32");
        assert_eq!(request["last_render_request"]["style_model"], "megastyle-siglip-so400m");
    }

    /// 落库：出图结果写 render_* 列并清 render_error；取到图再写 render_local；失败只写 render_error、旧结果保留；`render_target` 取重画材料。
    #[test]
    fn render_columns_round_trip() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO reverse_batch(batch_id, created_at, template_id, ruleset_id, provider_ids_json, question_count, status) VALUES ('b', 't', 1, 1, '[1]', 1, 'done');
                 INSERT INTO reverse_run(run_id, batch_id, created_at, source_kind, source_ref, image_sha256, template_id, ruleset_id, provider_id, request_json, parsed_json, status, cleaned_tags_json, render_error)
                   VALUES (1, 'b', 't', 'local_file', '/tmp/q.png', 'x', 1, 1, 1, '{\"render\":{\"character\":\"阿莉亚\",\"seed\":9}}', '{\"tags\":\"smile\",\"framing\":\"cowboy shot\"}', 'ok', '[\"smile\"]', 'old error');",
            )?;
            let out = RenderOutcome {
                file: "lab_rerender_1.png".into(),
                seed: Some(5),
                prompt: "p".into(),
                negative: "n".into(),
                size: "832x1216".into(),
                latency_ms: Some(1),
                quality_score: Some(7.0),
                quality_notes: vec!["a".into()],
                similarity_score: None,
                similarity_reason: None,
                ..Default::default()
            };
            save_outcome(c, 1, &out, Some("artist:foo"), None)?;
            let row = store::run_row(c, 1)?;
            assert_eq!((row.render_seed, row.render_prompt.as_deref(), row.quality_score, row.render_path, row.render_error), (Some(5), Some("p"), Some(7.0), None, None));
            assert!(row.render_style_entry_id.is_none() && row.render_style_score.is_none() && !row.render_style_fallback, "§12.7 非 style 模式五列空");
            assert_eq!(row.render_artist.as_deref(), Some("artist:foo"), "§12.5 注入的画师串落进 render_artist（老插件没回 artist_source → 用 Mac 送去的）");
            assert_eq!((row.render_artist_source, row.render_size.as_deref(), row.render_size_source), (None, Some("832x1216"), None), "§12.6：老插件只有 size");
            assert!(row.plugin_dropped.is_empty() && !row.plugin_would_reject && row.render_candidates.is_empty());
            assert_eq!(row.quality_notes, ["a"]);
            assert_eq!((row.truth_kind.as_str(), row.leak_appearance, row.leak_banned), ("tags", 0, 0), "直接插的行按列默认值");
            save_local(c, 1, "/data/labruns/reverse/1.jpg")?;
            assert_eq!(store::run_row(c, 1)?.render_path.as_deref(), Some("/data/labruns/reverse/1.jpg"));
            save_error(c, 1, "取图失败")?;
            let row = store::run_row(c, 1)?;
            assert_eq!(
                (row.render_error.as_deref(), row.render_path.as_deref(), row.render_seed),
                (Some("取图失败"), Some("/data/labruns/reverse/1.jpg"), Some(5)),
                "失败只写 render_error，旧结果保留"
            );
            assert_eq!(save_outcome(c, 99, &out, None, None).unwrap_err().code, "not_found");
            save_outcome(c, 1, &out, Some("  "), None)?;
            assert_eq!(store::run_row(c, 1)?.render_artist, None, "不注入画师时 render_artist 清空");
            // §12.6：新插件的响应 → 六列；render_artist 以插件回传的为准；save_candidates 回写 local_path；none 模式清空 render_artist
            let aligned = RenderOutcome {
                artist: Some("artist:pooled".into()),
                artist_source: Some("pool".into()),
                size_source: Some("ratio".into()),
                images: vec![
                    RenderCandidate { file: "a.png".into(), seed: Some(1), quality_score: Some(5.5), quality_notes: vec!["x".into()], local_path: None },
                    RenderCandidate { file: "lab_rerender_1.png".into(), seed: Some(5), quality_score: Some(7.0), ..Default::default() },
                ],
                best_idx: Some(1),
                plugin_dropped: vec!["realistic".into()],
                plugin_would_reject: true,
                ..out.clone()
            };
            save_outcome(c, 1, &aligned, Some("artist:foo"), None)?;
            let row = store::run_row(c, 1)?;
            assert_eq!(row.render_artist.as_deref(), Some("artist:pooled"), "插件回了 artist_source 就以它回传的 artist 为准");
            assert_eq!((row.render_artist_source.as_deref(), row.render_size.as_deref(), row.render_size_source.as_deref()), (Some("pool"), Some("832x1216"), Some("ratio")));
            assert_eq!(row.plugin_dropped, ["realistic"]);
            assert!(row.plugin_would_reject);
            assert_eq!(row.render_candidates, aligned.images, "拉图前 local_path 为空");
            assert_eq!((row.render_path, row.render_error), (None, None), "重画后 render_local / render_error 先清空");
            let mut cands = row.render_candidates.clone();
            cands[1].local_path = Some("/data/labruns/reverse/1_1.jpg".into());
            save_candidates(c, 1, &cands)?;
            let row = store::run_row(c, 1)?;
            assert_eq!(row.render_candidates, cands);
            assert_eq!(row.render_candidates[1].local_path.as_deref(), Some("/data/labruns/reverse/1_1.jpg"));
            let none = RenderOutcome { artist: None, artist_source: Some("none".into()), size: String::new(), ..aligned.clone() };
            save_outcome(c, 1, &none, Some("artist:foo"), None)?;
            let row = store::run_row(c, 1)?;
            assert_eq!((row.render_artist, row.render_artist_source.as_deref(), row.render_size), (None, Some("none"), None), "none 模式：哪怕 Mac 送了 artist 也清空；size 空串存 NULL");
            // §12.7：用到检索画风 → 五列 + render_artist_source="style"（插件回的是 given）；兜底 → fallback=1、来源照插件回传；再非 style → 五列清空
            let given = RenderOutcome { artist: Some("1.2::artist:foo::".into()), artist_source: Some("given".into()), ..aligned.clone() };
            let used = StyleChoice { entry_id: Some("a_foo".into()), title: Some("foo".into()), score: Some(0.83), decision: Some("include".into()), fallback: false, artist: Some("1.2::artist:foo::".into()) };
            save_outcome(c, 1, &given, Some("1.2::artist:foo::"), Some(&used))?;
            let row = store::run_row(c, 1)?;
            assert_eq!((row.render_artist.as_deref(), row.render_artist_source.as_deref()), (Some("1.2::artist:foo::"), Some("style")));
            assert_eq!(
                (row.render_style_entry_id.as_deref(), row.render_style_title.as_deref(), row.render_style_score, row.render_style_decision.as_deref(), row.render_style_fallback),
                (Some("a_foo"), Some("foo"), Some(0.83), Some("include"), false)
            );
            let pooled = RenderOutcome { artist: Some("artist:pooled".into()), artist_source: Some("pool".into()), ..aligned.clone() };
            let below = StyleChoice { entry_id: Some("a_foo".into()), title: Some("foo".into()), score: Some(0.31), decision: Some("include".into()), fallback: true, artist: None };
            save_outcome(c, 1, &pooled, None, Some(&below))?;
            let row = store::run_row(c, 1)?;
            assert_eq!((row.render_artist.as_deref(), row.render_artist_source.as_deref()), (Some("artist:pooled"), Some("pool")), "兜底：来源照插件回传");
            assert_eq!((row.render_style_entry_id.as_deref(), row.render_style_score, row.render_style_fallback), (Some("a_foo"), Some(0.31), true), "兜底仍记最像的那条");
            save_outcome(c, 1, &pooled, None, Some(&StyleChoice { fallback: true, ..StyleChoice::default() }))?;
            let row = store::run_row(c, 1)?;
            assert!(row.render_style_entry_id.is_none() && row.render_style_score.is_none() && row.render_style_fallback, "无命中：四列空、fallback=1");
            save_outcome(c, 1, &aligned, Some("artist:foo"), None)?;
            let row = store::run_row(c, 1)?;
            assert!(row.render_style_entry_id.is_none() && !row.render_style_fallback, "非 style 模式再画一次 → 五列清空");
            assert_eq!(row.render_artist_source.as_deref(), Some("pool"));
            // 坏 JSON 的 render_candidates / plugin_dropped 列读成空，不炸
            c.execute("UPDATE reverse_run SET render_candidates = 'oops', plugin_dropped = '{', plugin_would_reject = NULL WHERE run_id = 1", [])?;
            let row = store::run_row(c, 1)?;
            assert!(row.render_candidates.is_empty() && row.plugin_dropped.is_empty() && !row.plugin_would_reject);
            assert_eq!(store::run_row(c, 99).unwrap_err().code, "not_found");
            let t = store::render_target(c, 1)?;
            assert_eq!((t.cleaned, t.image_path.as_deref(), t.framing.as_deref(), t.status.as_str()), (vec!["smile".to_string()], Some("/tmp/q.png"), Some("cowboy shot"), "ok"));
            assert_eq!(t.source, StyleSearchSource::Path { path: "/tmp/q.png".into() }, "§12.7 本地题的检索来源是 path");
            assert_eq!(RenderPlan::from_request_log(&t.request), RenderPlan { character: "阿莉亚".into(), seed: Some(9), ..RenderPlan::defaults() });
            Ok(())
        })
        .unwrap();
    }
}
