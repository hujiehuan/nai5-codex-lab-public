//! 反推实验室的数据层：模板/规则集（同名 version+1 永不覆盖）、模型端点（key 只在钥匙串）、
//! 题库（条目 × 图片 ordinal，带真值预览）、批次列表与结果汇总、单次运行详情（重算四色集合）、
//! 两批次按 question 对齐比较、人工评分。SQL 只读 `reverse_*` 与法典镜像表，不碰决策表。
//! v0.2.4（CONTRACT §11.4）：`RUN_SQL` 多读 truth_kind / render_* / 泄漏两列；汇总只用 truth_kind='tags' 的 run 算 P/R/F1，
//! 新增 tags_n / render_n / quality_avg / similarity_avg；`run_row` / `render_target` 给单条重画用。v0.3.0（§12.5）多读 `render_artist`；
//! §12.6 再多读 render_artist_source / render_size / render_size_source / plugin_dropped / plugin_would_reject / render_candidates 六列。
//! §12.7 再多读 render_style_entry_id / render_style_title / render_style_score / render_style_decision / render_style_fallback 五列，
//! 汇总多给 style_hit_n / style_score_avg（只算用到检索画风、没兜底的 run）。

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use serde_json::Value;

use crate::db::{json_str_vec, now_iso};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::secrets;

use super::builtin::character_aliases;
use super::clean::CompiledRuleset;
use super::metrics;

// ---------- 模板 ----------

fn template_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReverseTemplate> {
    Ok(ReverseTemplate {
        template_id: r.get(0)?,
        name: r.get(1)?,
        version: r.get(2)?,
        body: r.get(3)?,
        parent_id: r.get(4)?,
        created_at: r.get(5)?,
        note: r.get(6)?,
    })
}

const TEMPLATE_COLS: &str = "template_id, name, version, body, parent_id, created_at, note";

pub fn templates_list(conn: &Connection) -> AppResult<Vec<ReverseTemplate>> {
    let mut stmt = conn.prepare(&format!("SELECT {TEMPLATE_COLS} FROM reverse_template ORDER BY name, version DESC"))?;
    let rows = stmt.query_map([], template_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn template_get(conn: &Connection, id: i64) -> AppResult<ReverseTemplate> {
    conn.query_row(&format!("SELECT {TEMPLATE_COLS} FROM reverse_template WHERE template_id = ?1"), params![id], template_from_row)
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("模板不存在: {id}")))
}

pub fn template_save(conn: &Connection, input: &ReverseTemplateInput) -> AppResult<ReverseTemplate> {
    let name = input.name.trim();
    if name.is_empty() || input.body.trim().is_empty() {
        return Err(AppError::bad_request("模板 name/body 不能为空"));
    }
    let version: i64 = conn.query_row("SELECT COALESCE(MAX(version), 0) + 1 FROM reverse_template WHERE name = ?1", params![name], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO reverse_template(name, version, body, parent_id, created_at, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, version, input.body, input.parent_id, now_iso(), input.note],
    )?;
    template_get(conn, conn.last_insert_rowid())
}

// ---------- 规则集 ----------

const RULESET_COLS: &str = "ruleset_id, name, version, banned_json, appearance_regex, accessory_ok_regex, refusal_regex, always_drop_json, max_words, max_len, created_at, note";

fn ruleset_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReverseRuleset> {
    Ok(ReverseRuleset {
        ruleset_id: r.get(0)?,
        name: r.get(1)?,
        version: r.get(2)?,
        banned: json_str_vec(r.get(3)?),
        appearance_regex: r.get(4)?,
        accessory_ok_regex: r.get(5)?,
        refusal_regex: r.get(6)?,
        always_drop: json_str_vec(r.get(7)?),
        max_words: r.get(8)?,
        max_len: r.get(9)?,
        created_at: r.get(10)?,
        note: r.get(11)?,
    })
}

pub fn rulesets_list(conn: &Connection) -> AppResult<Vec<ReverseRuleset>> {
    let mut stmt = conn.prepare(&format!("SELECT {RULESET_COLS} FROM reverse_ruleset ORDER BY name, version DESC"))?;
    let rows = stmt.query_map([], ruleset_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn ruleset_get(conn: &Connection, id: i64) -> AppResult<ReverseRuleset> {
    conn.query_row(&format!("SELECT {RULESET_COLS} FROM reverse_ruleset WHERE ruleset_id = ?1"), params![id], ruleset_from_row)
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("规则集不存在: {id}")))
}

pub fn ruleset_save(conn: &Connection, input: &ReverseRulesetInput) -> AppResult<ReverseRuleset> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request("规则集 name 不能为空"));
    }
    let probe = ReverseRuleset {
        ruleset_id: 0,
        name: name.into(),
        version: 0,
        banned: input.banned.clone(),
        appearance_regex: input.appearance_regex.clone(),
        accessory_ok_regex: input.accessory_ok_regex.clone(),
        refusal_regex: input.refusal_regex.clone(),
        always_drop: input.always_drop.clone(),
        max_words: input.max_words,
        max_len: input.max_len,
        created_at: String::new(),
        note: None,
    };
    CompiledRuleset::compile(&probe)?;
    let version: i64 = conn.query_row("SELECT COALESCE(MAX(version), 0) + 1 FROM reverse_ruleset WHERE name = ?1", params![name], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO reverse_ruleset(name, version, banned_json, appearance_regex, accessory_ok_regex, refusal_regex, always_drop_json, max_words, max_len, created_at, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            name,
            version,
            serde_json::to_string(&input.banned)?,
            input.appearance_regex,
            input.accessory_ok_regex,
            input.refusal_regex,
            serde_json::to_string(&input.always_drop)?,
            input.max_words.max(1),
            input.max_len.max(1),
            now_iso(),
            input.note,
        ],
    )?;
    ruleset_get(conn, conn.last_insert_rowid())
}

// ---------- 模型端点 ----------

const PROVIDER_COLS: &str = "provider_id, name, base_url, model, max_tokens, enabled, created_at, kind, plugin_provider_id";

fn provider_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReverseProvider> {
    Ok(ReverseProvider {
        provider_id: r.get(0)?,
        name: r.get(1)?,
        base_url: r.get(2)?,
        model: r.get(3)?,
        max_tokens: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        has_key: false,
        created_at: r.get(6)?,
        kind: r.get(7)?,
        plugin_provider_id: r.get(8)?,
    })
}

/// `has_key`：插件端点不需要 key，恒 true（§10.1）；本地端点看钥匙串里有没有 `provider:<id>`。
fn fill_has_key(mut p: ReverseProvider) -> ReverseProvider {
    p.has_key = if p.is_plugin() { true } else { secrets::has(&secrets::provider_account(p.provider_id)).unwrap_or(false) };
    p
}

pub fn providers_list(conn: &Connection) -> AppResult<Vec<ReverseProvider>> {
    let mut stmt = conn.prepare(&format!("SELECT {PROVIDER_COLS} FROM reverse_provider ORDER BY provider_id"))?;
    let rows = stmt.query_map([], provider_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().map(fill_has_key).collect())
}

pub fn provider_get(conn: &Connection, id: i64) -> AppResult<ReverseProvider> {
    conn.query_row(&format!("SELECT {PROVIDER_COLS} FROM reverse_provider WHERE provider_id = ?1"), params![id], provider_from_row)
        .optional()?
        .map(fill_has_key)
        .ok_or_else(|| AppError::not_found(format!("模型端点不存在: {id}")))
}

/// 这些端点里有没有 `kind='plugin'` 的：命令层据此决定启动批次前要不要先取插件登录信息（§10.1）。
/// 不存在的 id 这里不报错，留给 `batch::prepare` 给出 not_found。
pub fn providers_any_plugin(conn: &Connection, ids: &[i64]) -> AppResult<bool> {
    if ids.is_empty() {
        return Ok(false);
    }
    let sql = format!("SELECT COUNT(*) FROM reverse_provider WHERE kind = 'plugin' AND provider_id IN ({})", vec!["?"; ids.len()].join(","));
    let n: i64 = conn.query_row(&sql, params_from_iter(ids.iter()), |r| r.get(0))?;
    Ok(n > 0)
}

/// 校验并归一化端点输入（§10.1），返回落库用的 (name, base_url, model, plugin_provider_id)：
/// `plugin` → 必须有 `plugin_provider_id`，`name` 留空用它，`base_url` / `model` 存空串；
/// `openai` → `name` / `base_url` / `model` 都不能为空，`plugin_provider_id` 存 NULL；其它 kind → `bad_request`。
fn normalize_provider_input(input: &ReverseProviderInput) -> AppResult<(String, String, String, Option<String>)> {
    let name = input.name.trim();
    match input.kind.as_str() {
        PROVIDER_KIND_PLUGIN => {
            let pid = input.plugin_provider_id.as_deref().map(str::trim).unwrap_or("");
            if pid.is_empty() {
                return Err(AppError::bad_request("kind=plugin 的端点需要 plugin_provider_id"));
            }
            let name = if name.is_empty() { pid } else { name };
            Ok((name.to_string(), String::new(), String::new(), Some(pid.to_string())))
        }
        PROVIDER_KIND_OPENAI => {
            if name.is_empty() || input.base_url.trim().is_empty() || input.model.trim().is_empty() {
                return Err(AppError::bad_request("name/base_url/model 不能为空"));
            }
            Ok((name.to_string(), input.base_url.trim().to_string(), input.model.trim().to_string(), None))
        }
        other => Err(AppError::bad_request(format!("未知的端点 kind: {other}（应为 openai | plugin）"))),
    }
}

pub fn provider_save(conn: &Connection, input: &ReverseProviderInput) -> AppResult<ReverseProvider> {
    let (name, base_url, model, plugin_provider_id) = normalize_provider_input(input)?;
    let max_tokens = if input.max_tokens <= 0 { 1100 } else { input.max_tokens };
    match input.provider_id {
        Some(id) => {
            let n = conn.execute(
                "UPDATE reverse_provider SET name = ?2, base_url = ?3, model = ?4, max_tokens = ?5, enabled = ?6, kind = ?7, plugin_provider_id = ?8 WHERE provider_id = ?1",
                params![id, name, base_url, model, max_tokens, input.enabled as i64, input.kind, plugin_provider_id],
            )?;
            if n == 0 {
                return Err(AppError::not_found(format!("模型端点不存在: {id}")));
            }
            provider_get(conn, id)
        }
        None => {
            conn.execute(
                "INSERT INTO reverse_provider(name, base_url, model, max_tokens, enabled, created_at, kind, plugin_provider_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![name, base_url, model, max_tokens, input.enabled as i64, now_iso(), input.kind, plugin_provider_id],
            )?;
            provider_get(conn, conn.last_insert_rowid())
        }
    }
}

pub fn provider_delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM reverse_provider WHERE provider_id = ?1", params![id])?;
    let _ = secrets::delete(&secrets::provider_account(id));
    Ok(())
}

// ---------- 题库 ----------

pub struct QuestionRow {
    pub entry_id: String,
    pub ordinal: i64,
    pub title: String,
    pub rating: String,
    pub thumb_path: Option<String>,
    pub local_path: Option<String>,
    pub raw_tag: Option<String>,
    pub tags: String,
    pub negative: Option<String>,
    pub character_prompts: Vec<CharacterPrompt>,
}

pub fn question_row(conn: &Connection, entry_id: &str, ordinal: i64) -> AppResult<Option<QuestionRow>> {
    conn.query_row(
        "SELECT e.entry_id, i.ordinal, e.title, e.rating, i.thumb_path, i.local_path, i.raw_tag, e.tags, e.negative, e.character_prompts_json
         FROM codex_entry e JOIN entry_image i ON i.entry_id = e.entry_id WHERE e.entry_id = ?1 AND i.ordinal = ?2",
        params![entry_id, ordinal],
        map_question_row,
    )
    .optional()
    .map_err(Into::into)
}

fn map_question_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<QuestionRow> {
    let cp: Option<String> = r.get(9)?;
    Ok(QuestionRow {
        entry_id: r.get(0)?,
        ordinal: r.get(1)?,
        title: r.get(2)?,
        rating: r.get(3)?,
        thumb_path: r.get(4)?,
        local_path: r.get(5)?,
        raw_tag: r.get(6)?,
        tags: r.get(7)?,
        negative: r.get(8)?,
        character_prompts: cp.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
    })
}

pub fn question_id(entry_id: &str, ordinal: i64) -> String {
    format!("{entry_id}#{ordinal}")
}

pub fn questions(conn: &Connection, q: &QuestionQuery, ruleset: &CompiledRuleset) -> AppResult<Vec<Question>> {
    let mut sql = String::from("e.removed_at IS NULL");
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(c) = q.codex_id.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND e.codex_id = ?");
        args.push(Box::new(c.clone()));
    }
    if let Some(r) = q.rating.as_ref() {
        if r.is_empty() {
            sql.push_str(" AND 0");
        } else {
            sql.push_str(&format!(" AND e.rating IN ({})", vec!["?"; r.len()].join(",")));
            for x in r {
                args.push(Box::new(x.as_str().to_string()));
            }
        }
    }
    if let Some(p) = q.path_prefix.as_ref() {
        for (i, seg) in p.iter().enumerate() {
            sql.push_str(&format!(" AND json_extract(e.path_json, '$[{i}]') = ?"));
            args.push(Box::new(seg.clone()));
        }
    }
    if let Some(d) = q.decision.as_ref() {
        if d.is_empty() {
            sql.push_str(" AND 0");
        } else {
            sql.push_str(&format!(" AND COALESCE(m.decision, 'undecided') IN ({})", vec!["?"; d.len()].join(",")));
            for x in d {
                args.push(Box::new(x.as_str().to_string()));
            }
        }
    }
    if q.main_image_only {
        sql.push_str(" AND i.ordinal = 0");
    }
    if let Some(k) = q.keyword.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sql.push_str(" AND (e.title LIKE ? ESCAPE '\\' OR e.tags LIKE ? ESCAPE '\\')");
        let pat = format!("%{}%", k.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        args.push(Box::new(pat.clone()));
        args.push(Box::new(pat));
    }
    let limit = if q.limit <= 0 { 100 } else { q.limit.min(2000) };
    let offset = q.offset.max(0);
    args.push(Box::new(limit));
    args.push(Box::new(offset));
    let full = format!(
        "SELECT e.entry_id, i.ordinal, e.title, e.rating, i.thumb_path, i.local_path, i.raw_tag, e.tags, e.negative, e.character_prompts_json
         FROM codex_entry e JOIN entry_image i ON i.entry_id = e.entry_id
         LEFT JOIN library_membership m ON m.entry_id = e.entry_id
         WHERE {sql} ORDER BY e.codex_id, e.book_order, i.ordinal LIMIT ? OFFSET ?"
    );
    let mut stmt = conn.prepare(&full)?;
    let rows = stmt
        .query_map(params_from_iter(args.iter().map(|b| b.as_ref())), map_question_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let aliases = character_aliases();
    Ok(rows
        .into_iter()
        .map(|r| {
            let truth = metrics::build_truth(
                &metrics::TruthInput { raw_tag: r.raw_tag.as_deref(), tags: &r.tags, character_prompts: &r.character_prompts, negative: r.negative.as_deref() },
                ruleset,
                aliases,
            );
            // 与批次里 insert_run 写的 truth_kind 同一判定（§11.4），题库据此标「散文」/ 只看标签题
            let prose = metrics::truth_kind_of(r.raw_tag.as_deref(), &r.tags) == TRUTH_KIND_PROSE;
            Question {
                question_id: question_id(&r.entry_id, r.ordinal),
                entry_id: r.entry_id,
                ordinal: r.ordinal,
                title: r.title,
                rating: Rating::parse(&r.rating).unwrap_or(Rating::Safe),
                thumb_path: r.thumb_path,
                local_path: r.local_path,
                truth_source: truth.truth_source,
                truth_preview: truth.truth.join(", ").chars().take(200).collect(),
                prose,
            }
        })
        .collect())
}

// ---------- 批次 ----------

fn provider_names(conn: &Connection, ids_json: &str) -> Vec<String> {
    let ids: Vec<i64> = serde_json::from_str(ids_json).unwrap_or_default();
    ids.iter()
        .map(|id| {
            conn.query_row("SELECT name FROM reverse_provider WHERE provider_id = ?1", params![id], |r| r.get::<_, String>(0))
                .unwrap_or_else(|_| format!("#{id}"))
        })
        .collect()
}

fn batch_from_row(conn: &Connection, r: &rusqlite::Row<'_>) -> rusqlite::Result<BatchSummary> {
    let ids_json: String = r.get(6)?;
    Ok(BatchSummary {
        batch_id: r.get(0)?,
        created_at: r.get(1)?,
        finished_at: r.get(2)?,
        status: r.get(3)?,
        template: r.get(4)?,
        ruleset: r.get(5)?,
        providers: provider_names(conn, &ids_json),
        question_count: r.get(7)?,
        note: r.get(8)?,
    })
}

const BATCH_SQL: &str = "SELECT b.batch_id, b.created_at, b.finished_at, b.status,
    COALESCE((SELECT t.name || ' v' || t.version FROM reverse_template t WHERE t.template_id = b.template_id), '#' || b.template_id),
    COALESCE((SELECT r.name || ' v' || r.version FROM reverse_ruleset r WHERE r.ruleset_id = b.ruleset_id), '#' || b.ruleset_id),
    b.provider_ids_json, b.question_count, b.note
    FROM reverse_batch b";

pub fn batches_list(conn: &Connection) -> AppResult<Vec<BatchSummary>> {
    let mut stmt = conn.prepare(&format!("{BATCH_SQL} ORDER BY b.created_at DESC"))?;
    let rows = stmt.query_map([], |r| batch_from_row(conn, r))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn batch_get(conn: &Connection, batch_id: &str) -> AppResult<BatchSummary> {
    conn.query_row(&format!("{BATCH_SQL} WHERE b.batch_id = ?1"), params![batch_id], |r| batch_from_row(conn, r))
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("批次不存在: {batch_id}")))
}

pub struct RunRecord {
    pub row: RunRow,
    pub batch_id: String,
    pub source_kind: String,
    pub source_ref: String,
    pub image_ordinal: Option<i64>,
    pub ruleset_id: i64,
    pub raw_response: Option<String>,
    pub parsed: Option<Value>,
    pub request: Value,
    pub truth: Vec<String>,
    pub truth_eval: Vec<String>,
    pub expected_miss: Vec<String>,
}

const RUN_SQL: &str = "SELECT r.run_id, r.source_kind, r.source_ref, r.image_ordinal, r.provider_id,
    COALESCE((SELECT p.name FROM reverse_provider p WHERE p.provider_id = r.provider_id), '#' || r.provider_id),
    r.status, r.latency_ms, r.error, r.cleaned_tags_json,
    m.hit, m.missed, m.extra, m.expected_hit, m.precision, m.recall, m.f1, m.appearance_leak, m.banned_leak, m.ruleset_drop, m.contradiction,
    COALESCE((SELECT i.thumb_path FROM entry_image i WHERE i.entry_id = r.source_ref AND i.ordinal = COALESCE(r.image_ordinal, 0)),
             CASE WHEN r.source_kind = 'local' THEN r.source_ref END),
    r.batch_id, r.ruleset_id, r.raw_response, r.parsed_json, r.request_json, m.truth_json, m.truth_eval_json, m.expected_miss_json,
    r.truth_kind, r.render_local, r.render_seed, r.render_prompt, r.quality_score, r.quality_notes, r.similarity_score, r.similarity_reason, r.render_error,
    COALESCE(m.appearance_leak, r.leak_appearance, 0), COALESCE(m.banned_leak, r.leak_banned, 0), r.render_artist,
    r.render_artist_source, r.render_size, r.render_size_source, r.plugin_dropped, COALESCE(r.plugin_would_reject, 0), r.render_candidates,
    r.render_style_entry_id, r.render_style_title, r.render_style_score, r.render_style_decision, COALESCE(r.render_style_fallback, 0)
    FROM reverse_run r LEFT JOIN reverse_metric m ON m.run_id = r.run_id";

/// §12.6 `render_candidates` JSON 列 → `Vec<RenderCandidate>`（NULL / 坏形状 → 空）。
fn json_candidates(s: Option<String>) -> Vec<RenderCandidate> {
    s.and_then(|t| serde_json::from_str::<Vec<RenderCandidate>>(&t).ok()).unwrap_or_default()
}

fn run_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    let source_kind: String = r.get(1)?;
    let source_ref: String = r.get(2)?;
    let image_ordinal: Option<i64> = r.get(3)?;
    let hit: Option<i64> = r.get(10)?;
    let metric = hit.map(|hit| Metric {
        hit,
        missed: r.get(11).unwrap_or(0),
        extra: r.get(12).unwrap_or(0),
        expected_hit: r.get(13).unwrap_or(0),
        precision: r.get(14).unwrap_or(None),
        recall: r.get(15).unwrap_or(None),
        f1: r.get(16).unwrap_or(None),
        appearance_leak: r.get(17).unwrap_or(0),
        banned_leak: r.get(18).unwrap_or(0),
        ruleset_drop: r.get(19).unwrap_or(0),
        contradiction: r.get(20).unwrap_or(0),
    });
    let question_id = if source_kind == "local_file" { Some(format!("local:{source_ref}")) } else { Some(question_id(&source_ref, image_ordinal.unwrap_or(0))) };
    let parsed: Option<String> = r.get(25)?;
    let request: Option<String> = r.get(26)?;
    Ok(RunRecord {
        row: RunRow {
            run_id: r.get(0)?,
            question_id,
            provider_id: r.get(4)?,
            provider: r.get(5)?,
            status: r.get(6)?,
            latency_ms: r.get(7)?,
            error: r.get(8)?,
            cleaned_tags: json_str_vec(r.get(9)?),
            metric,
            thumb_path: r.get(21)?,
            truth_kind: r.get(30)?,
            render_path: r.get(31)?,
            render_seed: r.get(32)?,
            render_prompt: r.get(33)?,
            quality_score: r.get(34)?,
            quality_notes: json_str_vec(r.get(35)?),
            similarity_score: r.get(36)?,
            similarity_reason: r.get(37)?,
            render_error: r.get(38)?,
            leak_appearance: r.get(39)?,
            leak_banned: r.get(40)?,
            render_artist: r.get(41)?,
            render_artist_source: r.get(42)?,
            render_size: r.get(43)?,
            render_size_source: r.get(44)?,
            plugin_dropped: json_str_vec(r.get(45)?),
            plugin_would_reject: r.get::<_, i64>(46)? != 0,
            render_candidates: json_candidates(r.get(47)?),
            render_style_entry_id: r.get(48)?,
            render_style_title: r.get(49)?,
            render_style_score: r.get(50)?,
            render_style_decision: r.get(51)?,
            render_style_fallback: r.get::<_, i64>(52)? != 0,
        },
        batch_id: r.get(22)?,
        source_kind,
        source_ref,
        image_ordinal,
        ruleset_id: r.get(23)?,
        raw_response: r.get(24)?,
        parsed: parsed.and_then(|s| serde_json::from_str(&s).ok()),
        request: request.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null),
        truth: json_str_vec(r.get(27)?),
        truth_eval: json_str_vec(r.get(28)?),
        expected_miss: json_str_vec(r.get(29)?),
    })
}

pub fn runs_of_batch(conn: &Connection, batch_id: &str) -> AppResult<Vec<RunRecord>> {
    let mut stmt = conn.prepare(&format!("{RUN_SQL} WHERE r.batch_id = ?1 ORDER BY r.run_id"))?;
    let rows = stmt.query_map(params![batch_id], run_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn mean(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        None
    } else {
        Some(v.iter().sum::<f64>() / v.len() as f64)
    }
}

pub fn batch_results(conn: &Connection, batch_id: &str) -> AppResult<BatchResults> {
    let batch = batch_get(conn, batch_id)?;
    let records = runs_of_batch(conn, batch_id)?;
    let mut by_provider: BTreeMap<i64, Vec<&RunRecord>> = BTreeMap::new();
    for r in &records {
        by_provider.entry(r.row.provider_id).or_default().push(r);
    }
    let mut summary = Vec::new();
    for (pid, runs) in by_provider {
        let provider = runs.first().map(|r| r.row.provider.clone()).unwrap_or_default();
        // §11.4：P/R/F1 只聚合 truth_kind='tags' 且 metric 非空的 run；泄漏数任何 truth_kind 都计（RunRow.leak_*，
        // tags 题与 metric 里的相同）；contradiction 仍来自有指标的 run
        let tag_metrics: Vec<&Metric> = runs.iter().filter(|r| r.row.truth_kind == TRUTH_KIND_TAGS).filter_map(|r| r.row.metric.as_ref()).collect();
        let all_metrics: Vec<&Metric> = runs.iter().filter_map(|r| r.row.metric.as_ref()).collect();
        let latencies: Vec<f64> = runs.iter().filter_map(|r| r.row.latency_ms.map(|l| l as f64)).collect();
        // §12.7：只算用到检索画风的 run（有命中且没兜底）；兜底时记下的「最像那条」不计
        let style_used: Vec<&&RunRecord> = runs.iter().filter(|r| r.row.render_style_entry_id.is_some() && !r.row.render_style_fallback).collect();
        summary.push(ProviderSummary {
            provider_id: pid,
            provider,
            runs: runs.len() as i64,
            ok: runs.iter().filter(|r| r.row.status == "ok").count() as i64,
            refused: runs.iter().filter(|r| r.row.status == "refused").count() as i64,
            errors: runs.iter().filter(|r| r.row.status == "error").count() as i64,
            precision: mean(&tag_metrics.iter().filter_map(|m| m.precision).collect::<Vec<_>>()),
            recall: mean(&tag_metrics.iter().filter_map(|m| m.recall).collect::<Vec<_>>()),
            f1: mean(&tag_metrics.iter().filter_map(|m| m.f1).collect::<Vec<_>>()),
            appearance_leak: runs.iter().map(|r| r.row.leak_appearance).sum(),
            banned_leak: runs.iter().map(|r| r.row.leak_banned).sum(),
            contradiction: all_metrics.iter().map(|m| m.contradiction).sum(),
            avg_latency_ms: mean(&latencies),
            tags_n: tag_metrics.len() as i64,
            render_n: runs.iter().filter(|r| r.row.render_path.is_some()).count() as i64,
            quality_avg: mean(&runs.iter().filter_map(|r| r.row.quality_score).collect::<Vec<_>>()),
            similarity_avg: mean(&runs.iter().filter_map(|r| r.row.similarity_score).collect::<Vec<_>>()),
            style_hit_n: style_used.len() as i64,
            style_score_avg: mean(&style_used.iter().filter_map(|r| r.row.render_style_score).collect::<Vec<_>>()),
        });
    }
    Ok(BatchResults { batch, runs: records.into_iter().map(|r| r.row).collect(), summary })
}

pub fn run_record(conn: &Connection, run_id: i64) -> AppResult<RunRecord> {
    conn.query_row(&format!("{RUN_SQL} WHERE r.run_id = ?1"), params![run_id], run_from_row)
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("run 不存在: {run_id}")))
}

/// 单条 `RunRow`（`reverse_render_run` 重画后返回；§11.4）。
pub fn run_row(conn: &Connection, run_id: i64) -> AppResult<RunRow> {
    run_record(conn, run_id).map(|r| r.row)
}

/// §11.4 单条重画要用到的东西：清洗后的标签、解析出的 framing、题目图片的本地路径（图包题查 `entry_image.local_path`，
/// 本地图片题就是 `source_ref`）、`request_json`（里面的 `render` 键记着批次当时的角色与固定种子）；
/// §12.7 `source` = 画风检索的来源（词典题 entry/ordinal、本地题 path）。
pub struct RenderTarget {
    pub run_id: i64,
    pub status: String,
    pub cleaned: Vec<String>,
    pub framing: Option<String>,
    pub image_path: Option<String>,
    pub request: Value,
    pub source: StyleSearchSource,
}

pub fn render_target(conn: &Connection, run_id: i64) -> AppResult<RenderTarget> {
    let rec = run_record(conn, run_id)?;
    let framing = rec.parsed.as_ref().and_then(|p| p.get("framing")).and_then(Value::as_str).map(str::to_string);
    let image_path = if rec.source_kind == "local_file" {
        Some(rec.source_ref.clone())
    } else {
        question_row(conn, &rec.source_ref, rec.image_ordinal.unwrap_or(0))?.and_then(|q| q.local_path)
    };
    let source = if rec.source_kind == "local_file" {
        StyleSearchSource::Path { path: rec.source_ref.clone() }
    } else {
        StyleSearchSource::Entry { entry_id: rec.source_ref.clone(), ordinal: rec.image_ordinal.unwrap_or(0) }
    };
    Ok(RenderTarget { run_id, status: rec.row.status, cleaned: rec.row.cleaned_tags, framing, image_path, request: rec.request, source })
}

pub fn run_get(conn: &Connection, run_id: i64) -> AppResult<RunDetail> {
    let rec = run_record(conn, run_id)?;
    let ruleset = ruleset_get(conn, rec.ruleset_id).map(|r| CompiledRuleset::compile(&r)).unwrap_or_else(|_| CompiledRuleset::compile(&super::clean::builtin_ruleset()))?;
    let framing = rec.parsed.as_ref().and_then(|p| p.get("framing")).and_then(|f| f.as_str()).unwrap_or("").to_string();
    let pred_raw = rec.parsed.as_ref().map(|p| metrics::pred_raw_from_tags(&super::client::tags_field(p))).unwrap_or_default();
    let pred = metrics::pred_from_cleaned(&rec.row.cleaned_tags, &framing);
    // negatives 未入库：从条目现值重算
    let negatives = if rec.source_kind == "pack_image" {
        question_row(conn, &rec.source_ref, rec.image_ordinal.unwrap_or(0))?
            .map(|q| {
                let t = metrics::build_truth(
                    &metrics::TruthInput { raw_tag: q.raw_tag.as_deref(), tags: &q.tags, character_prompts: &q.character_prompts, negative: q.negative.as_deref() },
                    &ruleset,
                    character_aliases(),
                );
                t.negatives
            })
            .unwrap_or_default()
    } else {
        vec![]
    };
    let sets = metrics::TruthSets {
        truth: rec.truth.clone(),
        excluded: vec![],
        expected_miss: rec.expected_miss.clone(),
        truth_eval: rec.truth_eval.clone(),
        negatives,
        truth_source: String::new(),
    };
    let detail = if rec.row.metric.is_some() { metrics::compute(&sets, &pred_raw, &pred, &ruleset) } else { metrics::MetricDetail::default() };
    let judgment = conn
        .query_row("SELECT score, comment, at FROM reverse_judgment WHERE run_id = ?1", params![run_id], |r| {
            Ok(Judgment { score: r.get(0)?, comment: r.get(1)?, at: r.get(2)? })
        })
        .optional()?;
    Ok(RunDetail {
        run: rec.row,
        raw_response: rec.raw_response,
        parsed: rec.parsed,
        pred_raw,
        pred,
        truth: rec.truth,
        truth_eval: rec.truth_eval,
        expected_miss: rec.expected_miss,
        hits: detail.hits,
        misses: detail.misses,
        extras: detail.extras,
        expected_hits: detail.expected_hits,
        contradictions: detail.contradictions,
        request: rec.request,
        judgment,
    })
}

pub fn compare(conn: &Connection, batch_a: &str, batch_b: &str) -> AppResult<CompareResult> {
    let a = batch_get(conn, batch_a)?;
    let b = batch_get(conn, batch_b)?;
    let collect = |id: &str| -> AppResult<HashMap<String, (Vec<f64>, i64)>> {
        let mut map: HashMap<String, (Vec<f64>, i64)> = HashMap::new();
        for r in runs_of_batch(conn, id)? {
            let Some(q) = r.row.question_id.clone() else { continue };
            let e = map.entry(q).or_insert_with(|| (vec![], r.row.run_id));
            if let Some(f1) = r.row.metric.as_ref().and_then(|m| m.f1) {
                e.0.push(f1);
            }
        }
        Ok(map)
    };
    let ma = collect(batch_a)?;
    let mb = collect(batch_b)?;
    let mut keys: Vec<String> = ma.keys().chain(mb.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    let mut rows = Vec::new();
    let mut all_a = Vec::new();
    let mut all_b = Vec::new();
    for k in keys {
        let (f1_a, run_a) = ma.get(&k).map(|(v, id)| (mean(v), Some(*id))).unwrap_or((None, None));
        let (f1_b, run_b) = mb.get(&k).map(|(v, id)| (mean(v), Some(*id))).unwrap_or((None, None));
        if let Some(x) = f1_a {
            all_a.push(x);
        }
        if let Some(x) = f1_b {
            all_b.push(x);
        }
        let delta = match (f1_a, f1_b) {
            (Some(x), Some(y)) => Some(y - x),
            _ => None,
        };
        rows.push(CompareRow { question_id: k, f1_a, f1_b, delta, run_a, run_b });
    }
    Ok(CompareResult { batch_a: a, batch_b: b, rows, avg_f1_a: mean(&all_a), avg_f1_b: mean(&all_b) })
}

pub fn judge(conn: &Connection, run_id: i64, score: i64, comment: &str) -> AppResult<()> {
    let exists: i64 = conn.query_row("SELECT COUNT(*) FROM reverse_run WHERE run_id = ?1", params![run_id], |r| r.get(0))?;
    if exists == 0 {
        return Err(AppError::not_found(format!("run 不存在: {run_id}")));
    }
    let comment: Option<&str> = if comment.trim().is_empty() { None } else { Some(comment) };
    conn.execute(
        "INSERT INTO reverse_judgment(run_id, score, comment, at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(run_id) DO UPDATE SET score = excluded.score, comment = excluded.comment, at = excluded.at",
        params![run_id, score, comment, now_iso()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_input(provider_id: Option<i64>, name: &str, max_tokens: i64, enabled: bool) -> ReverseProviderInput {
        ReverseProviderInput {
            provider_id,
            name: name.into(),
            base_url: "https://x/v1".into(),
            model: "m".into(),
            max_tokens,
            enabled,
            kind: PROVIDER_KIND_OPENAI.into(),
            plugin_provider_id: None,
        }
    }

    fn plugin_input(provider_id: Option<i64>, name: &str, plugin_provider_id: Option<&str>) -> ReverseProviderInput {
        ReverseProviderInput {
            provider_id,
            name: name.into(),
            base_url: String::new(),
            model: String::new(),
            max_tokens: 800,
            enabled: true,
            kind: PROVIDER_KIND_PLUGIN.into(),
            plugin_provider_id: plugin_provider_id.map(str::to_string),
        }
    }

    /// §10.1：插件端点落库 / 读回：kind 持久化、base_url / model 存空串、has_key 恒 true、name 留空用 provider id；
    /// 校验：plugin 缺 plugin_provider_id、openai 缺 base_url、未知 kind 都 bad_request；老客户端不传 kind 即 openai。
    #[test]
    fn plugin_provider_round_trip_and_validation() {
        let db = crate::db::Db::open_memory().unwrap();
        db.with(|c| {
            let p = provider_save(c, &plugin_input(None, "", Some("  gpt-4o  ")))?;
            assert_eq!(p.kind, "plugin");
            assert_eq!(p.plugin_provider_id.as_deref(), Some("gpt-4o"), "首尾空白去掉");
            assert_eq!(p.name, "gpt-4o", "name 留空默认为 provider id");
            assert_eq!((p.base_url.as_str(), p.model.as_str()), ("", ""));
            assert!(p.has_key, "插件端点不需要 key，has_key 恒 true");
            assert!(p.is_plugin());
            let (kind, pid, base, model): (String, Option<String>, String, String) = c.query_row(
                "SELECT kind, plugin_provider_id, base_url, model FROM reverse_provider WHERE provider_id = ?1",
                params![p.provider_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            assert_eq!((kind.as_str(), pid.as_deref(), base.as_str(), model.as_str()), ("plugin", Some("gpt-4o"), "", ""));

            // 列表读回同样带 kind；本地端点 kind=openai、plugin_provider_id=None、has_key 看钥匙串（这台机器没有 → false）
            let o = provider_save(c, &openai_input(None, "local", 1100, true))?;
            assert_eq!((o.kind.as_str(), o.plugin_provider_id.as_deref(), o.has_key), ("openai", None, false));
            let all = providers_list(c)?;
            assert_eq!(all.len(), 2);
            assert_eq!(all.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(), ["plugin", "openai"]);
            assert!(providers_any_plugin(c, &[p.provider_id])?);
            assert!(!providers_any_plugin(c, &[o.provider_id])?);
            assert!(providers_any_plugin(c, &[o.provider_id, p.provider_id])?);
            assert!(!providers_any_plugin(c, &[])?);
            assert!(!providers_any_plugin(c, &[999])?, "不存在的 id 不算");

            // 更新：改名、关掉，kind 与 provider id 保持
            let p2 = provider_save(c, &ReverseProviderInput { provider_id: Some(p.provider_id), name: "插件 4o".into(), enabled: false, ..plugin_input(None, "", Some("gpt-4o")) })?;
            assert_eq!((p2.name.as_str(), p2.enabled, p2.kind.as_str(), p2.plugin_provider_id.as_deref()), ("插件 4o", false, "plugin", Some("gpt-4o")));
            assert!(p2.has_key);
            // 同一行可以改成 openai（plugin_provider_id 清成 NULL）
            let p3 = provider_save(c, &openai_input(Some(p.provider_id), "改成本地", 100, true))?;
            assert_eq!((p3.kind.as_str(), p3.plugin_provider_id.is_none(), p3.base_url.as_str()), ("openai", true, "https://x/v1"));

            // 校验
            let err = provider_save(c, &plugin_input(None, "x", None)).unwrap_err();
            assert_eq!(err.code, "bad_request");
            assert_eq!(provider_save(c, &plugin_input(None, "x", Some("   "))).unwrap_err().code, "bad_request");
            let mut no_base = openai_input(None, "x", 1, true);
            no_base.base_url = String::new();
            assert_eq!(provider_save(c, &no_base).unwrap_err().code, "bad_request");
            let mut bad_kind = openai_input(None, "x", 1, true);
            bad_kind.kind = "ollama".into();
            assert_eq!(provider_save(c, &bad_kind).unwrap_err().code, "bad_request");

            // 老客户端的 JSON（没有 kind / plugin_provider_id）→ openai
            let legacy: ReverseProviderInput = serde_json::from_value(serde_json::json!({ "name": "old", "base_url": "https://x/v1", "model": "m", "max_tokens": 1100, "enabled": true }))?;
            assert_eq!((legacy.kind.as_str(), legacy.plugin_provider_id.is_none(), legacy.provider_id), ("openai", true, None));
            let saved = provider_save(c, &legacy)?;
            assert_eq!(saved.kind, "openai");
            // 序列化出去带齐两个新键
            let v = serde_json::to_value(&saved)?;
            assert_eq!(v["kind"], "openai");
            assert!(v.get("plugin_provider_id").is_some_and(|x| x.is_null()));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn template_and_ruleset_versions_never_overwrite() {
        let db = crate::db::Db::open_memory().unwrap();
        db.with(|c| {
            super::super::builtin::ensure_builtin(c)?;
            let t = template_save(c, &ReverseTemplateInput { name: "plugin".into(), body: "v2 body".into(), parent_id: Some(1), note: None })?;
            assert_eq!(t.version, 2);
            let all = templates_list(c)?;
            assert_eq!(all.len(), 2);
            assert_eq!(all[0].version, 2);
            let mut rs_in = ReverseRulesetInput {
                name: "plugin".into(),
                banned: vec!["cat".into()],
                appearance_regex: r"\bhair\b".into(),
                accessory_ok_regex: "ribbon".into(),
                refusal_regex: "sorry".into(),
                always_drop: vec!["solo".into()],
                max_words: 5,
                max_len: 30,
                note: Some("n".into()),
            };
            let rs = ruleset_save(c, &rs_in)?;
            assert_eq!(rs.version, 2);
            assert_eq!(rs.banned, vec!["cat"]);
            rs_in.appearance_regex = "(".into();
            assert!(ruleset_save(c, &rs_in).is_err());
            let p = provider_save(c, &openai_input(None, "gpt", 0, true))?;
            assert_eq!(p.max_tokens, 1100);
            let p2 = provider_save(c, &openai_input(Some(p.provider_id), "gpt2", 500, false))?;
            assert_eq!((p2.name.as_str(), p2.max_tokens, p2.enabled), ("gpt2", 500, false));
            assert!(provider_save(c, &openai_input(Some(999), "x", 1, true)).is_err());
            assert!(judge(c, 12345, 5, "x").is_err());
            Ok(())
        })
        .unwrap();
    }
}
