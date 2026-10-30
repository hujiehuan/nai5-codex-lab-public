// 前后端 DTO（权威形状，Rust models.rs 必须逐字段一致）。字段全部 snake_case。

export type Decision = "include" | "exclude" | "undecided";
export type DecisionVia = "click" | "key" | "batch" | "import" | "undo" | "rule";
/** 只收 `safe` 分级的条目：其它分级的上游条目在同步解析时丢弃。 */
export type Rating = "safe";
export type Library = "artist" | "pack";
/** v0.3.0（§12.2）：images 之后多一个 embedding（画风索引）阶段；模型未下载时该阶段只发一条"跳过"消息，不算错误。 */
export type SyncPhase = "idle" | "manifest" | "entries" | "images" | "embedding" | "done" | "error";

export interface AppError { code: string; message: string }

export interface SourceImageResult {
  path: string | null;
  kind: "original" | "display" | "thumbnail" | "missing";
}

export interface AppInfo {
  version: string;
  app_data_dir: string;
  db_path: string;
  latest_release: string | null;
}

export type AppUpdatePhase = "idle" | "checking" | "up_to_date" | "available" | "preparing" | "ready" | "installing" | "error" | "cancelled";
export interface AppUpdateStatus {
  supported: boolean;
  phase: AppUpdatePhase;
  current_version: string;
  current_revision: string | null;
  source_dir: string | null;
  candidate: { version: string; revision: string; tag: string } | null;
  stage: string | null;
  message: string | null;
  log_tail: string[];
  started_at: string | null;
  can_install: boolean;
}

export type StyleDevicePreference = "auto" | "metal" | "cpu";

export interface Settings {
  plugin_base_url: string;        // 如 http://127.0.0.1:6185，不带尾斜杠
  webui_username: string;
  download_concurrency: number;   // 1–4
  style_device: StyleDevicePreference; // auto 优先 Metal，不可用时回退 CPU
}
export type SettingsPatch = Partial<Settings>;

export interface SyncStatus {
  running: boolean;
  phase: SyncPhase;
  release_local: string | null;
  release_remote: string | null;
  images_total: number;
  images_done: number;
  images_failed: number;
  message: string;
}
export interface SyncEvent {
  phase: SyncPhase;
  message: string;
  release: string | null;
  added: number;
  changed: number;
  images_total: number;
  images_done: number;
  images_failed: number;
  // ---- v0.3.0（§12.2）embedding 阶段的进度；其它阶段 / 老后端缺键 ----
  embed_total?: number;
  embed_done?: number;
  embed_failed?: number;
}
export interface SyncFailure { url: string; error: string; code?: string }
export interface SyncLog {
  id: number;
  started_at: string;
  finished_at: string | null;
  kind: "full" | "resume" | "noop" | "partial" | "failed";
  release_id: string | null;
  added: number;
  changed: number;
  images_ok: number;
  images_failed: number;
  failures: SyncFailure[];
}

export interface TreeNode { name: string; count: number; children: TreeNode[] }
export interface UpdateFilter { id: string; label: string; latest?: boolean }
export interface DecisionCounts { include: number; exclude: number; undecided: number }
export interface Book {
  codex_id: string;
  type: "string" | "pack" | "local";
  title: string;
  version: string;
  author: string | null;
  entry_count: number;
  tree: TreeNode[];
  update_filters: UpdateFilter[];
  counts: DecisionCounts & { total: number };
}

export interface EntryQuery {
  codex_id: string;
  rating?: Rating[];
  decision?: Decision[];
  batch?: string;                 // update_batches 包含该值
  first_seen_release?: string;
  path_prefix?: string[];         // 目录树前缀，如 ["单画师词典","作者乙"]
  keyword?: string;               // 匹配 title 与 tags，大小写不敏感
  multi_image_only?: boolean;
  dup_only?: boolean;             // 同 signature ≥ 2 条
  sort?: EntrySort;               // 默认 book_order
  sort_dir?: "asc" | "desc";      // 缺省按 sort 的自然方向：book_order/title/synced_at 升序，decided_at/first_seen 降序
  offset: number;
  limit: number;
}
/** 排序键：synced_at = 主图（ordinal 0）本地落地时间，未下载排最后；first_seen = 首次出现的 release 发布时间。 */
export type EntrySort = "book_order" | "title" | "decided_at" | "synced_at" | "first_seen";
export interface EntryCard {
  entry_id: string;
  codex_id: string;
  book_order: number;
  title: string;
  tags_first_line: string;        // tags 首行截 120 字
  rating: Rating;
  batches: string[];
  path: string[];
  decision: Decision;
  thumb_path: string | null;
  image_path: string | null;
  image_w: number | null;
  image_h: number | null;
  image_count: number;
  dup_count: number;              // 同 signature 的条目数（含自己），无 parse 时 1
  is_new: boolean;
}
export interface EntryPage { total: number; counts: DecisionCounts; items: EntryCard[] }

export interface CharacterPrompt { label: string; prompt: string; negative?: string }
export interface Entry {
  entry_id: string;
  codex_id: string;
  book_order: number;
  entry_version: number;
  title: string;
  path: string[];
  tags: string;
  negative: string | null;
  character_prompts: CharacterPrompt[] | null;
  note: string | null;
  rating: Rating;
  is_new: boolean;
  update_batches: string[];
  asset_rev: string | null;
  image_w: number | null;
  image_h: number | null;
  first_seen_release: string;
  last_seen_release: string;
  removed_at: string | null;
}
export interface EntryImage {
  ordinal: number;
  image_file: string;
  original_file: string | null;
  raw_tag: string | null;
  local_path: string | null;
  thumb_path: string | null;
  original_local_path: string | null;
  bytes: number | null;
  error: string | null;
}
export type TokenKind = "artist" | "year" | "quality" | "negative" | "style" | "other";
export interface RecipeToken { value: string; weight: number; kind: TokenKind }
export interface ArtistWeighted { name: string; weight: number }
export interface ArtistParse {
  artists: ArtistWeighted[];
  auxiliary: RecipeToken[];
  negative_tail: RecipeToken[];
  signature: string;
  warnings: string[];
}
export interface AuditRow { id: number; from_decision: Decision | null; to_decision: Decision; at: string; via: DecisionVia }
export interface Sibling { entry_id: string; codex_id: string; title: string; decision: Decision }
export interface EntryDetail {
  entry: Entry;
  images: EntryImage[];
  membership: { decision: Decision; member_note: string | null; decided_at: string } | null;
  audit: AuditRow[];
  parse: ArtistParse | null;
  siblings: Sibling[];
}
export interface UndoResult { entry_id: string; restored: Decision }

export interface BlacklistItem { name: string; reason: string | null; added_at: string }

export interface ExportOptions { out_dir?: string }
export interface ExportFile { name: string; count: number; path: string }
export interface ExportResult { dir: string; files: ExportFile[]; skipped: number }
export interface PushResult {
  ok: boolean;
  nai5: number;
  pack: number;
  dropped_blacklist: number;
  bak: string[];
  message: string;
  plugin_response: unknown;
}

// ---- v0.1.4 完整池与插件状态（CONTRACT §8.2）----
/** 完整池按 codex 的计数；与 full_pool.json 的 counts[codex_id] 同形。 */
export interface FullCounts { total: number; include: number; exclude: number; undecided: number; blacklisted: number }
export interface PushFullResult {
  ok: boolean;
  message: string;
  total: number;
  counts: Record<string, FullCounts>;   // codex_id → 计数
  bak: string[];
  plugin_response: unknown;
}
/** 插件端一种池的持有状态。filtered.counts 键为 nai5 / pack / kept_old / dropped_blacklist；full.counts 键为 codex_id → total。 */
export interface PoolStatus {
  imported_at: string;
  release_id: string | null;
  exported_at: string | null;
  app_version: string | null;
  counts: Record<string, number>;
  file: string;
  size_bytes: number;
  backups: string[];
}
export interface PluginLibraryStatus {
  reachable: boolean;               // false = 连不上 / 超时 / 未配置密码或 token，message 给原因；不是错误
  message: string;
  plugin_version: string | null;
  filtered: PoolStatus | null;
  full: PoolStatus | null;
}

// ---- v0.2.0 画图实验室（CONTRACT §9.1 / §9.3 / §9.4）----
// 两端共用：桌面端把 LabRunRequest 原样交给后端转发插件，插件返回的 LabPlan / LabRun / LabRunSummary 原样透传。
export type LabMode = "draw";
export type LabRunStatus = "queued" | "running" | "done" | "error";
export type LabImageStatus = "pending" | "done" | "error";
/** 画师串来源：style = 风格=；pool = 画风池掷中；none = 无。 */
export type LabArtistSource = "style" | "pool" | "none";
/** 单张图的人工评价：收藏 / 点踩；未评价为 null。 */
export type LabRating = "fav" | "dis";

export interface LabRunRequest {
  mode: LabMode;
  text: string;                   // 用户敲在 /画 后面的那串：首 token 角色（或 随机），其余为自然语言描述；可含 风格=xxx
  entry_id: string | null;        // 选中的图包（完整池里 nai5_community_pack 的 entry_id）；null = 随机
  scope: string[][];              // 随机时的候选范围：path 前缀列表（与 EntryQuery.path_prefix 同语义）；空数组 = 该模式的默认范围
  artist_ratio: number;           // 0..1，命中则注入画风池串
  count: number;                  // 1..8
  score: boolean;                 // 是否跑三评委
  seed: number | null;            // 固定种子（每张 +i）；null = 随机
}
export interface LabPlan {
  idx: number;
  character: string;
  pack_entry_id: string | null;
  pack_title: string | null;
  pack_path: string[] | null;
  tags_raw: string;
  tags_used: string;
  dropped: string[];
  extra_tags: string | null;
  prompt: string;
  negative: string;
  artist: string;
  artist_source: LabArtistSource;
  size: string;
  seed: number | null;
  model: string;
  /** v0.2.4（§11.3）：点名的图包是散文条目——tags_used 是原文、dropped 为空；老记录 / 非散文缺键或 false。 */
  prose?: boolean;
}
/** LabPlan & {...}：JSON 是扁平的（插件 / Rust 侧 flatten），没有 plan 子对象。 */
export interface LabImage extends LabPlan {
  status: LabImageStatus;
  file: string | null;
  error: string | null;
  score: number | null;
  notes: string[];
  rating: LabRating | null;
}
export interface LabRun {
  run_id: string;
  mode: LabMode;
  request: LabRunRequest;
  status: LabRunStatus;
  created_at: string;
  finished_at: string | null;
  stamina_before: number | null;
  stamina_after: number | null;
  images: LabImage[];
  error: string | null;
  plugin_version: string;
}
export interface LabRunSummary {
  run_id: string;
  mode: LabMode;
  status: LabRunStatus;
  created_at: string;
  count: number;
  done: number;
  text: string;
  pack_title: string | null;
  best_score: number | null;
}
/** lab_run_start 返回（§9.3 POST /page/lab/run/start → { run_id, plans }）。 */
export interface LabRunStart { run_id: string; plans: LabPlan[] }

// ---- 反推实验室 ----
export interface ReverseTemplate {
  template_id: number; name: string; version: number; body: string;
  parent_id: number | null; created_at: string; note: string | null;
}
export interface ReverseTemplateInput { name: string; body: string; parent_id?: number | null; note?: string }
export interface ReverseRuleset {
  ruleset_id: number; name: string; version: number;
  banned: string[]; appearance_regex: string; accessory_ok_regex: string; refusal_regex: string;
  always_drop: string[]; max_words: number; max_len: number; created_at: string; note: string | null;
}
export interface ReverseRulesetInput {
  name: string; banned: string[]; appearance_regex: string; accessory_ok_regex: string; refusal_regex: string;
  always_drop: string[]; max_words: number; max_len: number; note?: string;
}
/** v0.2.3（§10.1）模型来源：openai = 本地 OpenAI 兼容端点；plugin = AstrBot 插件里配置好的 provider（不需要 base_url / key）。 */
export type ReverseProviderKind = "openai" | "plugin";
export interface ReverseProvider {
  provider_id: number; name: string; base_url: string; model: string; max_tokens: number; enabled: boolean;
  has_key: boolean; created_at: string;
  kind: ReverseProviderKind;            // §10.1：plugin 时 base_url / model 为空串、has_key 恒 true；老记录缺省 openai
  plugin_provider_id: string | null;    // §10.1：kind='plugin' 时是 AstrBot 里的 provider id，否则 null
}
export interface ReverseProviderInput {
  provider_id?: number; name: string; base_url: string; model: string; max_tokens: number; enabled: boolean;
  kind: ReverseProviderKind; plugin_provider_id: string | null;
}

export interface QuestionQuery { codex_id?: string; rating?: Rating[]; path_prefix?: string[]; decision?: Decision[]; main_image_only: boolean; keyword?: string; limit: number; offset: number }
export interface Question {
  question_id: string;            // `${entry_id}#${ordinal}`
  entry_id: string;
  ordinal: number;
  title: string;
  rating: Rating;
  thumb_path: string | null;
  local_path: string | null;
  truth_source: "rawTag" | "tags" | "rawTag+characterPrompts" | "tags+characterPrompts";
  truth_preview: string;          // 归一后的答案前 200 字
  /** v0.2.4 前端追加（可选，见 docs/CONTRACT_CHANGES.md）：题目真值是不是 §11.1 散文；缺键时前端用 lib/prose.ts 从 truth_preview 退化判定。 */
  prose?: boolean;
}
export interface RunBatchRequest {
  question_ids: string[];         // 或本地文件：`local:<绝对路径>`
  template_id: number;
  ruleset_id: number;
  provider_ids: number[];
  note?: string;
  // ---- v0.2.4（§11.4）反推后重画并评分 ----
  render: boolean;                // 缺省 false；true = 每条解析成功的 run 交给插件按同一角色重画并评分（消耗点数）
  render_character: string | null;  // 重画角色；null / 空 → 薇拉
  render_seed: number | null;     // 固定种子；null → 插件随机并回传
  judge_provider_id: string | null; // 成对评委的 provider id（plugin_providers() 里的）；null → 插件默认
  // ---- v0.3.0（§12.5）----
  artist: string | null;          // 重画时的画师串；§12.6 起只在 render_artist_mode="given" 时插件才用它，其余模式忽略
  // ---- v0.3.0（§12.6 重画与 插件 /反推 对齐）----
  render_artist_mode: RenderArtistMode;  // 画师来源，缺省 "pool"（与线上同一概率抽画风池）；"given" 用 artist；"none" 纯标签对照；"style" 精准反推（§12.7）
  render_count: number | null;    // 每条 run 出几张（1–4，全部评分、取最高分那张）；null = 插件配置的默认（线上 reverse_draw.candidates）
  // ---- v0.3.1（§12.7 精准反推）：只在 render_artist_mode="style" 时后端才看；前端总是显式发（其余模式发契约缺省）----
  style_top_k: number;            // 检索候选数，缺省 5；取过滤后第一条可用的
  style_decisions: Decision[];    // 决策范围，空 = 全部；黑名单命中的永不用
  style_min_score: number | null; // 相似度低于它时按 style_fallback 处理；null = 不设门槛
  style_fallback: StyleFallback;  // 无命中 / 低于门槛时：pool = 画风池随机（缺省），none = 不带画风
  style_center: boolean;          // 缺省 true：减库均值再算余弦（减库均值，同 §12.2 center）
  style_model: string | null;     // null = 批次开始时固定当前模型
}
/**
 * §12.6 画师来源 / 画师模式：pool = 画风池随机（与线上 `/反推` 同一函数同一概率）；given = 指定画师串；none = 不带画风；
 * §12.7 style = 精准反推——桌面端自动用画风索引检索原图最像的一条，翻译成对插件的 given + 命中串（插件不改）。
 */
export type RenderArtistMode = "pool" | "given" | "none" | "style";
/** §12.7 精准反推的兜底方式（无命中或 score < style_min_score 时）：pool = 画风池随机，none = 不带画风。 */
export type StyleFallback = "pool" | "none";
/** §12.6 一张重画候选（插件 `images[]` 的一项，后端拉图后补 `local_path`）；顶层 render_path / render_seed / quality_* 是其中最优那张。 */
export interface RenderCandidate {
  file: string;
  seed: number | null;
  quality_score: number | null;
  quality_notes: string[];
  local_path: string | null;      // labruns/reverse/<run_id>_<idx>.jpg 的绝对路径；没取到为 null
}
export interface RunEvent {
  batch_id: string;
  run_id: number | null;
  status: "started" | "run_done" | "run_failed" | "batch_done" | "batch_failed" | "batch_cancelled";
  done: number;
  total: number;
  message: string | null;
}
export interface BatchSummary {
  batch_id: string; created_at: string; finished_at: string | null; status: string;
  template: string; ruleset: string; providers: string[]; question_count: number; note: string | null;
}
export interface Metric {
  hit: number; missed: number; extra: number; expected_hit: number;
  precision: number | null; recall: number | null; f1: number | null;
  appearance_leak: number; banned_leak: number; ruleset_drop: number; contradiction: number;
}
/** v0.2.4（§11.4）：tags = 标签题（有指标）；prose = 真值是 §11.1 散文（metric 为 null）；none = 本地图片题。 */
export type TruthKind = "tags" | "prose" | "none";
export interface RunRow {
  run_id: number; question_id: string | null; provider_id: number; provider: string;
  status: "ok" | "refused" | "error"; latency_ms: number | null; error: string | null;
  cleaned_tags: string[]; metric: Metric | null; thumb_path: string | null;
  // ---- v0.2.4（§11.4）----
  truth_kind: TruthKind;
  render_path: string | null;     // 重画图的本地绝对路径（labruns/reverse/<run_id>.jpg）；没重画 / 没取到图为 null
  render_seed: number | null;
  render_prompt: string | null;
  quality_score: number | null;   // 0–10，插件 _score_image；评委没配 / 失败为 null
  quality_notes: string[];
  similarity_score: number | null;  // 0–10，成对评委"像原图"；没原图 / 评委失败为 null
  similarity_reason: string | null;
  render_error: string | null;    // 重画任一步失败只写这里，已写入的出图结果保留
  leak_appearance: number;        // 按 pred_raw 算的泄漏数，任何 truth_kind 都填（tags 题与 metric 里的相同）
  leak_banned: number;
  // ---- v0.3.0（§12.5）----
  render_artist: string | null;   // 这次重画实际注入的画师串（pool 抽中的 / given 指定的）；none 或没重画为 null
  // ---- v0.3.0（§12.6）：插件回传的对齐信息；老插件 / 没重画时字符串为 null、数组为空、布尔为 false ----
  render_artist_source: RenderArtistMode | null;  // 插件回传的画师来源：pool 只在真抽中时；given 且串非空；其余 none
  render_size: string | null;     // 重画尺寸 "WxH"
  render_size_source: string | null;  // "tags" = 缺原图宽高退到 size_for_tags；其它非空（插件现发 "ratio"）= 按原图宽高比 size_for_ratio
  plugin_dropped: string[];       // 线上 _clean_tags 口径会比实验室多剔的 token（原顺序原写法）
  plugin_would_reject: boolean;   // 线上口径下清洗后 < 5 条（插件 /反推 会报错，实验室照常出图只标）
  render_candidates: RenderCandidate[];  // 全部候选（含最优那张），按 seed 顺序
  // ---- v0.3.1（§12.7 精准反推，迁移 007）：render_artist_mode="style" 时的检索记录；其余模式 / 老记录：字符串与分数 null、布尔 false ----
  render_style_entry_id: string | null;   // 命中条目（用了它的画师串；低于门槛未采用时后端也可能记下它）
  render_style_title: string | null;
  render_style_score: number | null;      // 余弦相似度 -1..1
  render_style_decision: Decision | null; // 命中条目的决策（"不加入"也可能命中，取决于 style_decisions）
  render_style_fallback: boolean;         // true = 无命中或低于 style_min_score，按 style_fallback 兜底（列是 INTEGER 0/1）
}
export interface ProviderSummary {
  provider_id: number; provider: string; runs: number; ok: number; refused: number; errors: number;
  precision: number | null; recall: number | null; f1: number | null;   // v0.2.4 起只聚合 truth_kind='tags' 且 metric 非空的 run
  appearance_leak: number; banned_leak: number; contradiction: number; avg_latency_ms: number | null;
  // ---- v0.2.4（§11.4）----
  tags_n: number;                 // 参与 P/R/F1 的题数
  render_n: number;               // 取到重画图的 run 数
  quality_avg: number | null;
  similarity_avg: number | null;
  // ---- v0.3.1（§12.7）----
  style_hit_n: number;            // 用到检索画风的张数（render_artist_source="style" 的 run 数）
  style_score_avg: number | null; // 这些 run 的 render_style_score 均值；一张都没有为 null
}
export interface BatchResults { batch: BatchSummary; runs: RunRow[]; summary: ProviderSummary[] }
export interface RunDetail {
  run: RunRow;
  raw_response: string | null;
  parsed: Record<string, unknown> | null;
  pred_raw: string[];
  pred: string[];
  truth: string[];
  truth_eval: string[];
  expected_miss: string[];
  hits: string[]; misses: string[]; extras: string[]; expected_hits: string[]; contradictions: string[];
  request: Record<string, unknown>;
  judgment: { score: number | null; comment: string | null; at: string } | null;
}
export interface CompareRow { question_id: string; f1_a: number | null; f1_b: number | null; delta: number | null; run_a: number | null; run_b: number | null }
export interface CompareResult { batch_a: BatchSummary; batch_b: BatchSummary; rows: CompareRow[]; avg_f1_a: number | null; avg_f1_b: number | null }

// ---- v0.3.0 画风索引与反推画风（CONTRACT §12.2 / §12.5）----
/** style_model_status：权重是否在数据目录 models/<model>/ 里、索引进度（indexed = 当前模型已有向量的展示图数，total = 应索引数）。 */
export interface StyleModelStatus {
  model: string;                  // 当前模型名，缺省 clip-vit-base-patch32（app_state('style_model')）
  label: string;
  description: string;
  dim: number;
  download_size_bytes: number;
  default_center: boolean;
  dir: string;                    // 权重目录绝对路径
  present: boolean;               // 非空 config.json + pytorch_model.bin（或已有 model.safetensors）
  size_bytes: number;             // 权重文件总大小；未下载为 0
  indexed: number;
  total: number;
  mean_ready: boolean;            // 画风词典库均值已算好
  device_preference: StyleDevicePreference;
  active_device: "cpu" | "metal" | null; // 当前选择尚未加载模型时为 null
  metal_available: boolean;       // 当前构建包含 Metal 支持；不保证设备初始化成功
  device_message: string | null;  // 自动回退 CPU 的原因 / 当前构建不可用说明
}
/** style_model_download 的进度事件：按文件发；total_bytes 取不到 Content-Length 时为 0（前端画跑马灯）；error 非空 = 失败。 */
export interface DownloadEvent {
  file: string;
  done_bytes: number;
  total_bytes: number;
  finished: boolean;
  error: string | null;
}
/** 检索来源：题库里的一道题（条目 × ordinal）或一张本地图片（绝对路径）。 */
export type StyleSearchSource =
  | { kind: "entry"; entry_id: string; ordinal: number }
  | { kind: "path"; path: string };
export interface StyleSearchRequest {
  model?: string;
  source: StyleSearchSource;
  top_k: number;                  // 1..50，缺省 10
  codex_ids: string[];            // 画风检索只含 artist_nai5_personal
  decisions: Decision[];          // 空 = 全部
  center: boolean;                // 查询与库向量都减去库均值再算余弦
}
export interface StyleHit {
  entry_id: string;
  codex_id: string;
  title: string;
  path: string[];
  decision: Decision;
  blacklisted: boolean;           // 恒 false：黑名单命中的永远不返回（§12.0）
  score: number;                  // 余弦相似度 -1..1，降序
  style_prompt: string;           // 完整原始画风配方，复制与重画均使用此字段
  core: string | null;            // 画师条目：§3.1 第 5 步的串（不带质量后缀）
  pack_tags: string | null;       // 图包条目：导出用的 pack.tags（散文原文或清洗后标签，原文里含画师块）
  thumb_path: string | null;
  prose: boolean;
}
export interface StyleSearchResult {
  model: string;
  indexed: number;
  query_ms: number;
  hits: StyleHit[];
}

/** 独立画师分类器：输出标签置信分，未参与画风向量索引。 */
export interface StyleClassifierStatus {
  model: string;
  label: string;
  description: string;
  download_size_bytes: number;
  present: boolean;
  dir: string;
  runtime_ready: boolean;
}
export interface StyleClassifyRequest { source: StyleSearchSource; model: string; top_k: number }
export interface StyleClassifyResult {
  model: string;
  label: string;
  query_ms: number;
  hits: { artist: string; score: number }[];
}

export interface AppEvent { id: number; at: string; level: "info" | "warn" | "error"; area: string; message: string; data: unknown }
