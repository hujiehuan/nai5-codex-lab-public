//! 前后端 DTO。形状以 `src/types.ts` 为权威，逐字段一致，全部 snake_case，
//! serde 不对字段名做 rename（枚举值按契约用小写字符串）。
//! 数据库行 → DTO 的转换放在各自模块里，这里只放纯数据结构。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------- 反推原图（§14） ----------
#[derive(Serialize, Debug, PartialEq)]
pub struct SourceImageResult {
    pub path: Option<String>,
    pub kind: String,
}

// ---------- 基础枚举 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Include,
    Exclude,
    Undecided,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Include => "include",
            Decision::Exclude => "exclude",
            Decision::Undecided => "undecided",
        }
    }
    pub fn parse(s: &str) -> Option<Decision> {
        match s {
            "include" => Some(Decision::Include),
            "exclude" => Some(Decision::Exclude),
            "undecided" => Some(Decision::Undecided),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionVia {
    Click,
    Key,
    Batch,
    Import,
    Undo,
    Rule,
}

impl DecisionVia {
    pub fn as_str(&self) -> &'static str {
        match self {
            DecisionVia::Click => "click",
            DecisionVia::Key => "key",
            DecisionVia::Batch => "batch",
            DecisionVia::Import => "import",
            DecisionVia::Undo => "undo",
            DecisionVia::Rule => "rule",
        }
    }
    pub fn parse(s: &str) -> DecisionVia {
        match s {
            "click" => DecisionVia::Click,
            "key" => DecisionVia::Key,
            "batch" => DecisionVia::Batch,
            "import" => DecisionVia::Import,
            "undo" => DecisionVia::Undo,
            _ => DecisionVia::Rule,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// 只收 `safe` 分级的条目：其它分级的上游条目在解析时丢弃，库里只会有 `safe`。
pub enum Rating {
    Safe,
}

impl Rating {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rating::Safe => "safe",
        }
    }
    pub fn parse(s: &str) -> Option<Rating> {
        match s {
            "safe" => Some(Rating::Safe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncPhase {
    Idle,
    Manifest,
    Entries,
    Images,
    /// v0.3.0（§12.2）：图片阶段之后给展示图算画风向量；`style_index_rebuild` 单独跑时也是这一相。
    Embedding,
    Done,
    Error,
}

// ---------- 应用 / 设置 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    pub app_data_dir: String,
    pub db_path: String,
    pub latest_release: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StyleDevicePreference {
    #[default]
    Auto,
    Metal,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StyleDevice {
    Cpu,
    Metal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub plugin_base_url: String,
    pub webui_username: String,
    pub download_concurrency: u32,
    #[serde(default)]
    pub style_device: StyleDevicePreference,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            plugin_base_url: "http://127.0.0.1:6185".into(),
            webui_username: "astrbot".into(),
            download_concurrency: 4,
            style_device: StyleDevicePreference::Auto,
        }
    }
}

/// `Partial<Settings>`：命令层接收 JSON 对象，逐键校验，未知键 → bad_request。
pub type SettingsPatch = Value;

// ---------- 同步 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub running: bool,
    pub phase: SyncPhase,
    pub release_local: Option<String>,
    pub release_remote: Option<String>,
    pub images_total: u64,
    pub images_done: u64,
    pub images_failed: u64,
    pub message: String,
}

impl Default for SyncStatus {
    fn default() -> Self {
        SyncStatus {
            running: false,
            phase: SyncPhase::Idle,
            release_local: None,
            release_remote: None,
            images_total: 0,
            images_done: 0,
            images_failed: 0,
            message: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEvent {
    pub phase: SyncPhase,
    pub message: String,
    pub release: Option<String>,
    pub added: u64,
    pub changed: u64,
    pub images_total: u64,
    pub images_done: u64,
    pub images_failed: u64,
    /// v0.3.0（§12.2）：画风索引阶段的进度；老客户端不读、老事件缺键按 0。
    #[serde(default)]
    pub embed_total: u64,
    #[serde(default)]
    pub embed_done: u64,
    #[serde(default)]
    pub embed_failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFailure {
    pub url: String,
    pub error: String,
    /// v0.1.1 追加：`AppError.code`（network / upstream / io / cancelled / checksum / internal）。
    /// 旧 `sync_log.failures_json` 没有这个键，反序列化时补空串。
    #[serde(default)]
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncLog {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub kind: String,
    pub release_id: Option<String>,
    pub added: i64,
    pub changed: i64,
    pub images_ok: i64,
    pub images_failed: i64,
    pub failures: Vec<SyncFailure>,
}

// ---------- 书与条目 ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeNode {
    pub name: String,
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFilter {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecisionCounts {
    pub include: i64,
    pub exclude: i64,
    pub undecided: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BookCounts {
    pub include: i64,
    pub exclude: i64,
    pub undecided: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Book {
    pub codex_id: String,
    pub r#type: String,
    pub title: String,
    pub version: String,
    pub author: Option<String>,
    pub entry_count: i64,
    pub tree: Vec<TreeNode>,
    pub update_filters: Vec<UpdateFilter>,
    pub counts: BookCounts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntryQuery {
    pub codex_id: String,
    #[serde(default)]
    pub rating: Option<Vec<Rating>>,
    #[serde(default)]
    pub decision: Option<Vec<Decision>>,
    #[serde(default)]
    pub batch: Option<String>,
    #[serde(default)]
    pub first_seen_release: Option<String>,
    #[serde(default)]
    pub path_prefix: Option<Vec<String>>,
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub multi_image_only: Option<bool>,
    #[serde(default)]
    pub dup_only: Option<bool>,
    /// v0.1.2：`book_order | title | decided_at | synced_at | first_seen`（见 `EntrySort`）；
    /// 留成字符串在 `list_entries` 里校验，未知值报 `bad_request` 而不是 serde 反序列化错误。
    #[serde(default)]
    pub sort: Option<String>,
    /// v0.1.2：`asc | desc`；缺省按 `EntrySort::default_desc`。
    #[serde(default)]
    pub sort_dir: Option<String>,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub limit: i64,
}

/// `EntryQuery.sort` 的合法值（CONTRACT §7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySort {
    BookOrder,
    Title,
    DecidedAt,
    SyncedAt,
    FirstSeen,
}

impl EntrySort {
    pub const ALL: [EntrySort; 5] = [EntrySort::BookOrder, EntrySort::Title, EntrySort::DecidedAt, EntrySort::SyncedAt, EntrySort::FirstSeen];

    pub fn as_str(&self) -> &'static str {
        match self {
            EntrySort::BookOrder => "book_order",
            EntrySort::Title => "title",
            EntrySort::DecidedAt => "decided_at",
            EntrySort::SyncedAt => "synced_at",
            EntrySort::FirstSeen => "first_seen",
        }
    }
    pub fn parse(s: &str) -> Option<EntrySort> {
        EntrySort::ALL.iter().copied().find(|k| k.as_str() == s)
    }
    /// 缺省方向（§7）：`book_order` / `title` / `synced_at` 升序，`decided_at` / `first_seen` 降序。
    pub fn default_desc(&self) -> bool {
        matches!(self, EntrySort::DecidedAt | EntrySort::FirstSeen)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryCard {
    pub entry_id: String,
    pub codex_id: String,
    pub book_order: i64,
    pub title: String,
    pub tags_first_line: String,
    pub rating: Rating,
    pub batches: Vec<String>,
    pub path: Vec<String>,
    pub decision: Decision,
    pub thumb_path: Option<String>,
    pub image_path: Option<String>,
    pub image_w: Option<i64>,
    pub image_h: Option<i64>,
    pub image_count: i64,
    pub dup_count: i64,
    pub is_new: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPage {
    pub total: i64,
    pub counts: DecisionCounts,
    pub items: Vec<EntryCard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterPrompt {
    pub label: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub entry_id: String,
    pub codex_id: String,
    pub book_order: i64,
    pub entry_version: i64,
    pub title: String,
    pub path: Vec<String>,
    pub tags: String,
    pub negative: Option<String>,
    pub character_prompts: Option<Vec<CharacterPrompt>>,
    pub note: Option<String>,
    pub rating: Rating,
    pub is_new: bool,
    pub update_batches: Vec<String>,
    pub asset_rev: Option<String>,
    pub image_w: Option<i64>,
    pub image_h: Option<i64>,
    pub first_seen_release: String,
    pub last_seen_release: String,
    pub removed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryImage {
    pub ordinal: i64,
    pub image_file: String,
    pub original_file: Option<String>,
    pub raw_tag: Option<String>,
    pub local_path: Option<String>,
    pub thumb_path: Option<String>,
    pub original_local_path: Option<String>,
    pub bytes: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Artist,
    Year,
    Quality,
    Negative,
    Style,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecipeToken {
    pub value: String,
    pub weight: f64,
    pub kind: TokenKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtistWeighted {
    pub name: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtistParse {
    pub artists: Vec<ArtistWeighted>,
    pub auxiliary: Vec<RecipeToken>,
    pub negative_tail: Vec<RecipeToken>,
    pub signature: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRow {
    pub id: i64,
    pub from_decision: Option<Decision>,
    pub to_decision: Decision,
    pub at: String,
    pub via: DecisionVia,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sibling {
    pub entry_id: String,
    pub codex_id: String,
    pub title: String,
    pub decision: Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membership {
    pub decision: Decision,
    pub member_note: Option<String>,
    pub decided_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryDetail {
    pub entry: Entry,
    pub images: Vec<EntryImage>,
    pub membership: Option<Membership>,
    pub audit: Vec<AuditRow>,
    pub parse: Option<ArtistParse>,
    pub siblings: Vec<Sibling>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoResult {
    pub entry_id: String,
    pub restored: Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedResult {
    pub changed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistItem {
    pub name: String,
    pub reason: Option<String>,
    pub added_at: String,
}

// ---------- 导出 / 推送 ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportOptions {
    #[serde(default)]
    pub out_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFile {
    pub name: String,
    pub count: i64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub dir: String,
    pub files: Vec<ExportFile>,
    pub skipped: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    pub ok: bool,
    pub nai5: i64,
    pub pack: i64,
    pub dropped_blacklist: i64,
    pub bak: Vec<String>,
    pub message: String,
    pub plugin_response: Value,
}

/// v0.1.4 完整池按书计数（CONTRACT §8.1 `counts[codex_id]`；插件 `import_full` 原样回传同形）。
/// `#[serde(default)]`：插件回传缺键时不整体反序列化失败。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullCounts {
    pub total: i64,
    pub include: i64,
    pub exclude: i64,
    pub undecided: i64,
    pub blacklisted: i64,
}

/// `push_full_to_plugin` 返回（CONTRACT §8.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushFullResult {
    pub ok: bool,
    pub message: String,
    pub total: i64,
    pub counts: BTreeMap<String, FullCounts>,
    pub bak: Vec<String>,
    pub plugin_response: Value,
}

/// 插件端一种池的持有状态（CONTRACT §8.2 `PoolStatus`；由插件 GET `/page/library/status` 返回，字段缺失按默认值）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PoolStatus {
    pub imported_at: String,
    pub release_id: Option<String>,
    pub exported_at: Option<String>,
    pub app_version: Option<String>,
    pub counts: BTreeMap<String, i64>,
    pub file: String,
    pub size_bytes: i64,
    pub backups: Vec<String>,
}

/// `plugin_library_status` 返回（CONTRACT §8.2）：连不上 / 未配置时 `reachable=false` 带原因，不抛错。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLibraryStatus {
    pub reachable: bool,
    pub message: String,
    pub plugin_version: Option<String>,
    pub filtered: Option<PoolStatus>,
    pub full: Option<PoolStatus>,
}

// ---------- 画图实验室（CONTRACT §9.1 / §9.3 / §9.4，v0.2.0）----------
//
// 这些 DTO 两端共用：桌面端把 `LabRunRequest` 原样 POST 给插件，插件返回的 `LabPlan` / `LabRun` / `LabRunSummary`
// 原样透传给前端并镜像进本地 `lab_run` 表。可空字段一律 `Option` + `#[serde(default)]`（插件缺键按 null），
// 数组字段 `#[serde(default)]`（缺键按空）；必填标量保持必填，形状不对整体报错而不是静默补零。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabMode {
    Draw,
}

impl LabMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            LabMode::Draw => "draw",
        }
    }
    pub fn parse(s: &str) -> Option<LabMode> {
        match s {
            "draw" => Some(LabMode::Draw),
            _ => None,
        }
    }
}

/// `LabRun.status`：`done` / `error` 为终态，桌面端只在终态落盘（§9.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabRunStatus {
    Queued,
    Running,
    Done,
    Error,
}

impl LabRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LabRunStatus::Queued => "queued",
            LabRunStatus::Running => "running",
            LabRunStatus::Done => "done",
            LabRunStatus::Error => "error",
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, LabRunStatus::Done | LabRunStatus::Error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabImageStatus {
    Pending,
    Done,
    Error,
}

/// 画师串来源：`style` = `风格=`；`pool` = 画风池掷中；`none` = 无。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabArtistSource {
    Style,
    Pool,
    None,
}

/// 单张图的人工评价：收藏 / 点踩；未评价为 null（`Option<LabRating>`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabRating {
    Fav,
    Dis,
}

/// §9.1 `LabRunRequest`：前端拼好、后端原样转发给插件（只校验 `count` 1..8 与 `artist_ratio` 0..1）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabRunRequest {
    pub mode: LabMode,
    pub text: String,
    #[serde(default)]
    pub entry_id: Option<String>,
    #[serde(default)]
    pub scope: Vec<Vec<String>>,
    pub artist_ratio: f64,
    pub count: i64,
    pub score: bool,
    #[serde(default)]
    pub seed: Option<i64>,
}

/// §9.1 `LabPlan`：一张图的组装结果（预览与出图共用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabPlan {
    pub idx: i64,
    pub character: String,
    #[serde(default)]
    pub pack_entry_id: Option<String>,
    #[serde(default)]
    pub pack_title: Option<String>,
    #[serde(default)]
    pub pack_path: Option<Vec<String>>,
    pub tags_raw: String,
    pub tags_used: String,
    #[serde(default)]
    pub dropped: Vec<String>,
    #[serde(default)]
    pub extra_tags: Option<String>,
    pub prompt: String,
    pub negative: String,
    pub artist: String,
    pub artist_source: LabArtistSource,
    pub size: String,
    #[serde(default)]
    pub seed: Option<i64>,
    pub model: String,
    /// §11.3：点名的图包是散文条目（tags_used 原文、dropped 空）；插件对非散文恒 false，老记录缺键 → false。
    /// 没有这一字段 serde 会把插件回的 `prose` 键丢掉，前端 `LabResultCard` 的「散文」徽标就永远出不来。
    #[serde(default)]
    pub prose: bool,
}

/// §9.1 `LabImage = LabPlan & {...}`：JSON 是**扁平**的（`plan` 用 `#[serde(flatten)]` 铺开），Rust 侧用 `image.plan.prompt` 访问。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabImage {
    #[serde(flatten)]
    pub plan: LabPlan,
    pub status: LabImageStatus,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub rating: Option<LabRating>,
}

/// §9.1 `LabRun`：插件 `runs/<run_id>.json` 的全量记录；`done|error` 时整份存进本地 `lab_run.json`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabRun {
    pub run_id: String,
    pub mode: LabMode,
    pub request: LabRunRequest,
    pub status: LabRunStatus,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub stamina_before: Option<f64>,
    #[serde(default)]
    pub stamina_after: Option<f64>,
    #[serde(default)]
    pub images: Vec<LabImage>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub plugin_version: String,
}

/// §9.1 `LabRunSummary`：插件 `index.jsonl` 每行一条；本地行由 `lab::summary_of` 从 `LabRun` 推导。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabRunSummary {
    pub run_id: String,
    pub mode: LabMode,
    pub status: LabRunStatus,
    pub created_at: String,
    pub count: i64,
    pub done: i64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub pack_title: Option<String>,
    #[serde(default)]
    pub best_score: Option<f64>,
}

/// `lab_run_start` 返回（§9.3 `POST /page/lab/run/start` → `{ run_id, plans }`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabRunStart {
    pub run_id: String,
    #[serde(default)]
    pub plans: Vec<LabPlan>,
}

// ---------- 反推实验室 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseTemplate {
    pub template_id: i64,
    pub name: String,
    pub version: i64,
    pub body: String,
    pub parent_id: Option<i64>,
    pub created_at: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseTemplateInput {
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseRuleset {
    pub ruleset_id: i64,
    pub name: String,
    pub version: i64,
    pub banned: Vec<String>,
    pub appearance_regex: String,
    pub accessory_ok_regex: String,
    pub refusal_regex: String,
    pub always_drop: Vec<String>,
    pub max_words: i64,
    pub max_len: i64,
    pub created_at: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseRulesetInput {
    pub name: String,
    pub banned: Vec<String>,
    pub appearance_regex: String,
    pub accessory_ok_regex: String,
    pub refusal_regex: String,
    pub always_drop: Vec<String>,
    pub max_words: i64,
    pub max_len: i64,
    #[serde(default)]
    pub note: Option<String>,
}

/// `ReverseProvider.kind`（CONTRACT §10.1）：本地 OpenAI 兼容端点 / AstrBot 插件里配置好的 provider。
pub const PROVIDER_KIND_OPENAI: &str = "openai";
pub const PROVIDER_KIND_PLUGIN: &str = "plugin";

fn default_provider_kind() -> String {
    PROVIDER_KIND_OPENAI.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseProvider {
    pub provider_id: i64,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: i64,
    pub enabled: bool,
    pub has_key: bool,
    pub created_at: String,
    /// §10.1：`"openai" | "plugin"`；老客户端不传即 `openai`。`plugin` 时 `base_url` / `model` 为空串、`has_key` 恒 true。
    #[serde(default = "default_provider_kind")]
    pub kind: String,
    /// §10.1：`kind='plugin'` 时是 AstrBot 里的 provider id，否则 null。
    #[serde(default)]
    pub plugin_provider_id: Option<String>,
}

impl ReverseProvider {
    pub fn is_plugin(&self) -> bool {
        self.kind == PROVIDER_KIND_PLUGIN
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseProviderInput {
    #[serde(default)]
    pub provider_id: Option<i64>,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: i64,
    pub enabled: bool,
    /// §10.1：`"openai" | "plugin"`，缺省 `openai`；`plugin` 需要 `plugin_provider_id`，`name` 留空则用它。
    #[serde(default = "default_provider_kind")]
    pub kind: String,
    #[serde(default)]
    pub plugin_provider_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuestionQuery {
    #[serde(default)]
    pub codex_id: Option<String>,
    #[serde(default)]
    pub rating: Option<Vec<Rating>>,
    #[serde(default)]
    pub path_prefix: Option<Vec<String>>,
    #[serde(default)]
    pub decision: Option<Vec<Decision>>,
    #[serde(default)]
    pub main_image_only: bool,
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub question_id: String,
    pub entry_id: String,
    pub ordinal: i64,
    pub title: String,
    pub rating: Rating,
    pub thumb_path: Option<String>,
    pub local_path: Option<String>,
    pub truth_source: String,
    pub truth_preview: String,
    /// v0.2.4 前端追加（docs/CONTRACT_CHANGES.md「前端 v0.2.4」）：题目真值是 §11.1 散文，与批次里 `truth_kind='prose'` 同一判定
    /// （`metrics::truth_kind_of(raw_tag, tags)`）；题库靠它标「散文」/「只看标签题」，前端只在缺键时才从 truth_preview 退化判。
    #[serde(default)]
    pub prose: bool,
}

/// `RunRow.truth_kind` / `reverse_run.truth_kind`（CONTRACT §11.4）：标签题 / 散文题（真值是 §11.1 散文，metric 为空）/ 本地图片题（无真值）。
pub const TRUTH_KIND_TAGS: &str = "tags";
pub const TRUTH_KIND_PROSE: &str = "prose";
pub const TRUTH_KIND_NONE: &str = "none";

fn default_truth_kind() -> String {
    TRUTH_KIND_TAGS.to_string()
}

/// `RunBatchRequest.render_artist_mode`（CONTRACT §12.6 / §12.7）：`pool` = 与线上 `/反推` 同一概率从画风池抽；`given` = 用 `artist`；
/// `none` = 纯标签对照；`style`（§12.7 精准反推）= 桌面端先用画风索引检索最像的一条，翻译成对插件的 `given` + `artist`——
/// 重画请求体 `artist_mode` 只会有前三个值，插件不认识 `style`。
pub const RENDER_ARTIST_MODE_POOL: &str = "pool";
pub const RENDER_ARTIST_MODE_GIVEN: &str = "given";
pub const RENDER_ARTIST_MODE_NONE: &str = "none";
pub const RENDER_ARTIST_MODE_STYLE: &str = "style";
pub const RENDER_ARTIST_MODES: [&str; 4] = [RENDER_ARTIST_MODE_POOL, RENDER_ARTIST_MODE_GIVEN, RENDER_ARTIST_MODE_NONE, RENDER_ARTIST_MODE_STYLE];
/// §12.6：重画张数上限（线上 `reverse_draw.candidates` 的上限）。
pub const RENDER_COUNT_MAX: i64 = 4;

pub fn default_render_artist_mode() -> String {
    RENDER_ARTIST_MODE_POOL.to_string()
}

/// §12.7 `style_fallback`：检索无命中 / 相似度不够时退到 `pool`（与线上同一概率抽画风池）或 `none`（纯标签对照）。
pub const STYLE_FALLBACK_POOL: &str = "pool";
pub const STYLE_FALLBACK_NONE: &str = "none";
pub const STYLE_FALLBACKS: [&str; 2] = [STYLE_FALLBACK_POOL, STYLE_FALLBACK_NONE];
/// §12.7 `style_top_k` 缺省：检索前 5 条，取第一条可用的。
pub const DEFAULT_STYLE_TOP_K: i64 = 5;

pub fn default_style_top_k() -> i64 {
    DEFAULT_STYLE_TOP_K
}

pub fn default_style_fallback() -> String {
    STYLE_FALLBACK_POOL.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBatchRequest {
    /// v0.4.0：精准反推的编码器，null 在批次开始时固定为当前选中的模型。
    #[serde(default)]
    pub style_model: Option<String>,
    pub question_ids: Vec<String>,
    pub template_id: i64,
    pub ruleset_id: i64,
    pub provider_ids: Vec<i64>,
    #[serde(default)]
    pub note: Option<String>,
    /// §11.4：解析成功后重画并评分（消耗点数）；缺省 false。
    #[serde(default)]
    pub render: bool,
    /// §11.4：重画用的角色，null / 空 → 薇拉。
    #[serde(default)]
    pub render_character: Option<String>,
    /// §11.4：固定种子；null → 插件随机并回传。
    #[serde(default)]
    pub render_seed: Option<i64>,
    /// §11.4：成对评委的 provider id；null → 插件默认。
    #[serde(default)]
    pub judge_provider_id: Option<String>,
    /// §12.5：重画时注入的画师串（画风检索命中的 `core` / `pack_tags`）；null / 空白 → 不注入。
    #[serde(default)]
    pub artist: Option<String>,
    /// §12.6：画师来源 `pool | given | none`；缺省 `pool`（与线上 `/反推` 同一概率抽画风池），`given` 用 `artist`，`none` 纯标签对照；
    /// §12.7 再加 `style`（精准反推：先检索画风，再按 `given` + 命中串重画）。
    #[serde(default = "default_render_artist_mode")]
    pub render_artist_mode: String,
    /// §12.6：重画张数；null = 插件配置的默认（线上 `reverse_draw.candidates`），上限 4。
    #[serde(default)]
    pub render_count: Option<i64>,
    /// §12.7：精准反推（`render_artist_mode="style"`）的检索候选数，取第一条可用的；缺省 5。其它模式不看以下五个。
    #[serde(default = "default_style_top_k")]
    pub style_top_k: i64,
    /// §12.7：检索的决策范围 `include | exclude | undecided`，空 = 全部（黑名单命中的检索本来就永不返回）。
    #[serde(default)]
    pub style_decisions: Vec<String>,
    /// §12.7：相似度低于它时按 `style_fallback` 处理；null = 不设门槛。
    #[serde(default)]
    pub style_min_score: Option<f64>,
    /// §12.7：无命中 / 相似度不够时 `pool | none`；缺省 `pool`。
    #[serde(default = "default_style_fallback")]
    pub style_fallback: String,
    /// §12.7：检索时是否减库均值（§12.0「看风格不看内容」）；缺省 true。
    #[serde(default = "default_true")]
    pub style_center: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBatchResult {
    pub batch_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub batch_id: String,
    pub run_id: Option<i64>,
    pub status: String,
    pub done: i64,
    pub total: i64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSummary {
    pub batch_id: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub template: String,
    pub ruleset: String,
    pub providers: Vec<String>,
    pub question_count: i64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub hit: i64,
    pub missed: i64,
    pub extra: i64,
    pub expected_hit: i64,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub f1: Option<f64>,
    pub appearance_leak: i64,
    pub banned_leak: i64,
    pub ruleset_drop: i64,
    pub contradiction: i64,
}

/// §12.6：一张重画候选（插件响应 `images[]` 的一项；落库 `reverse_run.render_candidates` 时多带 `local_path`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderCandidate {
    pub file: String,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub quality_score: Option<f64>,
    #[serde(default)]
    pub quality_notes: Vec<String>,
    /// 拉到本地的绝对路径（`labruns/reverse/<run_id>_<idx>.jpg`）；没取到为 null。
    #[serde(default)]
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRow {
    pub run_id: i64,
    pub question_id: Option<String>,
    pub provider_id: i64,
    pub provider: String,
    pub status: String,
    pub latency_ms: Option<i64>,
    pub error: Option<String>,
    pub cleaned_tags: Vec<String>,
    pub metric: Option<Metric>,
    pub thumb_path: Option<String>,
    /// §11.4：`tags | prose | none`
    #[serde(default = "default_truth_kind")]
    pub truth_kind: String,
    /// §11.4：重画图的本地绝对路径（`labruns/reverse/<run_id>.jpg`），没重画 / 没取到图为 null
    #[serde(default)]
    pub render_path: Option<String>,
    #[serde(default)]
    pub render_seed: Option<i64>,
    #[serde(default)]
    pub render_prompt: Option<String>,
    #[serde(default)]
    pub quality_score: Option<f64>,
    #[serde(default)]
    pub quality_notes: Vec<String>,
    #[serde(default)]
    pub similarity_score: Option<f64>,
    #[serde(default)]
    pub similarity_reason: Option<String>,
    #[serde(default)]
    pub render_error: Option<String>,
    /// §11.4：按 pred_raw 算的泄漏数，任何 truth_kind 都填（tags 题与 metric 里的相同）
    #[serde(default)]
    pub leak_appearance: i64,
    #[serde(default)]
    pub leak_banned: i64,
    /// §12.5：这次重画注入的画师串；没注入 / 没重画为 null
    #[serde(default)]
    pub render_artist: Option<String>,
    /// §12.6：插件回传的画师来源（`pool | given | none`）；老插件 / 没重画为 null
    #[serde(default)]
    pub render_artist_source: Option<String>,
    /// §12.6：重画尺寸（`WxH`）与尺寸来源（按原图宽高比选的，或缺原图宽高时退到 `tags`）
    #[serde(default)]
    pub render_size: Option<String>,
    #[serde(default)]
    pub render_size_source: Option<String>,
    /// §12.6：线上 `_clean_tags` 口径会多剔的 token
    #[serde(default)]
    pub plugin_dropped: Vec<String>,
    /// §12.6：线上口径下清洗后 < 5 条（线上会报错，实验室不报只标）
    #[serde(default)]
    pub plugin_would_reject: bool,
    /// §12.6：全部候选（含最优那张）的 file / seed / 分数 / 本地路径
    #[serde(default)]
    pub render_candidates: Vec<RenderCandidate>,
    /// §12.7：精准反推用到的那条命中（相似度不够被兜底时也记最像的那条，靠 `render_style_fallback` 区分）：条目 id、标题、相似度、决策；
    /// 非 style 模式 / 没重画 / 无命中为 null
    #[serde(default)]
    pub render_style_entry_id: Option<String>,
    #[serde(default)]
    pub render_style_title: Option<String>,
    #[serde(default)]
    pub render_style_score: Option<f64>,
    #[serde(default)]
    pub render_style_decision: Option<String>,
    /// §12.7：true = 这次没用检索结果（无命中 / `score < style_min_score`），按 `style_fallback` 退到了 pool / none
    #[serde(default)]
    pub render_style_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub provider_id: i64,
    pub provider: String,
    pub runs: i64,
    pub ok: i64,
    pub refused: i64,
    pub errors: i64,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub f1: Option<f64>,
    pub appearance_leak: i64,
    pub banned_leak: i64,
    pub contradiction: i64,
    pub avg_latency_ms: Option<f64>,
    /// §11.4：参与 P/R/F1 的题数（truth_kind='tags' 且 metric 非空）
    #[serde(default)]
    pub tags_n: i64,
    /// §11.4：取到重画图的 run 数
    #[serde(default)]
    pub render_n: i64,
    #[serde(default)]
    pub quality_avg: Option<f64>,
    #[serde(default)]
    pub similarity_avg: Option<f64>,
    /// §12.7：用到检索画风的 run 数（`render_style_entry_id` 非空且没兜底）与这些 run 的相似度均值
    #[serde(default)]
    pub style_hit_n: i64,
    #[serde(default)]
    pub style_score_avg: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResults {
    pub batch: BatchSummary,
    pub runs: Vec<RunRow>,
    pub summary: Vec<ProviderSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub score: Option<i64>,
    pub comment: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDetail {
    pub run: RunRow,
    pub raw_response: Option<String>,
    pub parsed: Option<Value>,
    pub pred_raw: Vec<String>,
    pub pred: Vec<String>,
    pub truth: Vec<String>,
    pub truth_eval: Vec<String>,
    pub expected_miss: Vec<String>,
    pub hits: Vec<String>,
    pub misses: Vec<String>,
    pub extras: Vec<String>,
    pub expected_hits: Vec<String>,
    pub contradictions: Vec<String>,
    pub request: Value,
    pub judgment: Option<Judgment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareRow {
    pub question_id: String,
    pub f1_a: Option<f64>,
    pub f1_b: Option<f64>,
    pub delta: Option<f64>,
    pub run_a: Option<i64>,
    pub run_b: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub batch_a: BatchSummary,
    pub batch_b: BatchSummary,
    pub rows: Vec<CompareRow>,
    pub avg_f1_a: Option<f64>,
    pub avg_f1_b: Option<f64>,
}

// ---------- 画风索引与反推画风（CONTRACT §12，v0.3.0）----------

/// §12.2 `style_model_status` 返回：模型名、权重目录（绝对路径）、是否已下载、两个文件的总字节数、
/// 已索引 / 应索引的展示图数（当前模型、ordinal=0、未下架）、均值缓存是否就绪。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleModelStatus {
    pub model: String,
    pub label: String,
    pub description: String,
    pub dim: usize,
    pub download_size_bytes: u64,
    pub default_center: bool,
    pub dir: String,
    pub present: bool,
    pub size_bytes: u64,
    pub indexed: i64,
    pub total: i64,
    pub mean_ready: bool,
    pub device_preference: StyleDevicePreference,
    pub active_device: Option<StyleDevice>,
    pub metal_available: bool,
    pub device_message: Option<String>,
}

/// §12.2 `style_model_download` 的进度事件：`file` 是 `config.json` / `pytorch_model.bin`（兼容已有 `model.safetensors`）；`total_bytes` 未知（上游没给长度）为 0；
/// `finished=true` 表示该文件已改名落地；`error` 非空表示该文件失败（此后不再有该文件的事件）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadEvent {
    pub file: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub finished: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// §12.2 `StyleSearchRequest.source`：题库里的一道题（条目 × ordinal 的本地展示图）或任意本地图片路径。
/// JSON 是 `kind` 标记的联合：`{ "kind": "entry", "entry_id", "ordinal" }` / `{ "kind": "path", "path" }`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StyleSearchSource {
    Entry { entry_id: String, ordinal: i64 },
    Path { path: String },
}

fn default_top_k() -> i64 {
    10
}

pub fn default_true() -> bool {
    true
}

/// §12.2 `style_search` 参数：`top_k` 1..50 缺省 10（超出钳到边界）；`codex_ids` 空 = 两本；`decisions` 空 = 全部；
/// `center` 缺省 true（查询与库向量都减库均值再算余弦）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleSearchRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub source: StyleSearchSource,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default)]
    pub codex_ids: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default = "default_true")]
    pub center: bool,
}

/// §12.2 一条命中：黑名单命中的永远不返回，所以 `blacklisted` 恒 false；`score` 是余弦（-1..1）；
/// 画师条目给 `core`（§3.1 第 5 步的串，不带质量后缀），图包条目给 `pack_tags`（导出用的 `pack.tags`：散文原文或清洗后标签）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleHit {
    pub style_prompt: String,
    pub entry_id: String,
    pub codex_id: String,
    pub title: String,
    pub path: Vec<String>,
    pub decision: Decision,
    pub blacklisted: bool,
    pub score: f64,
    pub core: Option<String>,
    pub pack_tags: Option<String>,
    pub thumb_path: Option<String>,
    pub prose: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleSearchResult {
    pub model: String,
    pub indexed: i64,
    pub query_ms: i64,
    pub hits: Vec<StyleHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEvent {
    pub id: i64,
    pub at: String,
    pub level: String,
    pub area: String,
    pub message: String,
    pub data: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// §12.6：`RunBatchRequest.render_artist_mode`（缺省 pool）/ `render_count`、`RenderCandidate`、`RunRow` 六个新键的 serde 往返；
    /// 值不在这里归一（空串原样收下，归一在 `render::RenderPlan::from_request`），非法值也不在这里拒（校验在 `render::validate_render_params`）。
    #[test]
    fn v030_render_alignment_dtos_round_trip() {
        let base = json!({ "question_ids": ["a#0"], "template_id": 1, "ruleset_id": 1, "provider_ids": [1] });
        let mut full = base.clone();
        full["render"] = json!(true);
        full["render_artist_mode"] = json!("given");
        full["artist"] = json!("artist:foo");
        full["render_count"] = json!(3);
        let req: RunBatchRequest = serde_json::from_value(full).unwrap();
        assert_eq!((req.render_artist_mode.as_str(), req.render_count, req.artist.as_deref()), ("given", Some(3), Some("artist:foo")));
        let back: RunBatchRequest = serde_json::from_value(serde_json::to_value(&req).unwrap()).unwrap();
        assert_eq!((back.render_artist_mode.as_str(), back.render_count), ("given", Some(3)));
        let mut empty = base.clone();
        empty["render_artist_mode"] = json!("");
        empty["render_count"] = json!(null);
        let req: RunBatchRequest = serde_json::from_value(empty).unwrap();
        assert_eq!((req.render_artist_mode.as_str(), req.render_count), ("", None), "空串 / null 原样收下");
        assert_eq!(RENDER_ARTIST_MODES, ["pool", "given", "none", "style"], "§12.7 第四个取值");
        assert_eq!((default_render_artist_mode().as_str(), RENDER_COUNT_MAX), ("pool", 4));

        // RenderCandidate：只有 file 必需，其余缺键 → None / 空；序列化五键都在
        let c: RenderCandidate = serde_json::from_value(json!({ "file": "x.png" })).unwrap();
        assert_eq!(c, RenderCandidate { file: "x.png".into(), seed: None, quality_score: None, quality_notes: vec![], local_path: None });
        let v = serde_json::to_value(&c).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["file", "local_path", "quality_notes", "quality_score", "seed"]);
        assert!(serde_json::from_value::<RenderCandidate>(json!({ "seed": 1 })).is_err(), "没有 file 反序列化失败");
        let full_c: RenderCandidate = serde_json::from_value(json!({ "file": "y.png", "seed": 9, "quality_score": 6.5, "quality_notes": ["a", "b"], "local_path": "/d/1_1.jpg" })).unwrap();
        assert_eq!(serde_json::from_value::<RenderCandidate>(serde_json::to_value(&full_c).unwrap()).unwrap(), full_c);

        // RunRow：六键缺省（老 JSON）与带值往返
        let row: RunRow = serde_json::from_value(json!({
            "run_id": 3, "question_id": "e#0", "provider_id": 1, "provider": "p", "status": "ok", "latency_ms": 1, "error": null, "cleaned_tags": ["a"], "metric": null,
            "thumb_path": null, "render_artist_source": "pool", "render_size": "1216x832", "render_size_source": "tags", "plugin_dropped": ["long hair", "realistic"],
            "plugin_would_reject": true, "render_candidates": [{ "file": "a.png", "seed": 1 }, { "file": "b.png", "seed": 2, "quality_score": 8.0, "local_path": "/d/3_1.jpg" }]
        }))
        .unwrap();
        assert_eq!((row.render_artist_source.as_deref(), row.render_size.as_deref(), row.render_size_source.as_deref()), (Some("pool"), Some("1216x832"), Some("tags")));
        assert_eq!(row.plugin_dropped, ["long hair", "realistic"]);
        assert!(row.plugin_would_reject);
        assert_eq!(row.render_candidates.len(), 2);
        assert_eq!((row.render_candidates[0].local_path.as_deref(), row.render_candidates[1].local_path.as_deref(), row.render_candidates[1].quality_score), (None, Some("/d/3_1.jpg"), Some(8.0)));
        let back: RunRow = serde_json::from_value(serde_json::to_value(&row).unwrap()).unwrap();
        assert_eq!((back.render_candidates, back.plugin_dropped, back.plugin_would_reject), (row.render_candidates.clone(), row.plugin_dropped.clone(), true));
    }

    /// §11.4：`RunBatchRequest` 四个新键都有缺省（老前端不传照收）；`RunRow` / `ProviderSummary` 序列化带齐新键，老形状 JSON 也能读回。
    #[test]
    fn v024_dtos_round_trip_with_defaults() {
        let old: RunBatchRequest = serde_json::from_value(json!({ "question_ids": ["a#0"], "template_id": 1, "ruleset_id": 1, "provider_ids": [1] })).unwrap();
        assert!(!old.render);
        assert_eq!((old.render_character, old.render_seed, old.judge_provider_id, old.note), (None, None, None, None));
        assert_eq!(old.artist, None, "§12.5 artist 缺键 → null");
        assert_eq!((old.render_artist_mode.as_str(), old.render_count), ("pool", None), "§12.6 缺键 → pool / null");
        let full: RunBatchRequest = serde_json::from_value(json!({
            "question_ids": ["a#0"], "template_id": 1, "ruleset_id": 1, "provider_ids": [1], "note": "n",
            "render": true, "render_character": "薇拉", "render_seed": 42, "judge_provider_id": "gpt-4o"
        }))
        .unwrap();
        assert!(full.render);
        assert_eq!((full.render_character.as_deref(), full.render_seed, full.judge_provider_id.as_deref()), (Some("薇拉"), Some(42), Some("gpt-4o")));
        let v = serde_json::to_value(&full).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 18, "RunBatchRequest 9 键 + §12.5 artist + §12.6 render_artist_mode / render_count + §12.7 style_* 五键");
        assert_eq!((v["render_artist_mode"].as_str(), v["render_count"].is_null()), (Some("pool"), true), "缺省也序列化出来");
        let back: RunBatchRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back.render_seed, Some(42));

        // §11.3 `LabPlan.prose` / 前端追加的 `Question.prose`：缺键 → false，插件 / 题库给 true 能收下，序列化后键仍在
        let plan_json = json!({
            "idx": 0, "character": "薇拉", "tags_raw": "a", "tags_used": "a", "prompt": "p", "negative": "n", "artist": "",
            "artist_source": "none", "size": "832x1216", "model": "m"
        });
        let plan: LabPlan = serde_json::from_value(plan_json.clone()).unwrap();
        assert!(!plan.prose, "老记录 / 非散文缺键 → false");
        let mut with_prose = plan_json.clone();
        with_prose["prose"] = json!(true);
        let plan: LabPlan = serde_json::from_value(with_prose).unwrap();
        assert!(plan.prose);
        assert_eq!(serde_json::to_value(&plan).unwrap()["prose"], true);
        let image: LabImage = serde_json::from_value(json!({
            "idx": 0, "character": "薇拉", "tags_raw": "a", "tags_used": "a", "prompt": "p", "negative": "n", "artist": "",
            "artist_source": "none", "size": "832x1216", "model": "m", "prose": true, "status": "done"
        }))
        .unwrap();
        assert!(image.plan.prose, "LabImage 扁平铺开时 prose 也随 plan 收下");
        let q: Question = serde_json::from_value(json!({
            "question_id": "e#0", "entry_id": "e", "ordinal": 0, "title": "t", "rating": "safe", "thumb_path": null, "local_path": null,
            "truth_source": "tags", "truth_preview": "a, b"
        }))
        .unwrap();
        assert!(!q.prose);

        let row = RunRow {
            run_id: 1,
            question_id: Some("e#0".into()),
            provider_id: 1,
            provider: "p".into(),
            status: "ok".into(),
            latency_ms: Some(10),
            error: None,
            cleaned_tags: vec!["smile".into()],
            metric: None,
            thumb_path: None,
            truth_kind: TRUTH_KIND_PROSE.into(),
            render_path: Some("/d/labruns/reverse/1.jpg".into()),
            render_seed: Some(7),
            render_prompt: Some("薇拉, 1girl, smile".into()),
            quality_score: Some(7.5),
            quality_notes: vec!["手部正常".into()],
            similarity_score: Some(8.0),
            similarity_reason: Some("构图一致".into()),
            render_error: None,
            leak_appearance: 1,
            leak_banned: 0,
            render_artist: Some("artist:foo".into()),
            render_artist_source: Some("given".into()),
            render_size: Some("832x1216".into()),
            render_size_source: Some("ratio".into()),
            plugin_dropped: vec!["realistic".into()],
            plugin_would_reject: true,
            render_candidates: vec![RenderCandidate { file: "a.png".into(), seed: Some(7), quality_score: Some(7.5), quality_notes: vec![], local_path: Some("/d/labruns/reverse/1_0.jpg".into()) }],
            render_style_entry_id: Some("e9".into()),
            render_style_title: Some("T".into()),
            render_style_score: Some(0.83),
            render_style_decision: Some("include".into()),
            render_style_fallback: false,
        };
        let v = serde_json::to_value(&row).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "cleaned_tags", "error", "latency_ms", "leak_appearance", "leak_banned", "metric", "plugin_dropped", "plugin_would_reject", "provider", "provider_id",
                "quality_notes", "quality_score", "question_id", "render_artist", "render_artist_source", "render_candidates", "render_error", "render_path", "render_prompt",
                "render_seed", "render_size", "render_size_source", "render_style_decision", "render_style_entry_id", "render_style_fallback", "render_style_score",
                "render_style_title", "run_id", "similarity_reason", "similarity_score", "status", "thumb_path", "truth_kind"
            ],
            "RunRow 22 键 + §12.6 六键 + §12.7 五键"
        );
        assert_eq!(v["render_artist"], "artist:foo");
        assert_eq!(v["render_candidates"], json!([{ "file": "a.png", "seed": 7, "quality_score": 7.5, "quality_notes": [], "local_path": "/d/labruns/reverse/1_0.jpg" }]));
        let back: RunRow = serde_json::from_value(v).unwrap();
        assert_eq!((back.truth_kind.as_str(), back.leak_appearance, back.quality_notes.len(), back.similarity_score), ("prose", 1, 1, Some(8.0)));
        // 老形状的 RunRow JSON（没有新键）也能反序列化
        let legacy: RunRow = serde_json::from_value(json!({
            "run_id": 2, "question_id": null, "provider_id": 1, "provider": "p", "status": "error", "latency_ms": null, "error": "x",
            "cleaned_tags": [], "metric": null, "thumb_path": null
        }))
        .unwrap();
        assert_eq!((legacy.truth_kind.as_str(), legacy.render_path, legacy.leak_banned, legacy.quality_notes.len()), ("tags", None, 0, 0));
        assert_eq!(legacy.render_artist, None);
        assert_eq!((legacy.render_artist_source, legacy.render_size, legacy.render_size_source), (None, None, None), "§12.6 老形状缺键 → null");
        assert!(legacy.plugin_dropped.is_empty() && !legacy.plugin_would_reject && legacy.render_candidates.is_empty());
        assert!(legacy.render_style_entry_id.is_none() && legacy.render_style_score.is_none() && !legacy.render_style_fallback, "§12.7 老形状缺键 → null / false");

        let s = ProviderSummary {
            provider_id: 1,
            provider: "p".into(),
            runs: 3,
            ok: 2,
            refused: 0,
            errors: 1,
            precision: Some(0.5),
            recall: None,
            f1: None,
            appearance_leak: 1,
            banned_leak: 0,
            contradiction: 0,
            avg_latency_ms: Some(100.0),
            tags_n: 1,
            render_n: 2,
            quality_avg: Some(7.0),
            similarity_avg: None,
            style_hit_n: 0,
            style_score_avg: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 19, "ProviderSummary 13 + 4 键 + §12.7 两键");
        assert_eq!(v["tags_n"], 1);
        assert_eq!(v["render_n"], 2);
        assert_eq!(v["quality_avg"], 7.0);
        assert!(v["similarity_avg"].is_null());
        let back: ProviderSummary = serde_json::from_value(v).unwrap();
        assert_eq!((back.tags_n, back.render_n, back.quality_avg), (1, 2, Some(7.0)));
    }

    /// §12.7：`RunBatchRequest` 五个 `style_*` 键缺省（5 / [] / null / pool / true）与带值往返，值不在这里归一或校验（归一在 `render::StyleParams`，
    /// 校验在 `render::validate_style_params`）；`RunRow` 五键缺省与往返；`ProviderSummary` 两键缺省与往返。
    #[test]
    fn v031_style_dtos_round_trip() {
        let old: RunBatchRequest = serde_json::from_value(json!({ "question_ids": ["a#0"], "template_id": 1, "ruleset_id": 1, "provider_ids": [1] })).unwrap();
        assert_eq!((old.style_top_k, old.style_decisions.len(), old.style_min_score, old.style_fallback.as_str(), old.style_center), (5, 0, None, "pool", true), "缺键 → 缺省");
        assert_eq!((DEFAULT_STYLE_TOP_K, default_style_top_k(), default_style_fallback().as_str()), (5, 5, "pool"));
        assert_eq!(STYLE_FALLBACKS, ["pool", "none"]);
        let v = serde_json::to_value(&old).unwrap();
        assert_eq!((v["style_top_k"].as_i64(), v["style_decisions"].as_array().map(Vec::len), v["style_min_score"].is_null(), v["style_fallback"].as_str(), v["style_center"].as_bool()), (Some(5), Some(0), true, Some("pool"), Some(true)), "缺省也序列化出来");
        let full: RunBatchRequest = serde_json::from_value(json!({
            "question_ids": ["a#0"], "template_id": 1, "ruleset_id": 1, "provider_ids": [1], "render": true, "render_artist_mode": "style",
            "style_top_k": 8, "style_decisions": ["include", "Undecided"], "style_min_score": 0.35, "style_fallback": "none", "style_center": false
        }))
        .unwrap();
        assert_eq!((full.render_artist_mode.as_str(), full.style_top_k, full.style_min_score, full.style_fallback.as_str(), full.style_center), ("style", 8, Some(0.35), "none", false));
        assert_eq!(full.style_decisions, ["include", "Undecided"], "原样收下（大小写归一在 render::StyleParams）");
        let back: RunBatchRequest = serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
        assert_eq!((back.style_top_k, back.style_decisions.clone(), back.style_min_score, back.style_fallback.clone(), back.style_center), (8, full.style_decisions.clone(), Some(0.35), "none".to_string(), false));
        let odd: RunBatchRequest = serde_json::from_value(json!({ "question_ids": [], "template_id": 1, "ruleset_id": 1, "provider_ids": [], "style_top_k": 0, "style_fallback": "", "style_decisions": ["maybe"] })).unwrap();
        assert_eq!((odd.style_top_k, odd.style_fallback.as_str(), odd.style_decisions.as_slice()), (0, "", &["maybe".to_string()][..]), "非法值原样收下，校验在 validate_style_params");

        // RunRow：五键缺省（老 JSON）与带值往返
        let row: RunRow = serde_json::from_value(json!({
            "run_id": 3, "question_id": "e#0", "provider_id": 1, "provider": "p", "status": "ok", "latency_ms": 1, "error": null, "cleaned_tags": ["a"], "metric": null,
            "thumb_path": null, "render_artist_source": "style", "render_artist": "artist:foo",
            "render_style_entry_id": "a_foo", "render_style_title": "foo", "render_style_score": 0.91, "render_style_decision": "include", "render_style_fallback": false
        }))
        .unwrap();
        assert_eq!((row.render_style_entry_id.as_deref(), row.render_style_title.as_deref(), row.render_style_score, row.render_style_decision.as_deref(), row.render_style_fallback), (Some("a_foo"), Some("foo"), Some(0.91), Some("include"), false));
        assert_eq!(row.render_artist_source.as_deref(), Some("style"), "§12.7 render_artist_source 新取值");
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 33, "RunRow 28 键 + §12.7 五键");
        let back: RunRow = serde_json::from_value(v).unwrap();
        assert_eq!((back.render_style_entry_id, back.render_style_score, back.render_style_fallback), (row.render_style_entry_id.clone(), Some(0.91), false));
        let fb: RunRow = serde_json::from_value(json!({
            "run_id": 4, "question_id": null, "provider_id": 1, "provider": "p", "status": "ok", "latency_ms": null, "error": null, "cleaned_tags": [], "metric": null, "thumb_path": null,
            "render_style_fallback": true
        }))
        .unwrap();
        assert!(fb.render_style_fallback && fb.render_style_entry_id.is_none(), "兜底且无命中：四个字段空、fallback=true");

        // ProviderSummary：两键缺省（老 JSON）与带值往返
        let old_s: ProviderSummary = serde_json::from_value(json!({
            "provider_id": 1, "provider": "p", "runs": 1, "ok": 1, "refused": 0, "errors": 0, "precision": null, "recall": null, "f1": null,
            "appearance_leak": 0, "banned_leak": 0, "contradiction": 0, "avg_latency_ms": null
        }))
        .unwrap();
        assert_eq!((old_s.style_hit_n, old_s.style_score_avg), (0, None));
        let s = ProviderSummary { style_hit_n: 2, style_score_avg: Some(0.75), ..old_s };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!((v["style_hit_n"].as_i64(), v["style_score_avg"].as_f64()), (Some(2), Some(0.75)));
        let back: ProviderSummary = serde_json::from_value(v).unwrap();
        assert_eq!((back.style_hit_n, back.style_score_avg), (2, Some(0.75)));
    }

    /// §12：`SyncEvent` 三个 embed 键缺省 0 且序列化带齐、`SyncPhase::Embedding` 是 "embedding"；`StyleSearchRequest.source` 两种 kind 都能来回；
    /// `top_k` / `codex_ids` / `decisions` / `center` 缺键按 10 / [] / [] / true；`StyleHit` / `StyleSearchResult` / `StyleModelStatus` / `DownloadEvent` 键名照契约。
    #[test]
    fn v030_style_dtos_round_trip() {
        let old: SyncEvent = serde_json::from_value(json!({
            "phase": "images", "message": "m", "release": null, "added": 0, "changed": 0, "images_total": 1, "images_done": 1, "images_failed": 0
        }))
        .unwrap();
        assert_eq!((old.embed_total, old.embed_done, old.embed_failed), (0, 0, 0));
        let ev = SyncEvent { phase: SyncPhase::Embedding, embed_total: 5, embed_done: 2, embed_failed: 1, ..old.clone() };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["phase"], "embedding");
        assert_eq!(v.as_object().unwrap().len(), 11, "SyncEvent 8 键 + 3 个 embed 键");
        assert_eq!((v["embed_total"].as_u64(), v["embed_done"].as_u64(), v["embed_failed"].as_u64()), (Some(5), Some(2), Some(1)));
        assert_eq!(serde_json::from_str::<SyncPhase>("\"embedding\"").unwrap(), SyncPhase::Embedding);

        let entry: StyleSearchRequest = serde_json::from_value(json!({ "source": { "kind": "entry", "entry_id": "e1", "ordinal": 0 } })).unwrap();
        assert_eq!(entry.source, StyleSearchSource::Entry { entry_id: "e1".into(), ordinal: 0 });
        assert_eq!((entry.top_k, entry.codex_ids.len(), entry.decisions.len(), entry.center), (10, 0, 0, true), "缺省值");
        let path: StyleSearchRequest = serde_json::from_value(json!({
            "source": { "kind": "path", "path": "/tmp/q.png" }, "top_k": 3, "codex_ids": ["artist_nai5_personal"], "decisions": ["include", "undecided"], "center": false
        }))
        .unwrap();
        assert_eq!(path.source, StyleSearchSource::Path { path: "/tmp/q.png".into() });
        assert_eq!((path.top_k, path.codex_ids.as_slice(), path.center), (3, &["artist_nai5_personal".to_string()][..], false));
        assert_eq!(path.decisions, [Decision::Include, Decision::Undecided]);
        let v = serde_json::to_value(&path).unwrap();
        assert_eq!(v["source"], json!({ "kind": "path", "path": "/tmp/q.png" }), "联合体序列化回 kind 标记");
        assert_eq!(serde_json::to_value(&entry).unwrap()["source"], json!({ "kind": "entry", "entry_id": "e1", "ordinal": 0 }));
        let back: StyleSearchRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back, path);
        assert!(serde_json::from_value::<StyleSearchRequest>(json!({ "source": { "kind": "url", "url": "x" } })).is_err(), "未知 kind 报错");
        assert!(serde_json::from_value::<StyleSearchRequest>(json!({ "source": { "kind": "entry", "entry_id": "e1" } })).is_err(), "entry 缺 ordinal 报错");

        let hit = StyleHit {
            style_prompt: "artist:a, thick outlines".into(),
            entry_id: "e1".into(),
            codex_id: "artist_nai5_personal".into(),
            title: "t".into(),
            path: vec!["单画师词典".into()],
            decision: Decision::Exclude,
            blacklisted: false,
            score: 0.87,
            core: Some("artist:foo".into()),
            pack_tags: None,
            thumb_path: Some("/d/thumbs/x.jpg".into()),
            prose: false,
        };
        let result = StyleSearchResult { model: "clip-vit-base-patch32".into(), indexed: 3800, query_ms: 42, hits: vec![hit] };
        let v = serde_json::to_value(&result).unwrap();
        let mut keys: Vec<&str> = v["hits"][0].as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["blacklisted", "codex_id", "core", "decision", "entry_id", "pack_tags", "path", "prose", "score", "style_prompt", "thumb_path", "title"]);
        assert_eq!(v["hits"][0]["decision"], "exclude");
        assert_eq!(v.as_object().unwrap().len(), 4);
        let back: StyleSearchResult = serde_json::from_value(v).unwrap();
        assert_eq!(back, result);

        let st = StyleModelStatus { label: "CLIP".into(), description: "baseline".into(), dim: 512, download_size_bytes: 605247071, default_center: true, model: "clip-vit-base-patch32".into(), dir: "/d/models/clip-vit-base-patch32".into(), present: false, size_bytes: 0, indexed: 0, total: 12, mean_ready: false, device_preference: StyleDevicePreference::Auto, active_device: None, metal_available: false, device_message: None };
        let v = serde_json::to_value(&st).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["active_device", "default_center", "description", "device_message", "device_preference", "dim", "dir", "download_size_bytes", "indexed", "label", "mean_ready", "metal_available", "model", "present", "size_bytes", "total"]);
        let ev = DownloadEvent { file: "model.safetensors".into(), done_bytes: 10, total_bytes: 100, finished: false, error: None };
        let v = serde_json::to_value(&ev).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["done_bytes", "error", "file", "finished", "total_bytes"]);
        assert!(v["error"].is_null());
        let back: DownloadEvent = serde_json::from_value(json!({ "file": "config.json", "done_bytes": 1, "total_bytes": 1, "finished": true })).unwrap();
        assert_eq!(back.error, None, "error 缺键 → null");
    }
}
