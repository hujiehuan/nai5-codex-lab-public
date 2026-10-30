//! Creative drawing is a separate, free-image-only protocol. References are read from the
//! local decision database; immutable inputs/plans and run snapshots live outside SQLite.
use crate::{
    artist,
    error::{AppError, AppResult},
    library,
    models::CharacterPrompt,
    plugin::{self, PluginConn},
    state::AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::Connection;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// Built-in prompting guide shipped with the app (`resources/prompting-guide/GUIDE.md`); bump the version when the text changes.
pub const SKILL_VERSION: &str = "builtin-1";
pub(crate) const SKILL_TEXT: &str = include_str!("../../resources/prompting-guide/GUIDE.md");
const FREE_SIZES: [(&str, &str, u32, u32); 3] = [
    ("portrait", "832x1216", 832, 1216),
    ("landscape", "1216x832", 1216, 832),
    ("square", "1024x1024", 1024, 1024),
];
const MAX_TEXT: usize = 32_000;
const MAX_REFERENCE_BYTES: usize = 96_000;
const MAX_SAMPLE_REQUEST_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceMode {
    #[default]
    Sample,
    All,
}
impl ReferenceMode {
    fn timeout(self) -> u64 {
        if self == Self::All {
            600
        } else {
            180
        }
    }
}
fn validate_reference_capability(cap: &Capabilities, mode: ReferenceMode) -> AppResult<()> {
    if mode == ReferenceMode::All && !cap.full_reference_supported {
        return Err(AppError::new(
            "creative_full_reference_unsupported",
            "当前插件不支持全量参考，请更新并重载薇拉插件，再刷新连接状态；不会改为抽样",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FreeSize {
    pub id: String,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub version: u32,
    #[serde(default)]
    pub full_reference_supported: bool,
    pub free_only: bool,
    pub free_sizes: Vec<FreeSize>,
    pub providers: Vec<String>,
    pub characters: Vec<String>,
    pub model: String,
    pub steps: u32,
    pub available: bool,
    pub reason: Option<String>,
    pub render_available: bool,
    pub render_reason: Option<String>,
    pub min_stamina_percent: f64,
    pub free_limitations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub entry_id: String,
    pub title: String,
    pub path: Vec<String>,
    pub tags: String,
    pub negative: Option<String>,
    pub character_prompts: Vec<CharacterPrompt>,
    pub content_hash: String,
    pub decision: String,
    pub rating: String,
    pub prose: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceQuery {
    #[serde(default)]
    pub scope: Vec<Vec<String>>,
    pub search: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct ReferencePage {
    pub items: Vec<Reference>,
    pub total: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub character: String,
    pub requirements: String,
    pub provider_id: String,
    pub orientation: String,
    pub scope: Vec<Vec<String>>,
    pub reference_ids: Vec<String>,
    pub reference_count: usize,
    #[serde(default)]
    pub reference_mode: ReferenceMode,
    pub style_name: String,
    pub artist_ratio: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RenderCharacter {
    pub prompt: String,
    pub negative_prompt: String,
    pub position: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequirementCheck {
    pub requirement: String,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub scale: f64,
    pub cfg: f64,
    pub sampler: String,
    pub noise_schedule: String,
    pub other: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub plan_id: String,
    pub parent_plan_id: Option<String>,
    pub created_at: String,
    pub character: String,
    pub requirements: String,
    pub provider_id: String,
    pub skill_version: String,
    pub concept: String,
    pub prompt: String,
    pub negative: String,
    pub artist: String,
    pub artist_source: String,
    pub character_prompts: Vec<RenderCharacter>,
    pub reference_ids: Vec<String>,
    #[serde(default)]
    pub reference_mode: ReferenceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_reference_ids: Option<Vec<String>>,
    pub requirement_checks: Vec<RequirementCheck>,
    pub warnings: Vec<String>,
    pub model: String,
    pub size: String,
    pub steps: u32,
    pub parameters: Parameters,
    pub free_only: bool,
    pub llm_raw: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    pub plan_id: String,
    pub prompt: String,
    pub negative: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_prompts: Option<Vec<RenderCharacter>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub plan_id: String,
    pub request_id: String,
    pub count: usize,
    pub seed: Option<u32>,
    pub score: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreativeImage {
    pub idx: usize,
    pub seed: u32,
    pub status: String,
    pub file: Option<String>,
    pub error: Option<String>,
    pub score: Option<f64>,
    pub notes: Vec<String>,
    pub account_index: Option<u32>,
    pub anlas_before: Option<f64>,
    pub anlas_after: Option<f64>,
    pub anlas_delta: Option<f64>,
    pub stamina_before: Option<f64>,
    pub stamina_after: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub run_id: String,
    pub plan_id: String,
    pub plan: Plan,
    pub request_id: String,
    pub status: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub images: Vec<CreativeImage>,
    pub error: Option<String>,
    pub free_only: bool,
    pub anlas_spent: Option<f64>,
    pub stamina_before: Option<f64>,
    pub stamina_after: Option<f64>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cancel_requested: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPlan {
    version: u32,
    plugin_identity: String,
    input: Value,
    references: Vec<Reference>,
    plan: Plan,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRun {
    version: u32,
    plugin_identity: String,
    run: Run,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartIntent {
    version: u32,
    plugin_identity: String,
    request: StartRequest,
}

fn shape(message: impl Into<String>) -> AppError {
    AppError::new("creative_protocol", message)
}
fn unsupported() -> AppError {
    AppError::new("creative_unsupported", "当前连接的插件尚未提供创作接口。请确认已更新并重载支持创作实验室的薇拉插件，再刷新连接状态。")
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn identity(conn: &PluginConn) -> String {
    digest(format!("{}\n{}", conn.base, conn.username).as_bytes())
}
fn valid_id(id: &str) -> AppResult<()> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(shape("创作记录 ID 不合法"));
    }
    Ok(())
}
fn timestamp(s: &str) -> AppResult<()> {
    let t = chrono::DateTime::parse_from_rfc3339(s)
        .map_err(|_| shape("创作时间必须为 UTC ISO 8601"))?;
    if t.offset().local_minus_utc() != 0 {
        return Err(shape("创作时间必须为 UTC"));
    }
    Ok(())
}
fn scope_valid(scope: &[Vec<String>]) -> AppResult<()> {
    if scope.len() > 100
        || scope.iter().any(|p| {
            p.is_empty() || p.len() > 20 || p.iter().any(|v| v.is_empty() || v.len() > 1024)
        })
    {
        return Err(AppError::bad_request("参考范围必须是有效的目录前缀列表"));
    }
    Ok(())
}
fn in_scope(path: &[String], scope: &[Vec<String>]) -> bool {
    scope.is_empty() || scope.iter().any(|prefix| path.starts_with(prefix))
}

/// No exporting, syncing, or image hashing: this query is read-only even for previously
/// uncached entries. Parse all text sources for blacklisted artists, including prose.
pub fn local_references(conn: &Connection, scope: &[Vec<String>]) -> AppResult<Vec<Reference>> {
    local_references_for(conn, scope, false)
}

/// §16 studio-only decision expansion; legacy callers retain include. Only `safe` packs are ever references.
pub(crate) fn local_references_for(conn: &Connection, scope: &[Vec<String>], all_decisions: bool) -> AppResult<Vec<Reference>> {
    scope_valid(scope)?;
    let blacklist = library::blacklist_set(conn)?;
    let mut stmt = conn.prepare("SELECT e.entry_id,e.title,e.path_json,e.tags,e.negative,e.character_prompts_json,p.artists_json,COALESCE(m.decision,'undecided') FROM codex_entry e LEFT JOIN library_membership m ON m.entry_id=e.entry_id LEFT JOIN artist_parse p ON p.entry_id=e.entry_id WHERE e.codex_id IN ('nai5_community_pack','local_pack') AND e.rating='safe' AND e.removed_at IS NULL AND (m.decision='include' OR (?1 AND COALESCE(m.decision,'undecided') IN ('exclude','undecided'))) ORDER BY e.book_order,e.entry_id")?;
    let rows = stmt.query_map(rusqlite::params![all_decisions], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut refs = Vec::new();
    for row in rows {
        let (entry_id, title, path_json, tags, negative, characters, parsed_artists, decision) = row?;
        let path: Vec<String> = serde_json::from_str(&path_json)
            .map_err(|_| AppError::db(format!("图包 {entry_id} 的目录结构损坏")))?;
        if !in_scope(&path, scope) {
            continue;
        }
        let character_prompts: Vec<CharacterPrompt> =
            serde_json::from_str(characters.as_deref().unwrap_or("[]"))
                .map_err(|_| AppError::db(format!("图包 {entry_id} 的角色提示词结构损坏")))?;
        let combined = crate::export::pack_source_text(&tags, &character_prompts);
        if combined.trim().is_empty() {
            continue;
        }
        let mut artists =
            artist::parse_entry_tags(&combined, path.first().map(String::as_str)).artists;
        if let Some(parsed) = parsed_artists {
            // Broken cached artist parsing must not turn into a blacklist bypass.
            let cached: Vec<crate::models::ArtistWeighted> = serde_json::from_str(&parsed)
                .map_err(|_| AppError::db(format!("图包 {entry_id} 的画师解析损坏")))?;
            artists.extend(cached);
        }
        if artists
            .iter()
            .any(|a| blacklist.contains(&artist::canonical_artist_tag_name(&a.name)))
        {
            continue;
        }
        let prose = artist::is_prose_prompt(&tags)
            || character_prompts
                .iter()
                .any(|c| artist::is_prose_prompt(&c.prompt));
        let content_hash = digest(&serde_json::to_vec(
            &json!({"title":title,"path":path,"tags":tags,"negative":negative,"character_prompts":character_prompts,"prose":prose}),
        )?);
        refs.push(Reference {
            entry_id,
            title,
            path,
            tags,
            negative,
            character_prompts,
            content_hash,
            decision,
            rating: "safe".into(),
            prose,
        });
    }
    Ok(refs)
}

pub async fn references(state: &AppState, req: ReferenceQuery) -> AppResult<ReferencePage> {
    scope_valid(&req.scope)?;
    let offset = req.offset.unwrap_or(0);
    let limit = req.limit.unwrap_or(200);
    if limit == 0
        || limit > 500
        || offset > 100_000
        || req.search.as_ref().is_some_and(|s| s.len() > 1000)
    {
        return Err(AppError::bad_request("参考列表分页或搜索参数不合法"));
    }
    let mut refs =
        crate::commands::blocking(state.db.clone(), move |c| local_references(c, &req.scope))
            .await?;
    if let Some(search) = req.search.filter(|s| !s.trim().is_empty()) {
        let search = search.to_lowercase();
        refs.retain(|r| {
            format!("{} {} {}", r.title, r.path.join(" "), r.tags)
                .to_lowercase()
                .contains(&search)
        });
    }
    let total = refs.len();
    Ok(ReferencePage {
        items: refs.into_iter().skip(offset).take(limit).collect(),
        total,
    })
}

fn terms(text: &str) -> HashSet<String> {
    let mut out: HashSet<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(String::from)
        .collect();
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    for p in chars.windows(2) {
        if p.iter().any(|c| !c.is_ascii()) {
            out.insert(p.iter().collect());
        }
    }
    out
}
pub fn select_references(
    mut pool: Vec<Reference>,
    req: &PlanRequest,
    nonce: &str,
) -> AppResult<Vec<Reference>> {
    if req.reference_mode == ReferenceMode::All {
        // Sample count and saved priority selections are inactive in full mode.
        if pool.is_empty() {
            return Err(AppError::bad_request(
                "当前范围没有可用的已加入 safe 图包；请先在本地加入图包或调整范围",
            ));
        }
        return Ok(pool);
    }
    if !(3..=6).contains(&req.reference_count) || req.reference_ids.len() > 6 {
        return Err(AppError::bad_request("参考目标数必须为3至6，最多指定6条"));
    }
    let unique: HashSet<_> = req.reference_ids.iter().collect();
    if unique.len() != req.reference_ids.len() {
        return Err(AppError::bad_request("参考条目不能重复"));
    }
    let mut selected = Vec::new();
    for id in &req.reference_ids {
        let i = pool.iter().position(|r| &r.entry_id == id).ok_or_else(|| {
            AppError::bad_request(format!(
                "参考 {id} 不在当前已加入、安全、未删除且未被黑名单命中的本地图包范围内"
            ))
        })?;
        selected.push(pool.remove(i));
    }
    let target = req.reference_count.max(selected.len());
    let wanted = terms(&req.requirements);
    while selected.len() < target && !pool.is_empty() {
        let best = pool
            .iter()
            .enumerate()
            .max_by_key(|(_, r)| {
                let text = format!(
                    "{} {} {} {}",
                    r.title,
                    r.path.join(" "),
                    r.tags,
                    r.character_prompts
                        .iter()
                        .map(|c| c.prompt.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .to_lowercase();
                let relevance = wanted.iter().filter(|t| text.contains(t.as_str())).count() as i64;
                let same_path = selected.iter().filter(|p| p.path == r.path).count() as i64;
                (
                    relevance * 10 - same_path * 4,
                    digest(format!("{nonce}{}", r.entry_id).as_bytes()),
                )
            })
            .map(|(i, _)| i)
            .unwrap();
        selected.push(pool.remove(best));
    }
    if selected.is_empty() {
        return Err(AppError::bad_request(
            "当前范围没有可用的已加入 safe 图包；请先在本地加入图包或调整范围",
        ));
    }
    if serde_json::to_vec(&selected)?.len() > MAX_REFERENCE_BYTES {
        return Err(AppError::bad_request(
            "本次参考原文超过96KB，请减少参考或选择较短条目；不会截断原文",
        ));
    }
    Ok(selected)
}

fn validate_caps(c: &Capabilities, spending: bool) -> AppResult<()> {
    if c.version != 1 || !c.free_only || c.free_sizes.len() != 3 || c.min_stamina_percent != 5.0 {
        return Err(unsupported());
    }
    for (id, _, w, h) in FREE_SIZES {
        if c.free_sizes
            .iter()
            .filter(|s| s.id == id && s.width == w && s.height == h)
            .count()
            != 1
        {
            return Err(shape("插件免费尺寸能力与约定的三尺寸不一致"));
        }
    }
    if spending && !c.available {
        return Err(AppError::new(
            "creative_unavailable",
            c.reason.clone().unwrap_or_else(|| {
                "插件未满足免费生成或模型配置条件，请检查插件账户与提示词模型".into()
            }),
        ));
    }
    if c.available
        && (!matches!(
            c.model.as_str(),
            "nai-diffusion-5-full" | "nai-diffusion-5-curated"
        ) || !(1..=28).contains(&c.steps))
    {
        return Err(shape("插件没有提供可核对的免费模型和步数"));
    }
    Ok(())
}
fn validate_plan(p: &Plan) -> AppResult<()> {
    valid_id(&p.plan_id)?;
    if let Some(parent) = &p.parent_plan_id {
        valid_id(parent)?;
        if parent == &p.plan_id {
            return Err(shape("修改必须产生新的计划版本"));
        }
    }
    timestamp(&p.created_at)?;
    if !p.free_only
        || !FREE_SIZES.iter().any(|(_, size, _, _)| p.size == *size)
        || !(1..=28).contains(&p.steps)
        || !matches!(
            p.model.as_str(),
            "nai-diffusion-5-full" | "nai-diffusion-5-curated"
        )
        || p.parameters.other != "0"
    {
        return Err(shape("计划不满足三尺寸、最多28步、无付费附加项的免费约束"));
    }
    if p.prompt.trim().is_empty()
        || p.prompt.len() > MAX_TEXT
        || p.negative.len() > MAX_TEXT
        || p.requirements.len() > MAX_TEXT
        || p.concept.trim().is_empty()
        || p.provider_id.trim().is_empty()
        || p.skill_version != SKILL_VERSION
    {
        return Err(shape("计划的提示词、要求、模型或技能版本不合法"));
    }
    if p.reference_ids.is_empty()
        || (p.reference_mode == ReferenceMode::Sample && p.reference_ids.len() > 6)
        || p.reference_ids.iter().collect::<HashSet<_>>().len() != p.reference_ids.len()
    {
        return Err(shape("计划参考必须非空且不重复，抽样模式最多6条"));
    }
    if let Some(used) = &p.used_reference_ids {
        if used.iter().collect::<HashSet<_>>().len() != used.len()
            || used.iter().any(|id| !p.reference_ids.contains(id))
        {
            return Err(shape("模型声明借鉴的条目必须是已提供参考的不重复子集"));
        }
    }
    if !p.parameters.scale.is_finite()
        || !p.parameters.cfg.is_finite()
        || p.parameters.sampler.is_empty()
        || p.parameters.noise_schedule.is_empty()
    {
        return Err(shape("生成参数缺失或不合法"));
    }
    if !matches!(p.artist_source.as_str(), "style" | "pool" | "none")
        || (!p.artist.is_empty() && !p.prompt.starts_with(&p.artist))
    {
        return Err(shape("最终提示词与冻结的画风串不一致"));
    }
    validate_characters(&p.character_prompts)?;
    if !p.requirements.trim().is_empty() && p.requirement_checks.is_empty() {
        return Err(shape("有内容要求的方案必须提供要求检查结果"));
    }
    if p.requirement_checks
        .iter()
        .any(|r| r.requirement.trim().is_empty() || r.evidence.trim().is_empty())
    {
        return Err(shape("要求检查必须包含要求和落实依据"));
    }
    Ok(())
}
fn validate_execution(c: &Capabilities, plan: &Plan) -> AppResult<()> {
    validate_caps(c, true)?;
    validate_plan(plan)?;
    if !c.render_available {
        return Err(AppError::new(
            "creative_unavailable",
            c.render_reason
                .clone()
                .unwrap_or_else(|| "插件尚不能保证只用免费点数，已阻止出图".into()),
        ));
    }
    // Current defaults describe new plans. Existing immutable plans retain their own
    // model and steps as long as they still satisfy the supported free protocol.
    Ok(())
}
fn validate_characters(characters: &[RenderCharacter]) -> AppResult<()> {
    if characters.len() > 6
        || characters.iter().any(|c| {
            c.prompt.trim().is_empty()
                || c.prompt.len() > MAX_TEXT
                || c.negative_prompt.len() > MAX_TEXT
                || c.position.len() != 2
                || !matches!(c.position.as_bytes()[0], b'A'..=b'E')
                || !matches!(c.position.as_bytes()[1], b'1'..=b'5')
        })
    {
        return Err(shape("角色提示词或位置不合法（位置须为A1至E5）"));
    }
    Ok(())
}
fn validate_start(req: &StartRequest) -> AppResult<()> {
    valid_id(&req.plan_id)?;
    if uuid::Uuid::parse_str(&req.request_id).is_err() || !(1..=8).contains(&req.count) {
        return Err(AppError::bad_request(
            "出图需要唯一UUID请求号，张数必须为1至8",
        ));
    }
    if req
        .seed
        .is_some_and(|seed| seed.checked_add((req.count - 1) as u32).is_none())
    {
        return Err(AppError::bad_request(
            "种子加张数将超过4294967295，请减少张数或种子",
        ));
    }
    Ok(())
}
fn terminal(status: &str) -> bool {
    matches!(status, "done" | "error" | "cancelled")
}
fn validate_run(run: &Run) -> AppResult<()> {
    valid_id(&run.run_id)?;
    validate_plan(&run.plan)?;
    timestamp(&run.created_at)?;
    if let Some(at) = &run.finished_at {
        timestamp(at)?;
    }
    if !run.free_only
        || run.anlas_spent.is_some_and(|n| !n.is_finite() || n < 0.0)
        || run.plan_id != run.plan.plan_id
        || uuid::Uuid::parse_str(&run.request_id).is_err()
        || !matches!(
            run.status.as_str(),
            "queued" | "running" | "done" | "error" | "cancelled"
        )
        || run.images.is_empty()
        || run.images.len() > 8
    {
        return Err(shape("出图响应违反计划身份、状态或费用字段约束"));
    }
    if terminal(&run.status) != run.finished_at.is_some() {
        return Err(shape("运行状态与完成时间不一致"));
    }
    for (i, img) in run.images.iter().enumerate() {
        if img.idx != i
            || !matches!(
                img.status.as_str(),
                "pending" | "running" | "done" | "error" | "cancelled"
            )
            || img.score.is_some_and(|v| !v.is_finite())
        {
            return Err(shape("图片序号、状态或评分不合法"));
        }
        if let Some(file) = &img.file {
            if file.is_empty()
                || file.contains('/')
                || file.contains('\\')
                || file == "."
                || file == ".."
            {
                return Err(shape("插件图片名称不合法"));
            }
        }
        if img.status == "done" && img.file.is_none() {
            return Err(shape("已完成图片缺少文件名"));
        }
        if terminal(&run.status) && matches!(img.status.as_str(), "pending" | "running") {
            return Err(shape("已结束运行仍包含待处理图片"));
        }
        if [
            img.anlas_before,
            img.anlas_after,
            img.stamina_before,
            img.stamina_after,
        ]
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || *v < 0.0)
            || img.anlas_delta.is_some_and(|v| !v.is_finite())
        {
            return Err(shape("图片账户观测字段不合法"));
        }
    }
    if [run.stamina_before, run.stamina_after]
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(shape("点数采样不合法"));
    }
    Ok(())
}

fn store_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("creative")
}
fn record_path(data_dir: &Path, kind: &str, id: &str) -> AppResult<PathBuf> {
    valid_id(id)?;
    Ok(root(data_dir).join(kind).join(format!("{id}.json")))
}
fn read<T: DeserializeOwned>(path: &Path) -> AppResult<T> {
    let raw = fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("本地创作记录不存在；请先创建或同步计划")
        } else {
            AppError::io(e.to_string())
        }
    })?;
    serde_json::from_slice(&raw).map_err(|_| AppError::io("本地创作记录格式损坏"))
}
fn atomic(path: &Path, bytes: &[u8], immutable: bool) -> AppResult<()> {
    let _guard = store_lock()
        .lock()
        .map_err(|_| AppError::io("创作记录锁不可用"))?;
    if immutable && path.exists() {
        if fs::read(path)? != bytes {
            return Err(shape("相同记录ID返回不同内容，拒绝覆盖已冻结版本"));
        }
        return Ok(());
    }
    fs::create_dir_all(path.parent().ok_or_else(|| AppError::io("没有记录目录"))?)?;
    let tmp = path.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> AppResult<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
fn save_plan(data_dir: &Path, saved: &SavedPlan) -> AppResult<()> {
    atomic(
        &record_path(data_dir, "plans", &saved.plan.plan_id)?,
        &serde_json::to_vec(saved)?,
        true,
    )
}
fn load_plan(data_dir: &Path, id: &str) -> AppResult<SavedPlan> {
    let saved: SavedPlan = read(&record_path(data_dir, "plans", id)?)?;
    validate_plan(&saved.plan)?;
    if saved.version != 1 || saved.plan.plan_id != id {
        return Err(shape("本地计划版本或ID不匹配"));
    }
    Ok(saved)
}
fn save_run(data_dir: &Path, saved: &SavedRun) -> AppResult<()> {
    let bytes = serde_json::to_vec(saved)?;
    let hash = digest(&bytes);
    // Every observed status is retained; only the pointer used for the history view changes.
    atomic(
        &root(data_dir)
            .join("run_snapshots")
            .join(&saved.run.run_id)
            .join(format!("{hash}.json")),
        &bytes,
        true,
    )?;
    let path = record_path(data_dir, "runs", &saved.run.run_id)?;
    if path.exists() {
        let old: SavedRun = read(&path)?;
        if terminal(&old.run.status) && !terminal(&saved.run.status) {
            return Ok(());
        }
    }
    atomic(&path, &bytes, false)
}
fn load_run(data_dir: &Path, id: &str) -> AppResult<SavedRun> {
    let saved: SavedRun = read(&record_path(data_dir, "runs", id)?)?;
    validate_run(&saved.run)?;
    if saved.version != 1 || saved.run.run_id != id {
        return Err(shape("本地运行版本或ID不匹配"));
    }
    Ok(saved)
}
fn files<T: DeserializeOwned>(data_dir: &Path, kind: &str) -> AppResult<Vec<T>> {
    let path = root(data_dir).join(kind);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.extension().and_then(|x| x.to_str()) == Some("json") {
            out.push(read(&path)?);
        }
    }
    Ok(out)
}
fn check_identity(saved: &str, conn: &PluginConn) -> AppResult<()> {
    if saved != identity(conn) {
        return Err(AppError::bad_request(
            "该创作记录属于另一个插件地址或登录身份；请切回原插件后执行",
        ));
    }
    Ok(())
}
fn decode<T: DeserializeOwned>(status: reqwest::StatusCode, text: &str) -> AppResult<T> {
    // Registered routes can return business 404s (e.g. an image record not yet
    // published). Recognize those before treating an absent route as unsupported.
    let parsed = serde_json::from_str::<Value>(text);
    if let Ok(value) = &parsed {
        if value.get("status").and_then(Value::as_str) == Some("error") {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("插件拒绝创作请求");
            return Err(if status == reqwest::StatusCode::NOT_FOUND {
                AppError::not_found(message)
            } else {
                AppError::plugin_rejected(message)
            });
        }
    }
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        return Err(unsupported());
    }
    if !status.is_success() {
        return Err(AppError::plugin_rejected(format!(
            "创作接口 HTTP {status}: {}",
            plugin::truncate(text, 500)
        )));
    }
    let value = parsed.map_err(|_| shape("创作接口未返回JSON"))?;
    let data = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(data).map_err(|e| shape(format!("创作响应字段不符合协议: {e}")))
}
fn image_pending() -> AppError {
    AppError::new("creative_image_pending", "图片准备中，请稍候")
}
fn decode_route<T: DeserializeOwned>(
    route: &str,
    status: reqwest::StatusCode,
    text: &str,
) -> AppResult<T> {
    decode(status, text).map_err(|error| {
        if route == "image"
            && status == reqwest::StatusCode::NOT_FOUND
            && error.code == "not_found"
            && error.message == "这张图片尚未生成"
        {
            image_pending()
        } else {
            error
        }
    })
}
async fn call<T: DeserializeOwned>(
    state: &AppState,
    conn: &PluginConn,
    method: reqwest::Method,
    route: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
    seconds: u64,
) -> AppResult<T> {
    let mut query = query.to_vec();
    query.push(("features", "full_references".into()));
    let (status, text) = plugin::plugin_request(
        &state.http,
        conn,
        method,
        &format!("{}/lab/creative/{route}", plugin::PLUGIN_PAGE_PATH),
        &query,
        body,
        Duration::from_secs(seconds),
    )
    .await?;
    decode_route(route, status, &text)
}
async fn caps(state: &AppState, conn: &PluginConn, spending: bool) -> AppResult<Capabilities> {
    let c = call(
        state,
        conn,
        reqwest::Method::GET,
        "capabilities",
        &[],
        None,
        30,
    )
    .await?;
    validate_caps(&c, spending)?;
    Ok(c)
}
pub async fn capabilities(state: &AppState) -> AppResult<Capabilities> {
    let conn = plugin::plugin_conn(state).await?;
    caps(state, &conn, false).await
}

fn plan_character(requested: &str, pool: &[String], nonce: &str) -> AppResult<String> {
    if !requested.trim().is_empty() {
        // Preserve explicit input; the plugin resolves aliases and validates identity.
        return Ok(requested.to_owned());
    }
    // Rank each distinct identity using the new concept's random nonce. Duplicate pool
    // entries cannot bias selection, and pool ordering is not a default character.
    pool.iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty() && name.len() <= 1000)
        .min_by_key(|name| {
            (
                digest(format!("character:{nonce}:{name}").as_bytes()),
                *name,
            )
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::new(
                "creative_character_pool_empty",
                "插件角色池没有可用角色，无法随机选角；请检查插件角色池或明确填写角色后重新构思",
            )
        })
}

fn validate_new_plan(
    p: &Plan,
    req: &PlanRequest,
    cap: &Capabilities,
    references: &[Reference],
    character: &str,
) -> AppResult<()> {
    validate_plan(p)?;
    let wanted_size = FREE_SIZES
        .iter()
        .find(|(id, _, _, _)| req.orientation == *id)
        .ok_or_else(|| AppError::bad_request("构思方向不合法"))?
        .1;
    if p.parent_plan_id.is_some()
        || p.character != character
        || p.requirements != req.requirements
        || p.provider_id != req.provider_id
        || p.reference_mode != req.reference_mode
        || p.reference_ids
            != references
                .iter()
                .map(|r| r.entry_id.clone())
                .collect::<Vec<_>>()
        || p.size != wanted_size
        || p.model != cap.model
        || p.steps != cap.steps
    {
        return Err(shape("插件返回计划与冻结的输入、参考或生成配置不一致"));
    }
    Ok(())
}

pub async fn plan(state: &AppState, req: PlanRequest) -> AppResult<Plan> {
    scope_valid(&req.scope)?;
    if req.requirements.chars().count() > 4000
        || req.character.len() > 1000
        || req.style_name.len() > 1000
        || req.provider_id.len() > 1000
        || !req.artist_ratio.is_finite()
        || !(0.0..=1.0).contains(&req.artist_ratio)
        || !FREE_SIZES
            .iter()
            .any(|(id, _, _, _)| req.orientation == *id)
    {
        return Err(AppError::bad_request("构思输入长度、方向或画风概率不合法"));
    }
    let conn = plugin::plugin_conn(state).await?;
    let cap = caps(state, &conn, true).await?;
    validate_reference_capability(&cap, req.reference_mode)?;
    if !cap.providers.contains(&req.provider_id) {
        return Err(AppError::bad_request("请选择插件能力列表中的提示词模型"));
    }
    let input_id = uuid::Uuid::new_v4().to_string();
    let character = plan_character(&req.character, &cap.characters, &input_id)?;
    let scope = req.scope.clone();
    let pool =
        crate::commands::blocking(state.db.clone(), move |c| local_references(c, &scope)).await?;
    let references = select_references(pool, &req, &input_id)?;
    let wire = json!({"reference_mode":req.reference_mode,"character":character,"requirements":req.requirements,"provider_id":req.provider_id,"orientation":req.orientation,"references":references,"style_name":req.style_name,"artist_ratio":req.artist_ratio,"skill_version":SKILL_VERSION,"skill_text":SKILL_TEXT});
    if req.reference_mode == ReferenceMode::Sample
        && serde_json::to_vec(&wire)?.len() > MAX_SAMPLE_REQUEST_BYTES
    {
        return Err(AppError::bad_request(
            "抽样构思请求超过 4 MiB；不会截断参考",
        ));
    }
    // Preserve the exact materials before the billable call, including attempts that time out.
    atomic(
        &record_path(&state.data_dir, "inputs", &input_id)?,
        &serde_json::to_vec(
            &json!({"version":1,"plugin_identity":identity(&conn),"created_at":chrono::Utc::now().to_rfc3339(),"selection":req,"wire_request":wire}),
        )?,
        true,
    )?;
    let p: Plan = call(
        state,
        &conn,
        reqwest::Method::POST,
        "plan",
        &[],
        Some(&wire),
        req.reference_mode.timeout(),
    )
    .await?;
    validate_new_plan(&p, &req, &cap, &references, &character)?;
    save_plan(
        &state.data_dir,
        &SavedPlan {
            version: 1,
            plugin_identity: identity(&conn),
            input: wire,
            references,
            plan: p.clone(),
        },
    )?;
    Ok(p)
}

async fn verify_current_references(state: &AppState, saved: &SavedPlan) -> AppResult<()> {
    let now = crate::commands::blocking(state.db.clone(), |c| local_references(c, &[])).await?;
    validate_current_references(saved, &now)
}
fn validate_current_references(saved: &SavedPlan, now: &[Reference]) -> AppResult<()> {
    if saved.references.is_empty() {
        return Err(AppError::bad_request(
            "该计划缺少本地参考快照，请重新构思后执行",
        ));
    }
    for r in &saved.references {
        if !now.iter().any(|n| n.entry_id == r.entry_id) {
            return Err(AppError::bad_request(format!(
                "参考「{}」已排除、删除或命中黑名单；历史仍可查看，请重新生成方案",
                r.title
            )));
        }
    }
    Ok(())
}
pub async fn edit(state: &AppState, req: EditRequest) -> AppResult<Plan> {
    if req.prompt.trim().is_empty() || req.prompt.len() > MAX_TEXT || req.negative.len() > MAX_TEXT
    {
        return Err(AppError::bad_request("编辑的提示词不能为空或过长"));
    }
    if let Some(characters) = &req.character_prompts {
        validate_characters(characters)?;
    }
    let saved = load_plan(&state.data_dir, &req.plan_id)?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved.plugin_identity, &conn)?;
    verify_current_references(state, &saved).await?;
    let cap = caps(state, &conn, true).await?;
    validate_reference_capability(&cap, saved.plan.reference_mode)?;
    if !cap.providers.contains(&saved.plan.provider_id) {
        return Err(AppError::bad_request(
            "原提示词模型已不可用，请重新创建方案",
        ));
    }
    let wire = serde_json::to_value(&req)?;
    let input_id = uuid::Uuid::new_v4().to_string();
    atomic(
        &record_path(&state.data_dir, "edits", &input_id)?,
        &serde_json::to_vec(&wire)?,
        true,
    )?;
    let p: Plan = call(
        state,
        &conn,
        reqwest::Method::POST,
        "edit",
        &[],
        Some(&wire),
        saved.plan.reference_mode.timeout(),
    )
    .await?;
    validate_plan(&p)?;
    let old = &saved.plan;
    if p.parent_plan_id.as_deref() != Some(&old.plan_id)
        || p.character != old.character
        || p.requirements != old.requirements
        || p.provider_id != old.provider_id
        || p.reference_ids != old.reference_ids
        || p.reference_mode != old.reference_mode
        || p.model != old.model
        || p.size != old.size
        || p.steps != old.steps
        || p.parameters != old.parameters
        || p.artist != old.artist
        || p.artist_source != old.artist_source
        || p.prompt != req.prompt
        || p.negative != req.negative
        || p.character_prompts
            != req
                .character_prompts
                .as_ref()
                .unwrap_or(&old.character_prompts)
                .as_slice()
    {
        return Err(shape(
            "编辑响应改变了提交的提示词、冻结身份、参考、画风或生成参数",
        ));
    }
    save_plan(
        &state.data_dir,
        &SavedPlan {
            version: 1,
            plugin_identity: saved.plugin_identity,
            input: wire,
            references: saved.references,
            plan: p.clone(),
        },
    )?;
    Ok(p)
}
pub fn plans_list(data_dir: &Path, limit: usize) -> AppResult<Vec<Plan>> {
    if limit == 0 || limit > 200 {
        return Err(AppError::bad_request("历史条数须为1至200"));
    }
    let mut plans = files::<SavedPlan>(data_dir, "plans")?;
    for p in &plans {
        validate_plan(&p.plan)?;
    }
    plans.sort_by(|a, b| b.plan.created_at.cmp(&a.plan.created_at));
    Ok(plans.into_iter().take(limit).map(|p| p.plan).collect())
}
pub fn plan_references(data_dir: &Path, plan_id: &str) -> AppResult<Vec<Reference>> {
    Ok(load_plan(data_dir, plan_id)?.references)
}
fn validate_against_request(run: &Run, req: &StartRequest, plan: &Plan) -> AppResult<()> {
    validate_start(req)?;
    validate_run(run)?;
    if run.plan != *plan
        || run.plan_id != req.plan_id
        || run.request_id != req.request_id
        || run.images.len() != req.count
    {
        return Err(shape("运行没有使用已冻结的计划、请求号或张数"));
    }
    if let Some(seed) = req.seed {
        for (i, img) in run.images.iter().enumerate() {
            if Some(img.seed) != seed.checked_add(i as u32) {
                return Err(shape("运行的图像种子不符合冻结请求"));
            }
        }
    }
    Ok(())
}
pub async fn start(state: &AppState, req: StartRequest) -> AppResult<Run> {
    validate_start(&req)?;
    static START_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _guard = START_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let saved = load_plan(&state.data_dir, &req.plan_id)?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved.plugin_identity, &conn)?;
    for old in files::<SavedRun>(&state.data_dir, "runs")? {
        if old.plugin_identity == identity(&conn) && old.run.request_id == req.request_id {
            let intent: StartIntent =
                read(&record_path(&state.data_dir, "starts", &req.request_id)?)?;
            if intent.request != req {
                return Err(AppError::bad_request("该请求号已经绑定另一组出图参数"));
            }
            validate_against_request(&old.run, &req, &saved.plan)?;
            return Ok(old.run);
        }
    }
    verify_current_references(state, &saved).await?;
    let cap = caps(state, &conn, true).await?;
    validate_execution(&cap, &saved.plan)?;
    let intent = StartIntent {
        version: 1,
        plugin_identity: identity(&conn),
        request: req.clone(),
    };
    atomic(
        &record_path(&state.data_dir, "starts", &req.request_id)?,
        &serde_json::to_vec(&intent)?,
        true,
    )?;
    let run: Run = call(
        state,
        &conn,
        reqwest::Method::POST,
        "start",
        &[],
        Some(&serde_json::to_value(&req)?),
        45,
    )
    .await?;
    validate_against_request(&run, &req, &saved.plan)?;
    save_run(
        &state.data_dir,
        &SavedRun {
            version: 1,
            plugin_identity: identity(&conn),
            run: run.clone(),
        },
    )?;
    Ok(run)
}
fn validate_evolution(old: &Run, new: &Run) -> AppResult<()> {
    validate_run(new)?;
    if old.run_id != new.run_id
        || old.plan != new.plan
        || old.request_id != new.request_id
        || old.images.len() != new.images.len()
        || old.created_at != new.created_at
        || (terminal(&old.status) && !terminal(&new.status))
        || old.images.iter().zip(&new.images).any(|(a, b)| {
            a.seed != b.seed
                || (a.file.is_some() && a.file != b.file)
                || (a.status == "done" && b.status != "done")
        })
    {
        return Err(shape("运行状态更新改变了已冻结的计划或图像种子"));
    }
    Ok(())
}
pub async fn status(state: &AppState, run_id: String) -> AppResult<Run> {
    let saved = load_run(&state.data_dir, &run_id)?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved.plugin_identity, &conn)?;
    let run: Run = call(
        state,
        &conn,
        reqwest::Method::GET,
        "status",
        &[("run_id", run_id)],
        None,
        30,
    )
    .await?;
    validate_evolution(&saved.run, &run)?;
    save_run(
        &state.data_dir,
        &SavedRun {
            version: 1,
            plugin_identity: saved.plugin_identity,
            run: run.clone(),
        },
    )?;
    Ok(run)
}
pub async fn cancel(state: &AppState, run_id: String) -> AppResult<Run> {
    let saved = load_run(&state.data_dir, &run_id)?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved.plugin_identity, &conn)?;
    let run: Run = call(
        state,
        &conn,
        reqwest::Method::POST,
        "cancel",
        &[],
        Some(&json!({"run_id":run_id})),
        45,
    )
    .await?;
    validate_evolution(&saved.run, &run)?;
    save_run(
        &state.data_dir,
        &SavedRun {
            version: 1,
            plugin_identity: saved.plugin_identity,
            run: run.clone(),
        },
    )?;
    Ok(run)
}
pub async fn list(state: &AppState, limit: usize) -> AppResult<Vec<Run>> {
    if limit == 0 || limit > 100 {
        return Err(AppError::bad_request("运行历史条数须为1至100"));
    }
    let local = files::<SavedRun>(&state.data_dir, "runs")?;
    for saved in &local {
        validate_run(&saved.run)?;
    }
    let mut merged: HashMap<String, Run> = local
        .iter()
        .map(|r| (r.run.run_id.clone(), r.run.clone()))
        .collect();
    match plugin::plugin_conn(state).await {
        Ok(conn) => {
            let remote: AppResult<Vec<Run>> = call(
                state,
                &conn,
                reqwest::Method::GET,
                "list",
                &[("limit", limit.to_string())],
                None,
                30,
            )
            .await;
            match remote {
                Ok(runs) => {
                    for run in runs {
                        validate_run(&run)?;
                        if let Some(old) = local.iter().find(|s| s.run.run_id == run.run_id) {
                            check_identity(&old.plugin_identity, &conn)?;
                            validate_evolution(&old.run, &run)?;
                        }
                        save_run(
                            &state.data_dir,
                            &SavedRun {
                                version: 1,
                                plugin_identity: identity(&conn),
                                run: run.clone(),
                            },
                        )?;
                        // Import no synthetic plan ID. Its real, immutable plan can be viewed, but
                        // a plan without local reference evidence cannot become a new paid call.
                        if !record_path(&state.data_dir, "plans", &run.plan_id)?.exists() {
                            save_plan(
                                &state.data_dir,
                                &SavedPlan {
                                    version: 1,
                                    plugin_identity: identity(&conn),
                                    input: Value::Null,
                                    references: Vec::new(),
                                    plan: run.plan.clone(),
                                },
                            )?;
                        }
                        merged.insert(run.run_id.clone(), run);
                    }
                }
                Err(e)
                    if !merged.is_empty()
                        && matches!(e.code.as_str(), "network" | "creative_unsupported") => {}
                Err(e) => return Err(e),
            }
        }
        Err(e)
            if !merged.is_empty()
                && matches!(e.code.as_str(), "network" | "bad_request" | "secret") => {}
        Err(e) => return Err(e),
    }
    let mut out: Vec<Run> = merged.into_values().collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out.truncate(limit);
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageResponse {
    name: String,
    mime: String,
    data_base64: String,
}
fn image_filename(run: &Run, idx: usize) -> AppResult<&str> {
    let image = run
        .images
        .get(idx)
        .ok_or_else(|| AppError::not_found("图片编号不存在"))?;
    if matches!(image.status.as_str(), "pending" | "running") && !terminal(&run.status) {
        return Err(image_pending());
    }
    if image.status == "done" {
        image
            .file
            .as_deref()
            .ok_or_else(|| shape("已完成图片缺少文件名"))
    } else {
        Err(AppError::not_found("这张图未成功生成"))
    }
}
pub async fn image(state: &AppState, run_id: String, idx: usize, full: bool) -> AppResult<String> {
    let saved = load_run(&state.data_dir, &run_id)?;
    let expected = image_filename(&saved.run, idx)?;
    let stem = format!("{idx}{}", if full { "" } else { ".thumb" });
    let dir = root(&state.data_dir).join("images").join(&run_id);
    for ext in ["png", "jpg", "webp"] {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.is_file() {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved.plugin_identity, &conn)?;
    let response: ImageResponse = call(
        state,
        &conn,
        reqwest::Method::GET,
        "image",
        &[
            ("run_id", run_id),
            ("idx", idx.to_string()),
            ("full", if full { "1".into() } else { "0".into() }),
        ],
        None,
        60,
    )
    .await?;
    if response.name != expected || response.data_base64.len() > 40 * 1024 * 1024 {
        return Err(shape("图片响应身份或大小不合法"));
    }
    let (ext, format) = match response.mime.as_str() {
        "image/png" => ("png", image::ImageFormat::Png),
        "image/jpeg" => ("jpg", image::ImageFormat::Jpeg),
        "image/webp" => ("webp", image::ImageFormat::WebP),
        _ => return Err(shape("不支持的图片类型")),
    };
    let bytes = STANDARD
        .decode(response.data_base64)
        .map_err(|_| shape("图片Base64不合法"))?;
    if image::guess_format(&bytes).map_err(|_| shape("图片内容不合法"))? != format {
        return Err(shape("图片实际格式与响应MIME不一致"));
    }
    let path = dir.join(format!("{stem}.{ext}"));
    atomic(&path, &bytes, true)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_caps() -> Capabilities {
        Capabilities {
            version: 1,
            full_reference_supported: false,
            free_only: true,
            free_sizes: FREE_SIZES
                .iter()
                .map(|(id, _, w, h)| FreeSize {
                    id: (*id).into(),
                    width: *w,
                    height: *h,
                })
                .collect(),
            providers: vec!["synthetic".into()],
            characters: vec!["fixture character a".into(), "fixture character b".into()],
            model: "nai-diffusion-5-full".into(),
            steps: 28,
            available: true,
            reason: None,
            render_available: false,
            render_reason: Some("no free account".into()),
            min_stamina_percent: 5.0,
            free_limitations: vec![],
        }
    }
    fn reference(id: &str, path: &str, tags: &str) -> Reference {
        Reference {
            entry_id: id.into(),
            title: id.into(),
            path: vec![path.into()],
            tags: tags.into(),
            negative: None,
            character_prompts: vec![],
            content_hash: format!("hash-{id}"),
            decision: "include".into(),
            rating: "safe".into(),
            prose: false,
        }
    }
    fn request() -> PlanRequest {
        PlanRequest {
            character: "".into(),
            requirements: "beach dancing".into(),
            provider_id: "synthetic".into(),
            orientation: "portrait".into(),
            scope: vec![],
            reference_ids: vec![],
            reference_count: 3,
            reference_mode: ReferenceMode::Sample,
            style_name: "".into(),
            artist_ratio: 0.5,
        }
    }
    fn fixture_plan() -> Plan {
        Plan {
            plan_id: "plan-fixture".into(),
            parent_plan_id: None,
            created_at: "2026-09-10T10:00:00Z".into(),
            character: "".into(),
            requirements: "beach dancing".into(),
            provider_id: "synthetic".into(),
            skill_version: SKILL_VERSION.into(),
            concept: "A dancer moves across an empty beach.".into(),
            prompt: "beach, dancing".into(),
            negative: "".into(),
            artist: "".into(),
            artist_source: "none".into(),
            character_prompts: vec![],
            reference_ids: vec!["ref1".into()],
            reference_mode: ReferenceMode::Sample,
            used_reference_ids: None,
            requirement_checks: vec![RequirementCheck {
                requirement: "beach dancing".into(),
                evidence: "beach, dancing".into(),
            }],
            warnings: vec![],
            model: "nai-diffusion-5-full".into(),
            size: "832x1216".into(),
            steps: 28,
            parameters: Parameters {
                scale: 5.0,
                cfg: 0.0,
                sampler: "k_euler_ancestral".into(),
                noise_schedule: "karras".into(),
                other: "0".into(),
            },
            free_only: true,
            llm_raw: "synthetic model response".into(),
        }
    }
    fn fixture_run() -> Run {
        Run {
            run_id: "run-fixture".into(),
            plan_id: "plan-fixture".into(),
            plan: fixture_plan(),
            request_id: "aa355a10-6291-4449-a28c-8c0b22cd5a6c".into(),
            status: "queued".into(),
            created_at: "2026-09-10T10:01:00Z".into(),
            finished_at: None,
            images: vec![CreativeImage {
                idx: 0,
                seed: 42,
                status: "pending".into(),
                file: None,
                error: None,
                score: None,
                notes: vec![],
                account_index: None,
                anlas_before: None,
                anlas_after: None,
                anlas_delta: None,
                stamina_before: None,
                stamina_after: None,
            }],
            error: None,
            free_only: true,
            anlas_spent: None,
            stamina_before: None,
            stamina_after: None,
            warnings: vec![],
            cancel_requested: false,
        }
    }

    #[test]
    fn blank_character_uses_current_plugin_pool_and_never_model_or_reference_fallback() {
        let cap = fixture_caps();
        let mut noisy_pool = cap.characters.clone();
        noisy_pool.extend(["".into(), "  \n ".into(), "x".repeat(1001)]);
        noisy_pool.extend(vec![cap.characters[0].clone(); 20]);
        noisy_pool.push(format!(" {} ", cap.characters[1]));
        let mut seen = HashSet::new();
        for i in 0..64 {
            let nonce = format!("concept-{i}");
            let chosen = plan_character("", &cap.characters, &nonce).unwrap();
            assert!(cap.characters.contains(&chosen));
            assert_eq!(
                chosen,
                plan_character(" \n\t", &noisy_pool, &nonce).unwrap()
            );
            let mut reordered = cap.characters.clone();
            reordered.reverse();
            assert_eq!(chosen, plan_character("", &reordered, &nonce).unwrap());
            seen.insert(chosen);
        }
        assert_eq!(seen.len(), 2, "新构思不能总是取列表第一人");
        let refreshed = vec!["new plugin pool character".into()];
        assert_eq!(
            plan_character("", &refreshed, "concept-0").unwrap(),
            refreshed[0]
        );
        for empty in [vec![], vec!["  ".into(), "x".repeat(1001)]] {
            assert_eq!(
                plan_character("", &empty, "concept-0").unwrap_err().code,
                "creative_character_pool_empty"
            );
            assert_eq!(
                plan_character(" explicit alias ", &empty, "concept-0").unwrap(),
                " explicit alias ",
                "明确填写仍由插件解析，不被空池覆盖"
            );
        }
    }

    #[test]
    fn random_character_is_frozen_and_response_cannot_restore_blank_or_replace_identity() {
        let cap = fixture_caps();
        let req = request();
        let refs = vec![reference("ref1", "sea", "beach")];
        let chosen = plan_character(&req.character, &cap.characters, "concept-1").unwrap();
        let mut p = fixture_plan();
        p.character = chosen.clone();
        validate_new_plan(&p, &req, &cap, &refs, &chosen).unwrap();
        assert!(req.character.is_empty(), "原始空输入不能被随机选角改写");
        let mut wrong = p.clone();
        for character in ["", "reference-only outsider"] {
            wrong.character = character.into();
            assert!(validate_new_plan(&wrong, &req, &cap, &refs, &chosen).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let saved = SavedPlan {
            version: 1,
            plugin_identity: "fixture".into(),
            input: json!({"character":chosen}),
            references: refs,
            plan: p.clone(),
        };
        save_plan(dir.path(), &saved).unwrap();
        let loaded = load_plan(dir.path(), &p.plan_id).unwrap();
        assert_eq!(loaded.plan.character, chosen);
        assert_eq!(loaded.input["character"], chosen);
        let mut run = fixture_run();
        run.plan = loaded.plan;
        let req = StartRequest {
            plan_id: p.plan_id.clone(),
            request_id: run.request_id.clone(),
            count: 1,
            seed: Some(42),
            score: false,
        };
        validate_against_request(&run, &req, &p).unwrap();
        run.plan.character = "another character".into();
        assert!(validate_against_request(&run, &req, &p).is_err());
    }

    #[test]
    fn references_obey_local_decisions_and_preserve_character_structure() {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migrate(&c).unwrap();
        c.execute_batch("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES('nai5_community_pack','string','fixture','1','[]'),('local_pack','string','local fixture','1','[]'),('local_artist','string','artist fixture','1','[]');").unwrap();
        for (id, rating, decision, removed, tags, cp) in [
            (
                "good",
                "safe",
                "include",
                None,
                "beach",
                r#"[{"label":"char1","prompt":"dancing, dynamic pose","negative":"seated"}]"#,
            ),
            (
                "prose",
                "safe",
                "include",
                None,
                "<artwork>\nA dancer near the sea.\n</artwork>",
                "[]",
            ),
            ("excluded", "safe", "exclude", None, "beach", "[]"),
            ("undecided", "safe", "undecided", None, "beach", "[]"),
            (
                "removed",
                "safe",
                "include",
                Some("2026-09-10"),
                "beach",
                "[]",
            ),
            ("unrated", "unknown", "include", None, "beach", "[]"),
            (
                "blocked",
                "safe",
                "include",
                None,
                "beach",
                r#"[{"label":"char1","prompt":"artist:blocked_artist, dancing"}]"#,
            ),
        ] {
            c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,character_prompts_json,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,'nai5_community_pack',0,?1,'[\"fixture\"]',?2,?3,?4,'1','1',?5)",rusqlite::params![id,tags,cp,rating,removed]).unwrap();
            c.execute("INSERT INTO library_membership(entry_id,library,decision,decided_at) VALUES(?1,'pack',?2,'now')",rusqlite::params![id,decision]).unwrap();
        }
        c.execute(
            "INSERT INTO artist_blacklist(name,added_at) VALUES('blocked_artist','now')",
            [],
        )
        .unwrap();
        c.execute_batch("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release) VALUES('local-safe','local_pack',0,'local safe','[\"local\"]','sunlight, dancing','safe','local','local'),('local-no','local_pack',0,'local no','[]','sunlight','safe','local','local'),('artist-no','local_artist',0,'artist','[]','artist:fixture','safe','local','local');
            INSERT INTO library_membership(entry_id,library,decision,decided_at) VALUES('local-safe','pack','include','now'),('local-no','pack','exclude','now'),('artist-no','artist','include','now');").unwrap();
        let rows = local_references(&c, &[]).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.entry_id.as_str()).collect::<Vec<_>>(),
            vec!["good", "local-safe", "prose"]
        );
        assert_eq!(rows[0].character_prompts[0].prompt, "dancing, dynamic pose");
        assert_eq!(
            rows[0].character_prompts[0].negative.as_deref(),
            Some("seated")
        );
        assert!(rows[2].prose);
        assert!(local_references(&c, &[vec!["other".into()]])
            .unwrap()
            .is_empty());
        let hash = rows[0].content_hash.clone();
        c.execute(
            "UPDATE codex_entry SET tags='new beach' WHERE entry_id='good'",
            [],
        )
        .unwrap();
        assert_ne!(local_references(&c, &[]).unwrap()[0].content_hash, hash);
        assert_eq!(
            c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            crate::db::SCHEMA_VERSION as u32
        );
    }
    #[test]
    fn selection_is_relevant_bounded_and_never_substitutes_invalid_picks() {
        let pool = vec![
            reference("beach", "sea", "beach dancing"),
            reference("room", "inside", "classroom"),
            reference("stage", "stage", "dancing"),
            reference("beach2", "sea", "beach dancing"),
        ];
        let req = request();
        let picked = select_references(pool.clone(), &req, "seed").unwrap();
        assert_eq!(picked.len(), 3);
        assert_eq!(picked[0].path, vec!["sea"]);
        assert!(picked.iter().any(|p| p.entry_id == "stage"));
        assert_eq!(
            picked,
            select_references(pool.clone(), &req, "seed").unwrap()
        );
        let mut invalid = req.clone();
        invalid.reference_ids = vec!["not-eligible".into()];
        assert!(select_references(pool.clone(), &invalid, "seed").is_err());
        invalid.reference_ids = vec!["room".into(), "room".into()];
        assert!(select_references(pool.clone(), &invalid, "seed").is_err());
        assert_eq!(
            select_references(vec![pool[0].clone()], &req, "seed")
                .unwrap()
                .len(),
            1
        );
        assert!(select_references(vec![], &req, "seed").is_err());
        let mut oversized = pool[0].clone();
        oversized.tags = "x".repeat(MAX_REFERENCE_BYTES);
        assert!(select_references(vec![oversized], &req, "seed").is_err());
    }
    #[test]
    fn all_references_preserves_complete_material_beyond_old_byte_budgets() {
        let pool: Vec<_> = (0..18)
            .map(|n| {
                let mut r = reference(
                    &format!("ref-{n}"),
                    "included-scope",
                    &"完整构图参考".repeat(1100),
                );
                r.negative = Some(format!("negative-{n}"));
                r.character_prompts = vec![CharacterPrompt {
                    label: "role".into(),
                    prompt: format!("role-{n}"),
                    negative: Some("no blur".into()),
                }];
                r
            })
            .collect();
        assert!(serde_json::to_vec(&pool).unwrap().len() > MAX_REFERENCE_BYTES);
        let mut req = request();
        req.reference_mode = ReferenceMode::All;
        req.reference_ids = vec![pool[13].entry_id.clone()];
        let selected = select_references(pool.clone(), &req, "seed").unwrap();
        assert_eq!(selected.len(), 18, "one priority never narrows all scope");
        let mut inactive_selection = req.clone();
        inactive_selection.reference_count = 0;
        inactive_selection.reference_ids = vec!["outside old sample scope".into(); 30];
        assert_eq!(
            select_references(pool.clone(), &inactive_selection, "seed").unwrap(),
            pool
        );
        for r in &pool {
            assert!(
                selected.contains(r),
                "full body, negative and role remain exact"
            );
        }
        req.reference_mode = ReferenceMode::Sample;
        assert_eq!(
            select_references(pool.clone(), &req, "seed").unwrap().len(),
            3
        );
        req.reference_mode = ReferenceMode::All;
        assert!(select_references(vec![], &req, "seed").is_err());
        req.reference_ids.clear();
        let large: Vec<_> = (0..256)
            .map(|i| {
                let mut r = pool[i % pool.len()].clone();
                r.entry_id = format!("full-{i}");
                r
            })
            .collect();
        assert!(serde_json::to_vec(&large).unwrap().len() > 4 * 1024 * 1024);
        assert_eq!(
            select_references(large.clone(), &req, "seed").unwrap(),
            large
        );
    }
    #[test]
    fn full_reference_capabilities_and_old_history_are_compatible_without_downgrade() {
        let old = fixture_caps();
        validate_reference_capability(&old, ReferenceMode::Sample).unwrap();
        assert_eq!(
            validate_reference_capability(&old, ReferenceMode::All)
                .unwrap_err()
                .code,
            "creative_full_reference_unsupported"
        );
        let mut current = old.clone();
        current.full_reference_supported = true;
        validate_reference_capability(&current, ReferenceMode::All).unwrap();
        let mut raw = serde_json::to_value(&current).unwrap();
        raw.as_object_mut()
            .unwrap()
            .remove("full_reference_supported");
        assert!(
            !serde_json::from_value::<Capabilities>(raw)
                .unwrap()
                .full_reference_supported
        );
        let mut raw = serde_json::to_value(fixture_plan()).unwrap();
        raw.as_object_mut().unwrap().remove("reference_mode");
        raw.as_object_mut().unwrap().remove("used_reference_ids");
        let old_plan: Plan = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(old_plan.reference_mode, ReferenceMode::Sample);
        assert_eq!(old_plan.used_reference_ids, None);
        validate_plan(&old_plan).unwrap();
        raw["reference_mode"] = json!("unknown");
        assert!(serde_json::from_value::<Plan>(raw).is_err());
        let mut raw_req = serde_json::to_value(request()).unwrap();
        raw_req.as_object_mut().unwrap().remove("reference_mode");
        assert_eq!(
            serde_json::from_value::<PlanRequest>(raw_req)
                .unwrap()
                .reference_mode,
            ReferenceMode::Sample
        );
        assert_eq!(ReferenceMode::All.timeout(), 600);
        assert_eq!(ReferenceMode::Sample.timeout(), 180);
    }
    #[test]
    fn full_plans_roundtrip_large_snapshots_and_validate_model_declared_subset() {
        let mut p = fixture_plan();
        p.reference_mode = ReferenceMode::All;
        let refs: Vec<_> = (0..18)
            .map(|n| reference(&format!("ref-{n}"), "scope", "full body"))
            .collect();
        p.reference_ids = refs.iter().map(|r| r.entry_id.clone()).collect();
        p.used_reference_ids = Some(vec!["ref-2".into(), "ref-12".into()]);
        validate_plan(&p).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let saved = SavedPlan {
            version: 1,
            plugin_identity: "synthetic".into(),
            input: json!({"reference_mode":"all"}),
            references: refs.clone(),
            plan: p.clone(),
        };
        save_plan(dir.path(), &saved).unwrap();
        assert_eq!(load_plan(dir.path(), &p.plan_id).unwrap().plan, p);
        let mut req = request();
        req.reference_mode = ReferenceMode::All;
        validate_new_plan(&p, &req, &fixture_caps(), &refs, "").unwrap();
        req.reference_mode = ReferenceMode::Sample;
        assert!(validate_new_plan(&p, &req, &fixture_caps(), &refs, "").is_err());
        for used in [vec!["unknown".into()], vec!["ref-2".into(), "ref-2".into()]] {
            let mut invalid = p.clone();
            invalid.used_reference_ids = Some(used);
            assert!(validate_plan(&invalid).is_err());
        }
        p.used_reference_ids = Some(vec![]);
        validate_plan(&p).unwrap();
        p.reference_mode = ReferenceMode::Sample;
        assert!(validate_plan(&p).is_err());
    }
    #[test]
    fn frozen_plan_rejects_paid_shapes_and_preserves_real_request_identity() {
        let p = fixture_plan();
        validate_plan(&p).unwrap();
        for size in ["1024x1536", "832x1216 ", "auto"] {
            let mut bad = p.clone();
            bad.size = size.into();
            assert!(validate_plan(&bad).is_err());
        }
        let mut bad = p.clone();
        bad.steps = 29;
        assert!(validate_plan(&bad).is_err());
        bad = p.clone();
        bad.free_only = false;
        assert!(validate_plan(&bad).is_err());
        bad = p.clone();
        bad.parameters.other = "smea".into();
        assert!(validate_plan(&bad).is_err());
        bad = p.clone();
        bad.requirement_checks.clear();
        assert!(validate_plan(&bad).is_err());
        bad = p.clone();
        bad.artist = "artist:fixture".into();
        assert!(validate_plan(&bad).is_err());
        let mut raw = serde_json::to_value(&p).unwrap();
        raw["paid_fallback"] = json!(true);
        assert!(serde_json::from_value::<Plan>(raw).is_err());
        let run = fixture_run();
        let req = StartRequest {
            plan_id: p.plan_id.clone(),
            request_id: run.request_id.clone(),
            count: 1,
            seed: Some(42),
            score: false,
        };
        validate_against_request(&run, &req, &p).unwrap();
        let mut overflow = req.clone();
        overflow.seed = Some(u32::MAX);
        validate_start(&overflow).unwrap();
        overflow.count = 2;
        assert!(validate_start(&overflow).is_err());
        let edit = EditRequest {
            plan_id: p.plan_id.clone(),
            prompt: p.prompt.clone(),
            negative: p.negative.clone(),
            character_prompts: None,
        };
        assert!(
            serde_json::to_value(edit)
                .unwrap()
                .get("character_prompts")
                .is_none(),
            "省略角色编辑必须保留原角色，不能向插件发送null"
        );
        let mut changed = run.clone();
        changed.images[0].seed = 43;
        assert!(validate_against_request(&changed, &req, &p).is_err());
        changed = run.clone();
        changed.plan.prompt = "classroom, seated".into();
        assert!(validate_evolution(&run, &changed).is_err());
        changed = run.clone();
        changed.images[0].file = Some("../../secret".into());
        assert!(validate_run(&changed).is_err());
        // Unknown cost stays null. Real nonzero observations are not erased by the client.
        changed = run;
        changed.anlas_spent = Some(2.0);
        validate_run(&changed).unwrap();
    }
    #[test]
    fn immutable_local_records_and_start_intents_cannot_be_rebound() {
        let dir = tempfile::tempdir().unwrap();
        let saved = SavedPlan {
            version: 1,
            plugin_identity: "fixture".into(),
            input: json!({"exact":"input"}),
            references: vec![reference("ref1", "sea", "beach")],
            plan: fixture_plan(),
        };
        save_plan(dir.path(), &saved).unwrap();
        save_plan(dir.path(), &saved).unwrap();
        let mut changed = saved.clone();
        changed.plan.prompt = "different".into();
        assert!(save_plan(dir.path(), &changed).is_err());
        assert_eq!(
            load_plan(dir.path(), "plan-fixture").unwrap().plan.prompt,
            "beach, dancing"
        );
        let req = StartRequest {
            plan_id: "plan-fixture".into(),
            request_id: "aa355a10-6291-4449-a28c-8c0b22cd5a6c".into(),
            count: 1,
            seed: None,
            score: false,
        };
        let path = record_path(dir.path(), "starts", &req.request_id).unwrap();
        atomic(&path, &serde_json::to_vec(&req).unwrap(), true).unwrap();
        let mut changed = req;
        changed.count = 2;
        assert!(atomic(&path, &serde_json::to_vec(&changed).unwrap(), true).is_err());
        assert!(record_path(dir.path(), "plans", "../secrets").is_err());
        assert_eq!(plans_list(dir.path(), 30).unwrap().len(), 1);
        assert_eq!(
            plan_references(dir.path(), "plan-fixture").unwrap(),
            saved.references
        );
    }
    #[test]
    fn capability_and_wire_errors_fail_closed_without_old_route_fallback() {
        assert_eq!(
            decode::<Capabilities>(reqwest::StatusCode::NOT_FOUND, "missing")
                .unwrap_err()
                .code,
            "creative_unsupported"
        );
        assert!(decode::<Capabilities>(
            reqwest::StatusCode::OK,
            r#"{"status":"error","message":"no free budget"}"#
        )
        .is_err());
        let c = Capabilities {
            version: 1,
            full_reference_supported: false,
            free_only: true,
            free_sizes: FREE_SIZES
                .iter()
                .map(|(id, _, w, h)| FreeSize {
                    id: (*id).into(),
                    width: *w,
                    height: *h,
                })
                .collect(),
            providers: vec!["synthetic".into()],
            characters: vec![],
            model: "nai-diffusion-5-full".into(),
            steps: 28,
            available: true,
            reason: None,
            render_available: false,
            render_reason: Some("no free account".into()),
            min_stamina_percent: 5.0,
            free_limitations: vec![],
        };
        validate_caps(&c, true).unwrap();
        let mut changed_defaults = c.clone();
        changed_defaults.render_available = true;
        changed_defaults.providers.clear(); // Executing a frozen plan needs no prompt LLM.
        let mut frozen = fixture_plan();
        frozen.steps = 23;
        validate_execution(&changed_defaults, &frozen).unwrap();
        assert_eq!(frozen.steps, 23, "全局默认改为28步不会改变旧计划的23步");
        frozen.steps = 29;
        assert!(validate_execution(&changed_defaults, &frozen).is_err());
        let mut bad = c.clone();
        bad.free_sizes[0].width = 1024;
        assert!(validate_caps(&bad, true).is_err());
        bad = c;
        bad.free_only = false;
        assert!(validate_caps(&bad, true).is_err());
        assert_eq!(
            digest(SKILL_TEXT.as_bytes()),
            "270aa64fbf76f95cf786ea0ba3ac20a263686ec371124e30e4620da5620f8684"
        );
    }

    #[test]
    fn image_readiness_is_distinct_from_missing_interfaces_files_and_authentication() {
        let pending = r#"{"status":"error","message":"这张图片尚未生成"}"#;
        assert_eq!(
            decode_route::<Value>("image", reqwest::StatusCode::NOT_FOUND, pending)
                .unwrap_err()
                .code,
            "creative_image_pending"
        );
        // Only that image response is temporary: never hide unrelated failures.
        for (route, status, body, code) in [
            (
                "status",
                reqwest::StatusCode::NOT_FOUND,
                pending,
                "not_found",
            ),
            ("image", reqwest::StatusCode::OK, pending, "plugin_rejected"),
            (
                "image",
                reqwest::StatusCode::UNAUTHORIZED,
                pending,
                "plugin_rejected",
            ),
            (
                "image",
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                pending,
                "plugin_rejected",
            ),
            (
                "image",
                reqwest::StatusCode::NOT_FOUND,
                r#"{"status":"error","message":"图片文件不存在"}"#,
                "not_found",
            ),
            (
                "image",
                reqwest::StatusCode::NOT_FOUND,
                r#"{"status":"error","message":"记录不存在"}"#,
                "not_found",
            ),
            (
                "capabilities",
                reqwest::StatusCode::NOT_FOUND,
                r#"{"detail":"Not Found"}"#,
                "creative_unsupported",
            ),
            (
                "image",
                reqwest::StatusCode::OK,
                "broken json",
                "creative_protocol",
            ),
        ] {
            let error = decode_route::<Value>(route, status, body).unwrap_err();
            assert_eq!(error.code, code, "{route}: {status}, {body}");
        }
        let body = json!({"status":"ok","data":{
            "name":"fixture.png", "mime":"image/png", "data_base64":"synthetic"
        }})
        .to_string();
        let ready: ImageResponse = decode_route("image", reqwest::StatusCode::OK, &body).unwrap();
        assert_eq!(ready.name, "fixture.png");
        assert_eq!(ready.data_base64, "synthetic");
    }

    #[test]
    fn only_existing_images_still_in_progress_have_a_pending_local_state() {
        let mut run = fixture_run();
        assert_eq!(
            image_filename(&run, 0).unwrap_err().code,
            "creative_image_pending"
        );
        assert_eq!(image_filename(&run, 1).unwrap_err().code, "not_found");
        run.status = "running".into();
        run.images[0].status = "running".into();
        assert_eq!(
            image_filename(&run, 0).unwrap_err().code,
            "creative_image_pending"
        );
        run.images[0].status = "done".into();
        run.images[0].file = Some("fixture.png".into());
        assert_eq!(image_filename(&run, 0).unwrap(), "fixture.png");
        run.status = "done".into();
        assert_eq!(image_filename(&run, 0).unwrap(), "fixture.png");
        for status in ["error", "cancelled"] {
            run.status = status.into();
            run.images[0].status = status.into();
            run.images[0].file = None;
            assert_eq!(image_filename(&run, 0).unwrap_err().code, "not_found");
        }
    }

    #[test]
    #[ignore = "requires CREATIVE_FULL_FIXTURE_DIR from plugin synthetic full-reference tests"]
    fn plugin_full_reference_fixtures_preserve_all_material_through_edit_and_revalidation() {
        let dir = PathBuf::from(
            std::env::var("CREATIVE_FULL_FIXTURE_DIR")
                .expect("Generate full-reference synthetic plugin fixtures"),
        );
        let cap: Capabilities = read(&dir.join("capabilities.json")).unwrap();
        validate_caps(&cap, true).unwrap();
        validate_reference_capability(&cap, ReferenceMode::All).unwrap();
        let p: Plan = read(&dir.join("plan.json")).unwrap();
        let edited: Plan = read(&dir.join("edited-plan.json")).unwrap();
        let references: Vec<Reference> = read(&dir.join("references.json")).unwrap();
        assert!(references.len() > 8);
        assert!(serde_json::to_vec(&references).unwrap().len() > 4 * 1024 * 1024);
        validate_plan(&p).unwrap();
        validate_plan(&edited).unwrap();
        assert_eq!(p.reference_mode, ReferenceMode::All);
        assert_eq!(edited.reference_mode, ReferenceMode::All);
        assert_eq!(
            p.reference_ids,
            references
                .iter()
                .map(|r| r.entry_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(edited.reference_ids, p.reference_ids);
        assert_eq!(edited.parent_plan_id.as_ref(), Some(&p.plan_id));
        assert!(p.used_reference_ids.is_some());
        assert!(edited.used_reference_ids.is_some());
        let mut req = request();
        req.reference_mode = ReferenceMode::All;
        req.character = p.character.clone();
        req.requirements = p.requirements.clone();
        req.provider_id = p.provider_id.clone();
        req.orientation = FREE_SIZES
            .iter()
            .find(|(_, size, _, _)| *size == p.size)
            .unwrap()
            .0
            .into();
        validate_new_plan(&p, &req, &cap, &references, &p.character).unwrap();
        let temp = tempfile::tempdir().unwrap();
        for plan in [p, edited] {
            let saved = SavedPlan {
                version: 1,
                plugin_identity: "synthetic-full-plugin".into(),
                input: json!({"reference_mode":"all"}),
                references: references.clone(),
                plan: plan.clone(),
            };
            save_plan(temp.path(), &saved).unwrap();
            let restored = load_plan(temp.path(), &plan.plan_id).unwrap();
            assert_eq!(restored.plan, plan);
            assert_eq!(restored.references, references);
            validate_current_references(&restored, &references).unwrap();
            let omitted = references
                .iter()
                .rev()
                .find(|r| {
                    !plan
                        .used_reference_ids
                        .as_ref()
                        .unwrap()
                        .contains(&r.entry_id)
                })
                .unwrap();
            let current: Vec<_> = references
                .iter()
                .filter(|r| r.entry_id != omitted.entry_id)
                .cloned()
                .collect();
            assert!(
                validate_current_references(&restored, &current).is_err(),
                "all supplied references, including non-borrowed ones, must remain eligible"
            );
        }
    }

    /// Generated by the plugin repository's creative integration check with CREATIVE_WIRE_FIXTURE_DIR.
    /// The explicit ignored flag prevents an absent cross-language fixture from looking
    /// like a successful integration test in normal offline unit-test runs.
    #[test]
    #[ignore = "requires CREATIVE_WIRE_FIXTURE_DIR containing isolated synthetic plugin responses"]
    fn plugin_wire_fixtures_match_strict_native_protocol() {
        let dir = PathBuf::from(
            std::env::var("CREATIVE_WIRE_FIXTURE_DIR")
                .expect("Generate synthetic plugin fixtures and set CREATIVE_WIRE_FIXTURE_DIR"),
        );
        let cap: Capabilities = decode(
            reqwest::StatusCode::OK,
            &fs::read_to_string(dir.join("capabilities.json")).unwrap(),
        )
        .unwrap();
        validate_caps(&cap, true).unwrap();
        for file in ["plan.json", "edit.json"] {
            let plan: Plan = decode(
                reqwest::StatusCode::OK,
                &fs::read_to_string(dir.join(file)).unwrap(),
            )
            .unwrap();
            validate_plan(&plan).unwrap();
        }
        let runs: Vec<Run> = decode(
            reqwest::StatusCode::OK,
            &fs::read_to_string(dir.join("runs.json")).unwrap(),
        )
        .unwrap();
        assert!(!runs.is_empty());
        for run in &runs {
            validate_run(run).unwrap_or_else(|error| panic!("run {}: {error}", run.run_id));
        }
        assert!(runs.iter().any(|r| r.status == "done"));
        assert!(runs.iter().any(|r| r.status == "error"));
        assert!(runs.iter().any(|r| r.anlas_spent.is_none()));
    }

    #[test]
    #[ignore = "requires CREATIVE_WIRE_FIXTURE_DIR from the image-readiness integration checker"]
    fn installed_plugin_image_readiness_responses_have_the_correct_desktop_states() {
        let dir = PathBuf::from(
            std::env::var("CREATIVE_WIRE_FIXTURE_DIR")
                .expect("Generate synthetic plugin fixtures and set CREATIVE_WIRE_FIXTURE_DIR"),
        );
        let fixtures: Value =
            serde_json::from_slice(&fs::read(dir.join("image-readiness.json")).unwrap()).unwrap();
        let response = |name: &str| {
            let item = &fixtures[name];
            (
                reqwest::StatusCode::from_u16(item["http_status"].as_u64().unwrap() as u16)
                    .unwrap(),
                item["body"].to_string(),
            )
        };
        for (name, code) in [
            ("pending", "creative_image_pending"),
            ("missing_file", "not_found"),
            ("missing_run", "not_found"),
            ("bad_index", "plugin_rejected"),
        ] {
            let (status, body) = response(name);
            assert_eq!(
                decode_route::<Value>("image", status, &body)
                    .unwrap_err()
                    .code,
                code
            );
        }
        let (status, body) = response("status_while_pending");
        let pending: Run = decode_route("status", status, &body).unwrap();
        validate_run(&pending).unwrap();
        assert_eq!(pending.status, "running");
        assert_eq!(pending.images[0].status, "done");
        assert!(image_filename(&pending, 0).is_ok());
        // The plugin also uses its temporary message for an unused image slot;
        // native local validation must stop that invalid index before any HTTP request.
        assert_eq!(
            image_filename(&pending, pending.images.len())
                .unwrap_err()
                .code,
            "not_found"
        );
        let (status, body) = response("ready");
        let ready: ImageResponse = decode_route("image", status, &body).unwrap();
        assert_eq!(ready.name, image_filename(&pending, 0).unwrap());
        assert_eq!(
            image::guess_format(&STANDARD.decode(ready.data_base64).unwrap()).unwrap(),
            image::ImageFormat::Jpeg
        );
        let (status, body) = response("status_ready");
        let finished: Run = decode_route("status", status, &body).unwrap();
        validate_evolution(&pending, &finished).unwrap();
        assert_eq!(finished.status, "done");
    }
}
