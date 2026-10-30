//! 批次后台任务：题（条目×ordinal 或本地文件）× 模型端点，逐题串行、多端点并行；每次调用把
//! 原始响应全文、解析 JSON、清洗结果、指标写入 `reverse_run` / `reverse_metric`（设计 §5.2），
//! 通过 `Channel<RunEvent>` 推 started / run_done / run_failed / batch_done / batch_failed / batch_cancelled。
//! 解析：第一个 `{…}` JSON；失败 → status=error；`suitable=false` 或含拒答语 → status=refused（CONTRACT §4）。
//! 响应只有 `reasoning_content` 没有 `content` 时照常解析，但写一条 warn `app_event`，并在 `error` 里标
//! "answer taken from reasoning_content"（v0.1.1 #13）。
//! v0.2.3（CONTRACT §10.1）：`kind='plugin'` 的端点不调 OpenAI 端点，改 POST 插件 `{PLUGIN_PAGE_PATH}/lab/reverse`
//! （body `{ provider_id, prompt, image_base64, mime, max_tokens }`，`prompt` 与 openai 分支的 system 消息同文，图片同一套
//! `client::prepare_image` 缩放结果），响应 `text` 当模型原文走同样的解析 / 清洗 / 指标；整个批次复用一次登录（`PluginSession`）。
//! v0.2.4（CONTRACT §11.4）：`insert_run` 写 `truth_kind`（图包标签题 tags / 真值是散文 prose / 本地图片 none）与两个泄漏数
//! （任何题都按 pred_raw 算），指标只给 tags 题；`render=true` 时每个解析成功的 run 落库后同步走 `render::render_and_store`
//! 重画一张并评分（失败只写 render_error，再发一次 `run_done` 事件带结果文字）。
//! v0.3.1（CONTRACT §12.7）：`render_artist_mode="style"` 的批次在 `prepare` 里先查画风索引就绪（模型未下载 / 索引为空 → `bad_request`，
//! 不插批次行、不耗点数）；每条 run 的重画由 `render::render_and_store` 先检索再画，`run_done` 文字末尾带「画风=…」。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::params;
use futures_util::{stream::FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::db::{now_iso, Db};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::plugin::{plugin_data, rejected, truncate, PluginSession, PLUGIN_PAGE_PATH};
use crate::secrets;
use crate::style::{search as style_search, StyleState};

use super::builtin::character_aliases;
use super::clean::CompiledRuleset;
use super::client;
use super::control::BatchLease;
use super::metrics;
use super::render::{self, RenderJob, RenderPlan};
use super::store;

pub type RunEmitter = Arc<dyn Fn(RunEvent) + Send + Sync>;

pub struct BatchCtx {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    /// §11.4：重画图落在 `<data_dir>/labruns/reverse/<run_id>.jpg`。
    pub data_dir: PathBuf,
    pub cancel: CancellationToken,
    pub emit: RunEmitter,
    /// §10.1：批次里有 `kind='plugin'` 的端点时由命令层在启动前取好登录信息，整个批次复用一个 JWT；没有插件端点为 None。
    pub plugin: Option<Arc<PluginSession>>,
    /// §12.7：画风模型缓存（`AppState.style`），`render_artist_mode=style` 的重画检索用。
    pub style: Arc<StyleState>,
}

/// 插件 `/lab/reverse` 的超时：插件端 LLM 调用上限 120 s，再留上传大图与排队的余量，与 openai 分支一致取 180 s。
pub const PLUGIN_REVERSE_TIMEOUT_SECS: u64 = 180;

/// `POST {PLUGIN_PAGE_PATH}/lab/reverse`（§10.1）。
pub fn plugin_reverse_route() -> String {
    format!("{PLUGIN_PAGE_PATH}/lab/reverse")
}

/// `request_json` 里插件端点的 model 名：`plugin:<provider_id>`（§10.1）。
pub fn plugin_model_label(plugin_provider_id: &str) -> String {
    format!("plugin:{plugin_provider_id}")
}

/// §10.1 `/lab/reverse` 的请求体，键名照契约：`provider_id`、`prompt`（模板渲染后，与 openai 分支的 system 消息同一段文字）、
/// `image_base64`（`client::prepare_image` 压过的同一份字节）、`mime`、`max_tokens`。
pub fn build_plugin_reverse_body(template_body: &str, img: &client::PreparedImage, plugin_provider_id: &str, max_tokens: i64) -> Value {
    json!({
        "provider_id": plugin_provider_id,
        "prompt": client::render_template(template_body),
        "image_base64": client::image_base64(img),
        "mime": img.mime,
        "max_tokens": max_tokens,
    })
}

#[derive(Debug, Clone)]
pub enum QuestionSpec {
    Pack { entry_id: String, ordinal: i64, image_path: PathBuf },
    Local { path: PathBuf },
}

impl QuestionSpec {
    pub fn question_id(&self) -> String {
        match self {
            QuestionSpec::Pack { entry_id, ordinal, .. } => store::question_id(entry_id, *ordinal),
            QuestionSpec::Local { path } => format!("local:{}", path.display()),
        }
    }
    fn image_path(&self) -> &PathBuf {
        match self {
            QuestionSpec::Pack { image_path, .. } => image_path,
            QuestionSpec::Local { path } => path,
        }
    }
    /// §12.7：画风检索的来源——词典题用 entry/ordinal，本地题用 path。
    pub fn style_source(&self) -> StyleSearchSource {
        match self {
            QuestionSpec::Pack { entry_id, ordinal, .. } => StyleSearchSource::Entry { entry_id: entry_id.clone(), ordinal: *ordinal },
            QuestionSpec::Local { path } => StyleSearchSource::Path { path: path.to_string_lossy().to_string() },
        }
    }
}

pub struct Prepared {
    pub batch_id: String,
    pub template: ReverseTemplate,
    pub ruleset: ReverseRuleset,
    pub providers: Vec<(ReverseProvider, String)>,
    pub questions: Vec<QuestionSpec>,
    /// §11.4：`render=true` 时的角色 / 固定种子 / 评委；None = 不重画。
    pub render: Option<RenderPlan>,
}

/// 解析 question_id：`local:<abs path>` 或 `<entry_id>#<ordinal>`。
pub fn parse_question_id(id: &str) -> AppResult<(Option<String>, Option<(String, i64)>)> {
    if let Some(p) = id.strip_prefix("local:") {
        if p.trim().is_empty() {
            return Err(AppError::bad_request("local: 路径为空"));
        }
        return Ok((Some(p.to_string()), None));
    }
    let (entry, ord) = id.rsplit_once('#').ok_or_else(|| AppError::bad_request(format!("question_id 格式错误: {id}")))?;
    let ordinal: i64 = ord.parse().map_err(|_| AppError::bad_request(format!("question_id ordinal 非整数: {id}")))?;
    Ok((None, Some((entry.to_string(), ordinal))))
}

/// 校验请求、插入 `reverse_batch` 行，返回执行所需的一切。`data_dir` 只给 §12.7 查画风模型在不在场用。
pub fn prepare(db: &Db, data_dir: &std::path::Path, req: &RunBatchRequest) -> AppResult<Prepared> {
    if req.question_ids.is_empty() {
        return Err(AppError::bad_request("question_ids 为空"));
    }
    if req.provider_ids.is_empty() {
        return Err(AppError::bad_request("provider_ids 为空"));
    }
    let style_mode = req.render && render::normalize_artist_mode(&req.render_artist_mode) == RENDER_ARTIST_MODE_STYLE;
    if req.render {
        if let Some(seed) = req.render_seed.filter(|s| *s < 0) {
            return Err(AppError::bad_request(format!("render_seed 不能为负: {seed}")));
        }
        // §12.6：artist_mode 四选一、given 要带画师、count 1..4 或 null（不重画时同样不看）
        render::validate_render_params(&req.render_artist_mode, req.artist.as_deref(), req.render_count)?;
        // §12.7：style 模式再校验五个 style_* 参数
        if style_mode {
            render::validate_style_params(&render::StyleParams::from_request(req))?;
        }
    }
    let mut fixed_req = req.clone();
    db.with(|c| {
        // §12.7：精准反推要用画风索引——模型未下载 / 索引为空在这里就报 bad_request，不插批次行、不耗点数
        if style_mode {
            let (model, _) = style_search::ensure_ready_for_model(c, data_dir, req.style_model.as_deref())?;
            fixed_req.style_model = Some(model);
        }
        let template = store::template_get(c, req.template_id)?;
        let ruleset = store::ruleset_get(c, req.ruleset_id)?;
        CompiledRuleset::compile(&ruleset)?;
        let mut providers = Vec::new();
        for pid in &req.provider_ids {
            let p = store::provider_get(c, *pid)?;
            if !p.enabled {
                return Err(AppError::bad_request(format!("模型端点已禁用: {}", p.name)));
            }
            // §10.1：插件端点不需要 key，不查钥匙串（macOS 上少弹一次授权框）
            let key = if p.is_plugin() {
                if p.plugin_provider_id.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    return Err(AppError::bad_request(format!("插件端点缺 plugin_provider_id: {}", p.name)));
                }
                String::new()
            } else {
                secrets::get(&secrets::provider_account(*pid))?.unwrap_or_default()
            };
            providers.push((p, key));
        }
        let mut questions = Vec::new();
        for qid in &req.question_ids {
            match parse_question_id(qid)? {
                (Some(path), _) => {
                    let pb = PathBuf::from(&path);
                    if !pb.is_file() {
                        return Err(AppError::bad_request(format!("本地图片不存在: {path}")));
                    }
                    questions.push(QuestionSpec::Local { path: pb });
                }
                (_, Some((entry_id, ordinal))) => {
                    let row = store::question_row(c, &entry_id, ordinal)?.ok_or_else(|| AppError::not_found(format!("题目不存在: {qid}")))?;
                    let local = row.local_path.ok_or_else(|| AppError::bad_request(format!("题目 {qid} 的图片尚未下载")))?;
                    questions.push(QuestionSpec::Pack { entry_id, ordinal, image_path: PathBuf::from(local) });
                }
                _ => unreachable!(),
            }
        }
        let batch_id = format!("b-{}", uuid::Uuid::new_v4().simple());
        c.execute(
            "INSERT INTO reverse_batch(batch_id, created_at, finished_at, template_id, ruleset_id, provider_ids_json, question_count, status, note)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, 'running', ?7)",
            params![batch_id, now_iso(), template.template_id, ruleset.ruleset_id, serde_json::to_string(&req.provider_ids)?, questions.len() as i64, req.note],
        )?;
        Ok(Prepared { batch_id, template, ruleset, providers, questions, render: RenderPlan::from_request(&fixed_req) })
    })
}

struct RunResult {
    status: &'static str,
    error: Option<String>,
    raw_response: Option<String>,
    parsed: Option<Value>,
    cleaned: Vec<String>,
    latency_ms: Option<i64>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
}

const REASONING_FALLBACK: &str = "answer taken from reasoning_content";

/// 插件分支（§10.1）：一次 `POST /lab/reverse` → 与 openai 分支同形的 `ChatOutcome`：`text` 为模型原文、
/// `latency_ms` 取插件回的 `latency_ms`（缺则用本地耗时）、token 数为 None、`from_reasoning=false`。
/// 非 2xx → `plugin_rejected`（正文前 500 字，含 400 provider 不存在 / 413 图太大）；连不上 → `network`。
async fn call_plugin(ctx: &BatchCtx, body: &Value) -> AppResult<client::ChatOutcome> {
    let session = ctx.plugin.as_ref().ok_or_else(|| AppError::new("internal", "插件连接未初始化（批次里有 kind=plugin 的端点但没有登录信息）"))?;
    let started = Instant::now();
    let (status, text) = session
        .request(&ctx.http, reqwest::Method::POST, &plugin_reverse_route(), &[], Some(body), Duration::from_secs(PLUGIN_REVERSE_TIMEOUT_SECS))
        .await?;
    let elapsed = started.elapsed().as_millis() as i64;
    if !status.is_success() {
        return Err(rejected(status, &text));
    }
    let raw: Value = serde_json::from_str(&text).map_err(|e| AppError::plugin_rejected(format!("响应不是 JSON: {e}: {}", truncate(&text, 200))))?;
    if let Some(e) = crate::plugin::plugin_error(&raw) {
        return Err(e);
    }
    let data = plugin_data(&raw);
    let answer = data
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::plugin_rejected(format!("插件返回里没有 text: {}", truncate(&text, 200))))?
        .to_string();
    let latency_ms = data.get("latency_ms").and_then(Value::as_i64).unwrap_or(elapsed);
    Ok(client::ChatOutcome { text: answer, from_reasoning: false, prompt_tokens: None, completion_tokens: None, latency_ms, raw })
}

async fn call_provider(
    ctx: &BatchCtx,
    prepared: &Prepared,
    compiled: &CompiledRuleset,
    q: &QuestionSpec,
    provider: &ReverseProvider,
    key: &str,
    img: &client::PreparedImage,
) -> RunResult {
    // 取消令牌对两个分支同样生效：取消即丢弃响应（§10.1）
    let outcome = if provider.is_plugin() {
        let pid = provider.plugin_provider_id.as_deref().unwrap_or("");
        let body = build_plugin_reverse_body(&prepared.template.body, img, pid, provider.max_tokens);
        tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => Err(AppError::new("cancelled", "已取消")),
            r = call_plugin(ctx, &body) => r,
        }
    } else {
        let messages = client::build_messages(&prepared.template.body, img);
        tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => Err(AppError::new("cancelled", "已取消")),
            r = client::chat(&ctx.http, &provider.base_url, key, &provider.model, provider.max_tokens, messages) => r,
        }
    };
    match outcome {
        Err(e) => RunResult {
            status: "error",
            error: Some(e.message),
            raw_response: None,
            parsed: None,
            cleaned: vec![],
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
        },
        Ok(o) => {
            if o.from_reasoning {
                ctx.db.log_event(
                    "warn",
                    "reverse",
                    REASONING_FALLBACK,
                    json!({ "batch_id": prepared.batch_id, "question_id": q.question_id(), "provider_id": provider.provider_id, "model": provider.model }),
                );
            }
            let parsed = client::extract_json(&o.text);
            let (status, error, cleaned, parsed) = match parsed {
                None => ("error", Some("响应里找不到 JSON".to_string()), vec![], None),
                Some(p) => {
                    let tags = client::tags_field(&p);
                    let reason = p.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                    let framing = p.get("framing").and_then(|f| f.as_str()).unwrap_or("");
                    let suitable = p.get("suitable").and_then(|s| s.as_bool()).unwrap_or(true);
                    let blob = format!("{tags} {reason}");
                    let refusal = compiled.refusal.find(&blob).map(|m| m.as_str().to_string());
                    let cleaned = compiled.clean_tags(&tags, "", framing);
                    if !suitable {
                        ("refused", Some(format!("suitable=false: {reason}")), cleaned, Some(p))
                    } else if let Some(hit) = refusal {
                        ("refused", Some(format!("含拒答语「{hit}」")), cleaned, Some(p))
                    } else {
                        ("ok", None, cleaned, Some(p))
                    }
                }
            };
            // status 照常，只在 error 里留痕；原本就有错误信息的拼在后面
            let error = if o.from_reasoning {
                Some(match error {
                    Some(e) => format!("{REASONING_FALLBACK}; {e}"),
                    None => REASONING_FALLBACK.to_string(),
                })
            } else {
                error
            };
            RunResult {
                status,
                error,
                raw_response: Some(o.text),
                parsed,
                cleaned,
                latency_ms: Some(o.latency_ms),
                prompt_tokens: o.prompt_tokens,
                completion_tokens: o.completion_tokens,
            }
        }
    }
}

/// `request_json` 里的 (base_url, model)：本地端点照填；插件端点填插件地址与 `plugin:<provider_id>`（§10.1）。
fn request_target(provider: &ReverseProvider, plugin_base: Option<&str>) -> (String, String) {
    if provider.is_plugin() {
        (plugin_base.unwrap_or("").to_string(), plugin_model_label(provider.plugin_provider_id.as_deref().unwrap_or("")))
    } else {
        (provider.base_url.clone(), provider.model.clone())
    }
}

fn insert_run(
    db: &Db,
    prepared: &Prepared,
    compiled: &CompiledRuleset,
    q: &QuestionSpec,
    provider: &ReverseProvider,
    plugin_base: Option<&str>,
    img: &client::PreparedImage,
    res: &RunResult,
) -> AppResult<i64> {
    let (base_url, model) = request_target(provider, plugin_base);
    let mut request = client::build_request_log(&prepared.template.body, img, &base_url, &model, provider.max_tokens);
    if let Some(plan) = prepared.render.as_ref() {
        // §11.4：批次的重画参数记在 request_json.render，单条重画时沿用角色与固定种子
        request["render"] = serde_json::to_value(plan)?;
    }
    db.with(|c| {
        let question = match q {
            QuestionSpec::Pack { entry_id, ordinal, .. } => store::question_row(c, entry_id, *ordinal)?,
            QuestionSpec::Local { .. } => None,
        };
        let (source_kind, source_ref, ordinal) = match q {
            QuestionSpec::Pack { entry_id, ordinal, .. } => ("pack_image", entry_id.clone(), Some(*ordinal)),
            QuestionSpec::Local { path } => ("local_file", path.to_string_lossy().to_string(), None),
        };
        let truth_source = question.as_ref().map(|r| metrics::truth_source(r.raw_tag.as_deref(), &r.character_prompts));
        // §11.4：本地图片题 none；图包题看真值文本（COALESCE(raw_tag, tags)）是不是散文
        let truth_kind = match q {
            QuestionSpec::Local { .. } => TRUTH_KIND_NONE,
            QuestionSpec::Pack { .. } => question.as_ref().map(|r| metrics::truth_kind_of(r.raw_tag.as_deref(), &r.tags)).unwrap_or(TRUTH_KIND_TAGS),
        };
        // §11.4：泄漏数任何 truth_kind 都按 pred_raw 算（解析失败 → 0）
        let pred_raw = res.parsed.as_ref().map(|p| metrics::pred_raw_from_tags(&client::tags_field(p))).unwrap_or_default();
        let (leak_appearance, leak_banned) = metrics::leaks(&pred_raw, compiled);
        c.execute(
            "INSERT INTO reverse_run(batch_id, created_at, source_kind, source_ref, image_ordinal, image_sha256, truth_source, template_id, ruleset_id, provider_id,
               request_json, raw_response, parsed_json, cleaned_tags_json, latency_ms, prompt_tokens, completion_tokens, status, error, truth_kind, leak_appearance, leak_banned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                prepared.batch_id,
                now_iso(),
                source_kind,
                source_ref,
                ordinal,
                img.sha256,
                truth_source,
                prepared.template.template_id,
                prepared.ruleset.ruleset_id,
                provider.provider_id,
                request.to_string(),
                res.raw_response,
                res.parsed.as_ref().map(|p| p.to_string()),
                serde_json::to_string(&res.cleaned)?,
                res.latency_ms,
                res.prompt_tokens,
                res.completion_tokens,
                res.status,
                res.error,
                truth_kind,
                leak_appearance,
                leak_banned,
            ],
        )?;
        let run_id = c.last_insert_rowid();
        // 指标只给 truth_kind='tags' 的图包题（§11.4：散文题 metric=null，本地图片题从来没有）
        if truth_kind == TRUTH_KIND_TAGS {
            if let (Some(row), Some(parsed)) = (question.as_ref(), res.parsed.as_ref()) {
                let truth = metrics::build_truth(
                    &metrics::TruthInput { raw_tag: row.raw_tag.as_deref(), tags: &row.tags, character_prompts: &row.character_prompts, negative: row.negative.as_deref() },
                    compiled,
                    character_aliases(),
                );
                let framing = parsed.get("framing").and_then(|f| f.as_str()).unwrap_or("");
                let pred = metrics::pred_from_cleaned(&res.cleaned, framing);
                let d = metrics::compute(&truth, &pred_raw, &pred, compiled);
                let m = &d.metric;
                c.execute(
                    "INSERT INTO reverse_metric(run_id, truth_json, truth_eval_json, expected_miss_json, hit, missed, extra, expected_hit, precision, recall, f1,
                       appearance_leak, banned_leak, ruleset_drop, contradiction)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        run_id,
                        serde_json::to_string(&truth.truth)?,
                        serde_json::to_string(&truth.truth_eval)?,
                        serde_json::to_string(&truth.expected_miss)?,
                        m.hit,
                        m.missed,
                        m.extra,
                        m.expected_hit,
                        m.precision,
                        m.recall,
                        m.f1,
                        m.appearance_leak,
                        m.banned_leak,
                        m.ruleset_drop,
                        m.contradiction,
                    ],
                )?;
            }
        }
        Ok(run_id)
    })
}

pub async fn run(ctx: Arc<BatchCtx>, prepared: Prepared, lease: BatchLease) {
    let total = (prepared.questions.len() * prepared.providers.len()) as i64;
    let batch_id = prepared.batch_id.clone();
    let emit = |run_id: Option<i64>, status: &str, done: i64, message: Option<String>| {
        (ctx.emit)(RunEvent { batch_id: batch_id.clone(), run_id, status: status.to_string(), done, total, message });
    };
    ctx.db.log_event("info", "reverse", "batch 开始", json!({ "batch_id": batch_id, "total": total, "template": prepared.template.template_id, "ruleset": prepared.ruleset.ruleset_id }));
    emit(None, "started", 0, None);
    let compiled = match CompiledRuleset::compile(&prepared.ruleset) {
        Ok(c) => c,
        Err(e) => {
            let status = lease.complete(|cancelled| {
                let status = if cancelled { "cancelled" } else { "failed" };
                finish(&ctx, &batch_id, status);
                status
            });
            emit(None, if status == "cancelled" { "batch_cancelled" } else { "batch_failed" }, 0, Some(e.message));
            return;
        }
    };
    let plugin_base = ctx.plugin.as_ref().map(|s| s.base().to_string());
    let mut done = 0i64;
    let mut failed = 0i64;
    'questions: for q in &prepared.questions {
        if ctx.cancel.is_cancelled() {
            break;
        }
        let path = q.image_path().clone();
        let prepared_image = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => break,
            result = tokio::task::spawn_blocking(move || client::prepare_image(&path)) => result,
        };
        if ctx.cancel.is_cancelled() { break; }
        let img = match prepared_image {
            Ok(Ok(i)) => Arc::new(i),
            Ok(Err(e)) => {
                for _ in &prepared.providers {
                    done += 1;
                    failed += 1;
                    emit(None, "run_failed", done, Some(format!("{}: {}", q.question_id(), e.message)));
                }
                continue;
            }
            Err(e) => {
                done += prepared.providers.len() as i64;
                failed += prepared.providers.len() as i64;
                emit(None, "run_failed", done, Some(e.to_string()));
                continue;
            }
        };
        let futures = prepared.providers.iter().map(|(p, key)| {
            let img = img.clone();
            let ctx = ctx.clone();
            let prepared = &prepared;
            let compiled = &compiled;
            async move {
                let res = call_provider(&ctx, prepared, compiled, q, p, key, &img).await;
                (p, res, img)
            }
        });
        // 收到一个响应就保存一个，慢端点不会挡住其他端点落库；停止时丢弃未完成的 future。
        let mut pending: FuturesUnordered<_> = futures.collect();
        let mut renders = Vec::new();
        loop {
            let next = tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => break 'questions,
                result = pending.next() => result,
            };
            let Some((p, res, img)) = next else { break; };
            if ctx.cancel.is_cancelled() { break 'questions; }
            done += 1;
            match insert_run(&ctx.db, &prepared, &compiled, q, p, plugin_base.as_deref(), &img, &res) {
                Ok(run_id) => {
                    if res.status == "error" {
                        failed += 1;
                        emit(Some(run_id), "run_failed", done, res.error.clone());
                    } else {
                        emit(Some(run_id), "run_done", done, Some(format!("{} · {} · {}", q.question_id(), p.name, res.status)));
                        // 同题端点响应都已保存后再串行重画，避免慢重画导致其他已返回响应丢失。
                        if prepared.render.is_some() && res.status == "ok" && !res.cleaned.is_empty() {
                            renders.push((run_id, p, res, img));
                        }
                    }
                }
                Err(e) => {
                    failed += 1;
                    emit(None, "run_failed", done, Some(format!("落库失败: {}", e.message)));
                }
            }
        }
        if let Some(plan) = prepared.render.as_ref() {
            // 固定按配置端点顺序重画；保存响应可以按返回顺序，排队顺序不因此变动。
            renders.sort_by_key(|(_, p, _, _)| prepared.providers.iter().position(|(provider, _)| provider.provider_id == p.provider_id));
            for (run_id, p, res, img) in renders {
                if ctx.cancel.is_cancelled() { break 'questions; }
                let framing = res.parsed.as_ref().and_then(|v| v.get("framing")).and_then(Value::as_str);
                let job = RenderJob { run_id, cleaned: &res.cleaned, framing, plan, original: Some(&img), style_source: Some(q.style_source()) };
                let outcome = tokio::select! {
                    biased;
                    _ = ctx.cancel.cancelled() => None,
                    r = render::render_and_store(&ctx.db, &ctx.http, ctx.plugin.as_deref(), &ctx.data_dir, &ctx.style, job) => Some(r),
                };
                match outcome {
                    Some(Ok(d)) => emit(
                        Some(run_id),
                        "run_done",
                        done,
                        Some(format!(
                            "{} · {} · 重画完成 seed={} 质量={} 相似={} 候选={}{}",
                            q.question_id(),
                            p.name,
                            fmt_opt(d.outcome.seed),
                            fmt_opt(d.outcome.quality_score),
                            fmt_opt(d.outcome.similarity_score),
                            d.outcome.images.len(),
                            style_note(d.style.as_ref(), &plan.style.style_fallback)
                        )),
                    ),
                    Some(Err(e)) => emit(Some(run_id), "run_done", done, Some(format!("{} · {} · 重画失败: {e}", q.question_id(), p.name))),
                    None => {
                        render::record_error(&ctx.db, run_id, "已停止（已保存的结果保留）");
                        break 'questions;
                    }
                }
            }
        }
    }
    let status = lease.complete(|cancelled| {
        let status = if cancelled { "cancelled" } else if failed == total { "failed" } else { "done" };
        finish(&ctx, &batch_id, status);
        status
    });
    ctx.db.log_event("info", "reverse", "batch 结束", json!({ "batch_id": batch_id, "status": status, "done": done, "failed": failed }));
    if status == "cancelled" {
        emit(None, "batch_cancelled", done, Some(format!("已停止，已保存反推结果 {done}/{total}")));
    } else if status == "failed" {
        emit(None, "batch_failed", done, Some("全部运行失败".into()));
    } else {
        emit(None, "batch_done", done, Some(format!("完成 {done}/{total}，失败 {failed}")));
    }
}

fn fmt_opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

/// §12.7 `run_done` 文字末尾：用到检索画风 → 「 画风=<标题>(<相似度>)」；兜底 → 「 画风=兜底(<pool|none>)」；非 style 模式空串。
pub fn style_note(style: Option<&render::StyleChoice>, fallback: &str) -> String {
    match style {
        Some(s) if s.used() => format!(" 画风={}({:.2})", s.title.as_deref().unwrap_or("?"), s.score.unwrap_or(0.0)),
        Some(_) => format!(" 画风=兜底({fallback})"),
        None => String::new(),
    }
}

fn finish(ctx: &BatchCtx, batch_id: &str, status: &str) {
    let _ = ctx.db.with(|c| {
        c.execute("UPDATE reverse_batch SET status = ?2, finished_at = ?3 WHERE batch_id = ?1", params![batch_id, status, now_iso()])?;
        Ok(())
    });
}

#[cfg(test)]
#[path = "batch_cancel_tests.rs"]
mod cancel_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn req_of(question_ids: Vec<String>, provider_ids: Vec<i64>, note: Option<&str>) -> RunBatchRequest {
        RunBatchRequest {
            style_model: None,
            question_ids,
            template_id: 1,
            ruleset_id: 1,
            provider_ids,
            note: note.map(str::to_string),
            render: false,
            render_character: None,
            render_seed: None,
            judge_provider_id: None,
            artist: None,
            render_artist_mode: "pool".into(),
            render_count: None,
            style_top_k: 5,
            style_decisions: vec![],
            style_min_score: None,
            style_fallback: "pool".into(),
            style_center: true,
        }
    }

    #[test]
    fn question_id_parsing() {
        assert_eq!(parse_question_id("local:/tmp/a.png").unwrap().0.as_deref(), Some("/tmp/a.png"));
        let (_, pack) = parse_question_id("nai5_community_pack_mengshen_0001#2").unwrap();
        assert_eq!(pack, Some(("nai5_community_pack_mengshen_0001".to_string(), 2)));
        assert!(parse_question_id("no-hash").is_err());
        assert!(parse_question_id("x#y").is_err());
    }

    #[test]
    fn prepare_validates_and_inserts_batch() {
        let db = Db::open_memory().unwrap();
        let data = tempfile::tempdir().unwrap();
        let dd = data.path();
        db.with(|c| super::super::builtin::ensure_builtin(c)).unwrap();
        let req = req_of(vec!["a#0".into()], vec![1], None);
        assert!(prepare(&db, dd, &req).is_err(), "没有 provider 应报错");
        db.with(|c| {
            store::provider_save(
                c,
                &ReverseProviderInput {
                    provider_id: None,
                    name: "p".into(),
                    base_url: "http://127.0.0.1:1/v1".into(),
                    model: "m".into(),
                    max_tokens: 100,
                    enabled: true,
                    kind: PROVIDER_KIND_OPENAI.into(),
                    plugin_provider_id: None,
                },
            )
            .map(|_| ())
        })
        .unwrap();
        assert!(prepare(&db, dd, &req).is_err(), "题目不存在应报错");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0])).save(&p).unwrap();
        let req2 = req_of(vec![format!("local:{}", p.display())], vec![1], Some("n"));
        let prepared = prepare(&db, dd, &req2).unwrap();
        assert_eq!(prepared.questions.len(), 1);
        assert_eq!(prepared.questions[0].style_source(), StyleSearchSource::Path { path: p.to_string_lossy().to_string() }, "§12.7 本地题的检索来源");
        assert!(prepared.batch_id.starts_with("b-"));
        assert!(prepared.render.is_none(), "render=false 不重画");
        let batches = db.with(|c| store::batches_list(c)).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].status, "running");
        assert_eq!(batches[0].template, "plugin v1");
        assert_eq!(batches[0].providers, vec!["p"]);

        // §11.4：render=true → 计划带缺省角色与评委；负种子拒绝；不重画时不看种子（每次 prepare 都插一行 reverse_batch）
        let with_render = RunBatchRequest { render: true, render_character: Some(" ".into()), render_seed: Some(3), judge_provider_id: Some("gpt".into()), ..req2.clone() };
        let plan = prepare(&db, dd, &with_render).unwrap().render.expect("render=true 应有计划");
        assert_eq!((plan.character.as_str(), plan.seed, plan.judge_provider_id.as_deref()), ("薇拉", Some(3), Some("gpt")));
        let bad_seed = RunBatchRequest { render_seed: Some(-1), ..with_render.clone() };
        assert_eq!(prepare(&db, dd, &bad_seed).err().map(|e| e.code), Some("bad_request".to_string()));
        assert!(prepare(&db, dd, &RunBatchRequest { render: false, ..bad_seed }).is_ok(), "不重画时不看种子");
        assert_eq!(db.with(|c| store::batches_list(c)).unwrap().len(), 3, "两次成功的 prepare 各插一行，被拒的不插");
        // §12.6：artist_mode 非法 / given 没画师 / count 越界 → bad_request；空串 mode 归一成 pool；不重画时同样不看
        let bad_mode = RunBatchRequest { render_artist_mode: "llm".into(), ..with_render.clone() };
        assert_eq!(prepare(&db, dd, &bad_mode).err().map(|e| e.code), Some("bad_request".to_string()));
        let given_no_artist = RunBatchRequest { render_artist_mode: "given".into(), artist: Some(" ".into()), ..with_render.clone() };
        assert_eq!(prepare(&db, dd, &given_no_artist).err().map(|e| e.code), Some("bad_request".to_string()));
        let bad_count = RunBatchRequest { render_count: Some(5), ..with_render.clone() };
        assert_eq!(prepare(&db, dd, &bad_count).err().map(|e| e.code), Some("bad_request".to_string()));
        assert!(prepare(&db, dd, &RunBatchRequest { render: false, ..bad_mode.clone() }).is_ok(), "不重画时不看 artist_mode");
        let ok_given = RunBatchRequest { render_artist_mode: " Given ".into(), artist: Some("artist:foo".into()), render_count: Some(4), ..with_render.clone() };
        let plan = prepare(&db, dd, &ok_given).unwrap().render.unwrap();
        assert_eq!((plan.artist_mode.as_str(), plan.artist.as_deref(), plan.count), ("given", Some("artist:foo"), Some(4)));
        let plan = prepare(&db, dd, &RunBatchRequest { render_artist_mode: "".into(), ..with_render.clone() }).unwrap().render.unwrap();
        assert_eq!((plan.artist_mode.as_str(), plan.count), ("pool", None), "空串 mode → pool");
        assert_eq!(db.with(|c| store::batches_list(c)).unwrap().len(), 6, "又三次成功的 prepare");

        // §12.7：style 模式——style_* 参数校验；模型未下载 / 索引为空 → bad_request 且不插批次行；就绪后计划带归一后的 style 参数；不重画 / 别的模式不查索引
        let style_req = RunBatchRequest {
            render_artist_mode: " Style ".into(),
            style_top_k: 3,
            style_decisions: vec!["Include".into(), " undecided ".into()],
            style_min_score: Some(0.3),
            style_fallback: " None ".into(),
            style_center: false,
            ..with_render.clone()
        };
        for bad in [
            RunBatchRequest { style_top_k: 0, ..style_req.clone() },
            RunBatchRequest { style_top_k: 51, ..style_req.clone() },
            RunBatchRequest { style_decisions: vec!["maybe".into()], ..style_req.clone() },
            RunBatchRequest { style_fallback: "given".into(), ..style_req.clone() },
            RunBatchRequest { style_min_score: Some(1.5), ..style_req.clone() },
        ] {
            assert_eq!(prepare(&db, dd, &bad).err().map(|e| e.code), Some("bad_request".to_string()), "{bad:?}");
        }
        assert!(prepare(&db, dd, &RunBatchRequest { render: false, style_top_k: 0, ..style_req.clone() }).is_ok(), "不重画时不看 style_*");
        assert!(prepare(&db, dd, &RunBatchRequest { render_artist_mode: "pool".into(), style_top_k: 0, ..style_req.clone() }).is_ok(), "pool 模式不看 style_*、不查索引");
        let before = db.with(|c| store::batches_list(c)).unwrap().len();
        let err = prepare(&db, dd, &style_req).err().expect("模型未下载应报错");
        assert!(err.code == "bad_request" && err.message.contains("未下载"), "{err:?}");
        let mdir = crate::style::model::model_dir(dd, crate::style::model::MODEL_NAME);
        std::fs::create_dir_all(&mdir).unwrap();
        std::fs::write(mdir.join("config.json"), b"{}").unwrap();
        std::fs::write(mdir.join("model.safetensors"), b"weights").unwrap();
        let err = prepare(&db, dd, &style_req).err().expect("索引为空应报错");
        assert!(err.code == "bad_request" && err.message.contains("索引为空"), "{err:?}");
        assert_eq!(db.with(|c| store::batches_list(c)).unwrap().len(), before, "索引未就绪的 style 批次不插行");
        db.with(|c| {
            crate::sync::upsert::ensure_release(c, "r-1", "p", "h", "f", "codex_raw/r-1")?;
            c.execute("INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('artist_nai5_personal','artist','画师','1','[]')", [])?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release) VALUES ('a_foo','artist_nai5_personal',0,'a_foo','[]','artist:foo','safe','r-1','r-1')",
                [],
            )?;
            crate::style::store::upsert_now(c, "a_foo", 0, crate::style::model::MODEL_NAME, &[1.0, 0.0], None)
        })
        .unwrap();
        let plan = prepare(&db, dd, &style_req).unwrap().render.unwrap();
        assert_eq!((plan.artist_mode.as_str(), plan.artist, plan.count), ("style", None, None));
        assert_eq!(
            plan.style,
            render::StyleParams { model: Some("clip-vit-base-patch32".into()), style_top_k: 3, style_decisions: vec!["include".into(), "undecided".into()], style_min_score: Some(0.3), style_fallback: "none".into(), style_center: false },
            "归一后的 style 参数进计划"
        );
        assert_eq!(db.with(|c| store::batches_list(c)).unwrap().len(), before + 1);
        assert_eq!(style_note(None, "pool"), "");
        assert_eq!(style_note(Some(&render::StyleChoice { fallback: true, ..Default::default() }), "none"), " 画风=兜底(none)");
        let used = render::StyleChoice { title: Some("foo".into()), score: Some(0.834), artist: Some("artist:foo".into()), ..Default::default() };
        assert_eq!(style_note(Some(&used), "pool"), " 画风=foo(0.83)");

        // §10.1：插件端点不查钥匙串，key 为空串；缺 plugin_provider_id 的插件端点（直接改库造出来）→ bad_request
        let pp = db
            .with(|c| {
                store::provider_save(
                    c,
                    &ReverseProviderInput {
                        provider_id: None,
                        name: String::new(),
                        base_url: String::new(),
                        model: String::new(),
                        max_tokens: 900,
                        enabled: true,
                        kind: PROVIDER_KIND_PLUGIN.into(),
                        plugin_provider_id: Some("gpt-4o".into()),
                    },
                )
            })
            .unwrap();
        let req3 = req_of(req2.question_ids.clone(), vec![1, pp.provider_id], None);
        let prepared = prepare(&db, dd, &req3).unwrap();
        assert_eq!(prepared.providers.len(), 2);
        let (plugin_provider, key) = &prepared.providers[1];
        assert!(plugin_provider.is_plugin());
        assert_eq!(key, "", "插件端点没有 key");
        assert_eq!(request_target(plugin_provider, Some("http://127.0.0.1:6185")), ("http://127.0.0.1:6185".to_string(), "plugin:gpt-4o".to_string()));
        assert_eq!(request_target(&prepared.providers[0].0, Some("http://127.0.0.1:6185")), ("http://127.0.0.1:1/v1".to_string(), "m".to_string()));
        db.with(|c| {
            c.execute("UPDATE reverse_provider SET plugin_provider_id = '' WHERE provider_id = ?1", params![pp.provider_id])?;
            Ok(())
        })
        .unwrap();
        let err = prepare(&db, dd, &req3).err().expect("缺 plugin_provider_id 应报错");
        assert_eq!(err.code, "bad_request");
    }

    /// §11.4：`insert_run` 按真值来源写 truth_kind（图包标签题 tags、散文题 prose、本地图片 none），泄漏数任何题都按 pred_raw 算，
    /// 指标只给 tags 题；`batch_results` 的汇总只用 tags 题算 P/R/F1，泄漏数三题都计；`request_json.render` 记录重画参数。
    #[test]
    fn insert_run_sets_truth_kind_and_leaks() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0])).save(&p).unwrap();
        let local = p.to_string_lossy().to_string();
        db.with(|c| {
            super::super::builtin::ensure_builtin(c)?;
            store::provider_save(
                c,
                &ReverseProviderInput {
                    provider_id: None,
                    name: "p".into(),
                    base_url: "http://127.0.0.1:1/v1".into(),
                    model: "m".into(),
                    max_tokens: 100,
                    enabled: true,
                    kind: PROVIDER_KIND_OPENAI.into(),
                    plugin_provider_id: None,
                },
            )?;
            crate::sync::upsert::ensure_release(c, "r-1", "p", "h", "f", "codex_raw/r-1")?;
            c.execute("INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('nai5_community_pack','pack','图包','1','[]')", [])?;
            for (id, tags) in [("e_tags", "1girl, smile, sitting, long hair"), ("e_prose", "<scene>\nA girl sitting by the window.\n</scene>")] {
                c.execute(
                    "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                     VALUES (?1,'nai5_community_pack',0,?1,'[]',?2,'safe','r-1','r-1')",
                    params![id, tags],
                )?;
                c.execute("INSERT INTO entry_image(entry_id, ordinal, image_file, local_path) VALUES (?1, 0, 'x.png', ?2)", params![id, local])?;
            }
            Ok(())
        })
        .unwrap();
        let req = RunBatchRequest { render: true, render_character: Some("阿莉亚".into()), render_seed: Some(9), ..req_of(vec!["e_tags#0".into(), "e_prose#0".into(), format!("local:{local}")], vec![1], None) };
        let prepared = prepare(&db, dir.path(), &req).unwrap();
        assert_eq!(prepared.questions[0].style_source(), StyleSearchSource::Entry { entry_id: "e_tags".into(), ordinal: 0 }, "§12.7 词典题的检索来源");
        let compiled = CompiledRuleset::compile(&prepared.ruleset).unwrap();
        // 题库 `Question.prose`（前端 v0.2.4 追加的可选键，联调时后端补上）与 insert_run 的 truth_kind 同一判定
        let qs = db.with(|c| store::questions(c, &QuestionQuery { codex_id: Some("nai5_community_pack".into()), ..Default::default() }, &compiled)).unwrap();
        let prose_of = |id: &str| qs.iter().find(|q| q.question_id == format!("{id}#0")).map(|q| q.prose).expect("题库应有该题");
        assert!(!prose_of("e_tags") && prose_of("e_prose"), "Question.prose 按 §11.1 判");
        let img = client::prepare_image(&p).unwrap();
        let parsed = json!({ "suitable": true, "tags": "smile, long hair, realistic, standing", "framing": "cowboy shot" });
        let res = RunResult {
            status: "ok",
            error: None,
            raw_response: Some(parsed.to_string()),
            parsed: Some(parsed.clone()),
            cleaned: vec!["smile".into(), "standing".into()],
            latency_ms: Some(10),
            prompt_tokens: None,
            completion_tokens: None,
        };
        let (provider, _) = &prepared.providers[0];
        let ids: Vec<i64> = prepared.questions.iter().map(|q| insert_run(&db, &prepared, &compiled, q, provider, None, &img, &res).unwrap()).collect();
        let results = db.with(|c| store::batch_results(c, &prepared.batch_id)).unwrap();
        assert_eq!(results.runs.len(), 3);
        let by_id = |id: i64| results.runs.iter().find(|r| r.run_id == id).unwrap();
        let tags_run = by_id(ids[0]);
        assert_eq!(tags_run.truth_kind, TRUTH_KIND_TAGS);
        assert!(tags_run.metric.is_some(), "标签题有指标");
        assert_eq!((tags_run.leak_appearance, tags_run.leak_banned), (1, 1), "long hair 外貌 / realistic 禁词");
        assert_eq!(tags_run.metric.as_ref().map(|m| (m.appearance_leak, m.banned_leak)), Some((1, 1)), "与 metric 里的一致");
        let prose_run = by_id(ids[1]);
        assert_eq!(prose_run.truth_kind, TRUTH_KIND_PROSE);
        assert!(prose_run.metric.is_none(), "散文题 metric=null");
        assert_eq!((prose_run.leak_appearance, prose_run.leak_banned), (1, 1), "泄漏数照算");
        let local_run = by_id(ids[2]);
        assert_eq!(local_run.truth_kind, TRUTH_KIND_NONE);
        assert!(local_run.metric.is_none());
        assert_eq!((local_run.leak_appearance, local_run.leak_banned), (1, 1));
        assert!(local_run.render_path.is_none() && local_run.render_error.is_none() && local_run.quality_notes.is_empty());
        let n: i64 = db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM reverse_metric", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 1, "库里 prose / none 题没有 reverse_metric 行");
        let s = &results.summary[0];
        assert_eq!((s.runs, s.ok, s.tags_n, s.render_n), (3, 3, 1, 0));
        assert_eq!((s.style_hit_n, s.style_score_avg), (0, None), "§12.7 没重画 → 0 / null");
        assert!(s.f1.is_some() && s.precision.is_some() && s.recall.is_some(), "只有标签题参与 F1");
        assert!((s.precision.unwrap() - 1.0 / 3.0).abs() < 1e-9 && (s.recall.unwrap() - 0.5).abs() < 1e-9, "{s:?}");
        assert_eq!((s.appearance_leak, s.banned_leak), (3, 3), "泄漏数三题都计");
        assert_eq!((s.quality_avg, s.similarity_avg), (None, None));
        // run_get 对散文题：metric 为空、四色集合为空但不报错；request_json.render 记了重画参数
        let detail = db.with(|c| store::run_get(c, ids[1])).unwrap();
        assert!(detail.run.metric.is_none() && detail.hits.is_empty() && detail.misses.is_empty());
        assert_eq!(detail.run.truth_kind, "prose");
        assert_eq!(
            detail.request["render"],
            json!({ "character": "阿莉亚", "seed": 9, "judge_provider_id": null, "artist": null, "artist_mode": "pool", "count": null,
                    "style_model": null, "style_top_k": 5, "style_decisions": [], "style_min_score": null, "style_fallback": "pool", "style_center": true }),
            "§12.7 五个 style_* 也记进 request_json.render"
        );
        assert_eq!(RenderPlan::from_request_log(&detail.request), prepared.render.clone().unwrap());
        let target = db.with(|c| store::render_target(c, ids[0])).unwrap();
        assert_eq!((target.cleaned.len(), target.framing.as_deref(), target.image_path.as_deref()), (2, Some("cowboy shot"), Some(local.as_str())), "图包题的原图路径来自 entry_image.local_path");
        // 解析失败的 run：泄漏数 0、没有指标
        let err_res = RunResult { status: "error", error: Some("x".into()), raw_response: None, parsed: None, cleaned: vec![], latency_ms: None, prompt_tokens: None, completion_tokens: None };
        let err_id = insert_run(&db, &prepared, &compiled, &prepared.questions[0], provider, None, &img, &err_res).unwrap();
        let row = db.with(|c| store::run_row(c, err_id)).unwrap();
        assert_eq!((row.truth_kind.as_str(), row.leak_appearance, row.leak_banned, row.metric.is_none()), ("tags", 0, 0, true));
        // §12.7 汇总：三条 run 分别「用到检索画风」「兜底（记了最像那条）」「无命中兜底」→ style_hit_n 只数第一条，均值只算它的分
        db.with(|c| {
            c.execute("UPDATE reverse_run SET render_style_entry_id = 'a_foo', render_style_title = 'foo', render_style_score = 0.9, render_style_decision = 'include', render_style_fallback = 0, render_artist_source = 'style' WHERE run_id = ?1", params![ids[0]])?;
            c.execute("UPDATE reverse_run SET render_style_entry_id = 'a_bar', render_style_title = 'bar', render_style_score = 0.2, render_style_decision = 'exclude', render_style_fallback = 1, render_artist_source = 'pool' WHERE run_id = ?1", params![ids[1]])?;
            c.execute("UPDATE reverse_run SET render_style_fallback = 1, render_artist_source = 'none' WHERE run_id = ?1", params![ids[2]])?;
            Ok(())
        })
        .unwrap();
        let results = db.with(|c| store::batch_results(c, &prepared.batch_id)).unwrap();
        let s = &results.summary[0];
        assert_eq!((s.runs, s.style_hit_n), (4, 1));
        assert!((s.style_score_avg.unwrap() - 0.9).abs() < 1e-9, "{s:?}");
        let by_id = |id: i64| results.runs.iter().find(|r| r.run_id == id).unwrap();
        assert_eq!((by_id(ids[0]).render_style_entry_id.as_deref(), by_id(ids[0]).render_style_fallback, by_id(ids[0]).render_artist_source.as_deref()), (Some("a_foo"), false, Some("style")));
        assert_eq!((by_id(ids[1]).render_style_score, by_id(ids[1]).render_style_fallback), (Some(0.2), true));
        assert!(by_id(ids[2]).render_style_entry_id.is_none() && by_id(ids[2]).render_style_fallback);
        db.with(|c| {
            c.execute("UPDATE reverse_run SET render_style_entry_id = 'a_baz', render_style_score = 0.7, render_style_fallback = 0 WHERE run_id = ?1", params![ids[2]])?;
            Ok(())
        })
        .unwrap();
        let s = db.with(|c| store::batch_results(c, &prepared.batch_id)).unwrap().summary.remove(0);
        assert_eq!(s.style_hit_n, 2);
        assert!((s.style_score_avg.unwrap() - 0.8).abs() < 1e-9, "{s:?}");
    }

    /// §10.1：从 `PreparedImage` 组 `/lab/reverse` 请求体，不联网。键名照契约、base64 能解回同一份字节、mime 与 prompt 正确；
    /// `request_json` 用 `plugin:<provider_id>` 与插件地址，且不含 base64。
    #[test]
    fn plugin_reverse_body_from_prepared_image() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.png");
        image::RgbImage::from_pixel(7, 5, image::Rgb([1, 2, 3])).save(&p).unwrap();
        let img = client::prepare_image(&p).unwrap();
        let body = build_plugin_reverse_body("sys {extra} body", &img, "gpt-4o", 900);
        let obj = body.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["image_base64", "max_tokens", "mime", "prompt", "provider_id"], "键名照 §10.1，不多不少");
        assert_eq!(body["provider_id"], "gpt-4o");
        assert_eq!(body["prompt"], "sys  body", "{{extra}} 替换为空串，与 openai 分支的 system 消息同文");
        assert_eq!(body["prompt"], client::build_messages("sys {extra} body", &img)[0]["content"]);
        assert_eq!(body["mime"], "image/png");
        assert_eq!(body["max_tokens"], 900);
        let b64 = body["image_base64"].as_str().unwrap();
        assert_eq!(b64.len(), (img.bytes.len() + 2) / 3 * 4, "标准 base64（带 = 填充）长度");
        assert!(!b64.starts_with("data:"), "不是 data URL，只有正文");
        let back = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).unwrap();
        assert_eq!(back, img.bytes, "解回来就是 prepare_image 的同一份字节");
        assert_eq!(back, std::fs::read(&p).unwrap(), "小图不缩放，原文件字节");
        assert_eq!(plugin_reverse_route(), "/api/plug/astrbot_plugin_vivian/page/lab/reverse");
        assert_eq!(plugin_model_label("gpt-4o"), "plugin:gpt-4o");
        let log = client::build_request_log("sys {extra} body", &img, "http://127.0.0.1:6185", &plugin_model_label("gpt-4o"), 900);
        assert_eq!((log["base_url"].as_str(), log["model"].as_str()), (Some("http://127.0.0.1:6185"), Some("plugin:gpt-4o")));
        assert_eq!(log["messages"][0]["content"], "sys  body");
        assert!(!log.to_string().contains(b64), "request_json 不存 base64");
    }
}
