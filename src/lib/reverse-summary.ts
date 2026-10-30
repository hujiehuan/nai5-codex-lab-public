// 反推结果表 / 单题面板 / 运行配置里 v0.2.4（CONTRACT §11.4）新增展示的纯函数：truth_kind 徽标、可空平均值与 0–10 分、
// 泄漏数（任何 truth_kind 都有）、重画状态一格、RunConfig 重画表单 → RunBatchRequest 的重画键、重画后替换结果行；
// v0.3.0（§12.6）：画师来源 / 张数（表单与单条重画的参数）、尺寸来源与画师来源的文字、线上多剔词提示、最优候选；
// v0.3.1（§12.7 精准反推）：画师来源第四个取值 "style" 的子表单（top_k / 决策范围 / 最低相似度 / 兜底 / 减库均值）→ 请求的六个 style_* 键、
// 结果表的"检索"徽标与每模型两列、单题面板的"命中画风"块。
// BatchResults.vue / RunDetailPanel.vue / RunConfig.vue / StylePanel.vue 只做渲染与调用。
import type { Decision, ProviderSummary, RenderArtistMode, RenderCandidate, RunBatchRequest, RunRow, StyleFallback, TruthKind } from "../types";
import { DECISION_LABEL } from "./decision";
import { DEFAULT_CHARACTER, LAB_MAX_SEED, fmtScore, parseSeed } from "./lab";
import { ALL_DECISIONS } from "./query";
import { fmtCosine } from "./style";

export const TRUTH_KIND_LABEL: Record<TruthKind, string> = { tags: "标签", prose: "散文", none: "本地" };
/** §11.4：散文题的运行详情提示。 */
export const PROSE_RUN_NOTE = "散文题：不计 P/R/F1，只看重画对照与泄漏";
export const NONE_RUN_NOTE = "本地图片题：没有标准答案，只看输出、重画对照与泄漏";
/** 成对评委下拉的空选项文字（§11.4：可空 = 插件默认）。 */
export const JUDGE_DEFAULT_LABEL = "插件默认";

type KindSource = Pick<RunRow, "question_id"> & { truth_kind?: string | null };

/** 老后端 / 老记录没有 truth_kind 时：本地图片题 → none，其余 → tags（与迁移 005 的回填一致）；不认识的值也这样退。 */
export function truthKindOf(r: KindSource): TruthKind {
  const k = r.truth_kind;
  if (k === "tags" || k === "prose" || k === "none") return k;
  return r.question_id === null || r.question_id.startsWith("local:") ? "none" : "tags";
}

/** 结果表里的徽标：标签题不标，散文 / 本地各给两字。 */
export function truthKindBadge(r: KindSource): string | null {
  const k = truthKindOf(r);
  return k === "tags" ? null : TRUTH_KIND_LABEL[k];
}

/** 单题面板顶部的提示：散文 / 本地各一句，标签题没有。 */
export function truthKindNote(r: KindSource): string | null {
  const k = truthKindOf(r);
  return k === "prose" ? PROSE_RUN_NOTE : k === "none" ? NONE_RUN_NOTE : null;
}

/** 可空平均值（重画质量 / 像原图的 0–10 均值）：null / undefined / NaN → "—"，否则固定 1 位小数。 */
export function fmtAvg(v: number | null | undefined, digits = 1): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "—";
  return v.toFixed(digits);
}

/** 可空计数（tags_n / render_n；老后端没有这两键）：null / undefined → "—"。 */
export function fmtCount(v: number | null | undefined): string {
  return v === null || v === undefined || !Number.isFinite(v) ? "—" : String(v);
}

type LeakSource = Pick<RunRow, "metric"> & { leak_appearance?: number | null; leak_banned?: number | null };

/** 泄漏数：顶层 `leak_appearance` / `leak_banned` 优先（任何 truth_kind 都填），老后端只有 metric 时退到 metric，都没有 → null。 */
export function leakCounts(r: LeakSource): { appearance: number; banned: number } | null {
  if (typeof r.leak_appearance === "number" && typeof r.leak_banned === "number") return { appearance: r.leak_appearance, banned: r.leak_banned };
  if (r.metric) return { appearance: r.metric.appearance_leak, banned: r.metric.banned_leak };
  return null;
}

/** 结果表"泄漏"一格：`外貌/禁词`，没有 → "—"。 */
export function fmtLeaks(r: LeakSource): string {
  const l = leakCounts(r);
  return l ? `${l.appearance}/${l.banned}` : "—";
}

export type RenderState = "none" | "ok" | "error";
type RenderSource = Pick<RunRow, "render_path" | "render_error" | "render_seed" | "quality_score" | "similarity_score">;

/** 重画状态：写了 render_error → error；有图或有任一出图结果（seed / 分数）→ ok；都没有 → none（没重画）。 */
export function renderState(r: RenderSource): RenderState {
  if (r.render_error) return "error";
  if (r.render_path || r.render_seed !== null || r.quality_score !== null || r.similarity_score !== null) return "ok";
  return "none";
}

/** 结果表"重画"一格：没重画 "—"；失败 "失败"；成功 `质量 7.5 · 像 8`（缺的分数用 —）。 */
export function fmtRenderCell(r: RenderSource): string {
  const s = renderState(r);
  if (s === "none") return "—";
  if (s === "error") return "失败";
  return `质量 ${fmtScore(r.quality_score)} · 像 ${fmtScore(r.similarity_score)}`;
}

/** 汇总表一行的四个新列（老后端缺键时显示 —）。 */
export function summaryRenderCells(s: Pick<ProviderSummary, "tags_n" | "render_n" | "quality_avg" | "similarity_avg">): { tags_n: string; render_n: string; quality_avg: string; similarity_avg: string } {
  return { tags_n: fmtCount(s.tags_n), render_n: fmtCount(s.render_n), quality_avg: fmtAvg(s.quality_avg), similarity_avg: fmtAvg(s.similarity_avg) };
}

// ---- RunConfig 的重画表单（§11.4 / §12.6）----

/** §12.6 批次里可选的画师来源：pool / none，§12.7 起加 style（精准反推）；"given"（指定画师串）只在画风反推面板的"用它重画"里用。 */
export type BatchArtistMode = Exclude<RenderArtistMode, "given">;
export const BATCH_ARTIST_MODES: readonly BatchArtistMode[] = ["pool", "none", "style"];
/** 契约缺省（§12.6）：与线上 `/反推` 同一概率抽画风池。 */
export const RENDER_ARTIST_MODE_DEFAULT: RenderArtistMode = "pool";
/** §12.6：张数上限（线上 `reverse_draw.candidates` 的上限）。 */
export const RENDER_COUNT_MAX = 4;
/** 画师来源下拉 / 徽标的文字。 */
export const RENDER_ARTIST_MODE_LABEL: Record<RenderArtistMode, string> = { pool: "画风池随机", given: "指定画师", none: "不带画风", style: "精准反推（画风检索）" };
/** 画师来源下拉里每项后面的解释（title）。 */
export const RENDER_ARTIST_MODE_HINT: Record<RenderArtistMode, string> = {
  pool: "与 插件 /反推 同一函数同一概率从画风池抽，未命中为空——和线上完全一致",
  given: "用给定的画师串（只在底部「画风反推」的“用它重画”里用）",
  none: "不加任何画师，纯标签对照",
  style: "每条 run 自动用画风索引在画风词典里检索原图最像的一条，把完整画风配方加进重画（§12.7）：一次同时反推“画什么”和“谁的画风”；索引未建 / 模型未下载时批次开始前就报错，不消耗点数",
};
/** 张数下拉的空选项文字（null = 插件配置的默认，线上 `reverse_draw.candidates`）。 */
export const RENDER_COUNT_DEFAULT_LABEL = "插件默认";

// ---- §12.7 精准反推的子表单 ----

/** 契约缺省 5（§12.7：检索候选数，取过滤后第一条可用的）；界面范围 1–20（契约没给上限，§12.2 检索本身收 1..50，这里够用）。 */
export const STYLE_RENDER_TOP_K_DEFAULT = 5;
export const STYLE_RENDER_TOP_K_MIN = 1;
export const STYLE_RENDER_TOP_K_MAX = 20;
export const STYLE_FALLBACKS: readonly StyleFallback[] = ["pool", "none"];
/** 兜底下拉的文字（与画师来源的 pool / none 同义）。 */
export const STYLE_FALLBACK_LABEL: Record<StyleFallback, string> = { pool: "画风池随机", none: "不带画风" };
export const STYLE_FALLBACK_HINT: Record<StyleFallback, string> = {
  pool: "检索无命中或相似度低于门槛时，退回与线上同一概率抽画风池",
  none: "检索无命中或相似度低于门槛时，不加画师、纯标签重画",
};
export const STYLE_MIN_SCORE_ERROR = "最低相似度须是 0–1 的数（留空 = 不设门槛）";
/** §12.7：索引未建 / 模型未下载时运行配置里的提示（后端会在批次开始前 bad_request，不消耗点数）。 */
export const STYLE_INDEX_MISSING_NOTE = "先在设置页下载模型并建索引";

/**
 * 精准反推子表单：`top_k` 1–20；`decisions` 勾了的决策（**空与全勾都 = 全部**，请求里发 `[]`）；`min_score` 是输入框原文（空 = 不设门槛）；
 * `fallback` 兜底方式；`center` 减库均值（缺省开）。
 */
export interface StyleRenderForm { model: string | null; top_k: number; decisions: Decision[]; min_score: string; fallback: StyleFallback; center: boolean }
/** 契约缺省：top_k 5、全部决策、不设门槛、兜底画风池、减库均值开。每次给新对象（组件会 v-model 改它）。 */
export function defaultStyleRenderForm(): StyleRenderForm {
  return { model: null, top_k: STYLE_RENDER_TOP_K_DEFAULT, decisions: [...ALL_DECISIONS], min_score: "", fallback: "pool", center: true };
}

/**
 * 表单形状：`seed` 是输入框原文（空 = 随机），`judge_provider_id` 空串 = 插件默认；
 * `artist_mode`（§12.6）批次只有 pool / none，§12.7 起加 style；`count` null = 插件默认张数，1–4 每张都消耗点数；
 * `style`（§12.7）只在 artist_mode="style" 时进请求。
 * v0.3.0 §12.5 曾有的批次级"画师串"文本域随 §12.6 去掉：指定画师只走画风反推面板。
 */
export interface RenderForm { render: boolean; character: string; seed: string; judge_provider_id: string; artist_mode: BatchArtistMode; count: number | null; style: StyleRenderForm }
/** 缺省表单（只读参照；要拿来改用 defaultRenderForm()——嵌套的 style 子对象浅拷贝会共用）。 */
export const DEFAULT_RENDER_FORM: Readonly<RenderForm> = Object.freeze({
  render: false, character: DEFAULT_CHARACTER, seed: "", judge_provider_id: "", artist_mode: "pool", count: null, style: Object.freeze(defaultStyleRenderForm()),
});
/** 缺省表单的深拷贝（store 用）。 */
export function defaultRenderForm(): RenderForm {
  return { ...DEFAULT_RENDER_FORM, style: defaultStyleRenderForm() };
}
export const RENDER_SEED_ERROR = `固定种子须是 0–${LAB_MAX_SEED} 的整数`;

/** 种子输入框的校验文字：合法（含空）→ null。 */
export function renderSeedError(text: string): string | null {
  return parseSeed(text) === undefined ? RENDER_SEED_ERROR : null;
}

/** 画师来源归一：认识的原样（§12.7 起含 style）；缺省 / 不认识 → pool（契约缺省）。 */
export function normalizeArtistMode(v: unknown): RenderArtistMode {
  return v === "given" || v === "none" || v === "style" ? v : "pool";
}
/** 批次表单里的画师来源：given 不在批次里（也 → pool）；none / style 原样。 */
export function normalizeBatchArtistMode(v: unknown): BatchArtistMode {
  return v === "none" || v === "style" ? v : "pool";
}
/** §12.7 top_k 收进 1..20 的整数；非数 → 缺省 5。 */
export function clampStyleTopK(n: unknown): number {
  const v = typeof n === "number" && Number.isFinite(n) ? Math.round(n) : STYLE_RENDER_TOP_K_DEFAULT;
  return Math.min(STYLE_RENDER_TOP_K_MAX, Math.max(STYLE_RENDER_TOP_K_MIN, v));
}
/** 最低相似度输入框：空 → null（不设门槛）；0..1 的数 → 数；其它（非数 / 越界）→ undefined（非法，组件挡在按钮上）。 */
export function parseMinScore(text: string): number | null | undefined {
  const s = text.trim();
  if (!s) return null;
  if (!/^(\d+\.?\d*|\.\d+)$/.test(s)) return undefined;
  const n = Number(s);
  return Number.isFinite(n) && n >= 0 && n <= 1 ? n : undefined;
}
/** 最低相似度输入框的校验文字：合法（含空）→ null。 */
export function styleMinScoreError(text: string): string | null {
  return parseMinScore(text) === undefined ? STYLE_MIN_SCORE_ERROR : null;
}
/** 兜底方式归一：none 原样，其余 pool（契约缺省）。 */
export function normalizeStyleFallback(v: unknown): StyleFallback {
  return v === "none" ? "none" : "pool";
}

/** §12.7 RunBatchRequest / reverse_render_run 的六个 style_* 键。 */
export type StyleRenderFields = Pick<RunBatchRequest, "style_model" | "style_top_k" | "style_decisions" | "style_min_score" | "style_fallback" | "style_center">;
/** 契约缺省：top_k 5、全部（[]）、不设门槛、兜底 pool、减库均值开。非 style 模式也发这一份（后端不看）。每次给新对象。 */
export function styleRenderDefaults(): StyleRenderFields {
  return { style_model: null, style_top_k: STYLE_RENDER_TOP_K_DEFAULT, style_decisions: [], style_min_score: null, style_fallback: "pool", style_center: true };
}
/**
 * 子表单 → 六个 style_* 键：画师来源不是 style（或没有子表单）→ 契约缺省；是 style → top_k 收进 1..20、
 * 决策按契约顺序去重丢掉不认识的，**空或全勾都发 `[]`**（= 全部；与画风反推面板不同，这里不挡）、门槛空 / 非法 → null、兜底归一、减库均值布尔化。
 */
export function styleRequestFields(form: StyleRenderForm | null | undefined, mode: RenderArtistMode): StyleRenderFields {
  if (mode !== "style" || !form) return styleRenderDefaults();
  const decisions = ALL_DECISIONS.filter((d) => form.decisions.includes(d));
  return {
    style_model: form.model?.trim() || null,
    style_top_k: clampStyleTopK(form.top_k),
    style_decisions: decisions.length === 0 || decisions.length === ALL_DECISIONS.length ? [] : decisions,
    style_min_score: parseMinScore(form.min_score) ?? null,
    style_fallback: normalizeStyleFallback(form.fallback),
    style_center: !!form.center,
  };
}
/** 张数：1..4 的整数原样；null / 非数 / 非整数 / 越界 → null（= 插件默认；下拉根本选不到越界值，这里只是兜底）。 */
export function normalizeRenderCount(v: unknown): number | null {
  if (typeof v !== "number" || !Number.isInteger(v)) return null;
  return v >= 1 && v <= RENDER_COUNT_MAX ? v : null;
}
/** 张数下拉：插件默认（null）在前，其后 1..4。 */
export function renderCountOptions(): { value: number | null; label: string }[] {
  const out: { value: number | null; label: string }[] = [{ value: null, label: RENDER_COUNT_DEFAULT_LABEL }];
  for (let n = 1; n <= RENDER_COUNT_MAX; n++) out.push({ value: n, label: `${n} 张` });
  return out;
}

export type RenderRequestFields = Pick<RunBatchRequest, "render" | "render_character" | "render_seed" | "judge_provider_id" | "artist" | "render_artist_mode" | "render_count"> & StyleRenderFields;

/**
 * 表单 → RunBatchRequest 的七个重画键 + 六个 style_* 键：不勾重画时全部"关"（render=false、其余 null，`render_artist_mode` 仍发契约缺省 pool，
 * style_* 发契约缺省），后端不看；勾了：角色去首尾空白、空 → null（后端与插件缺省薇拉）；种子空 / 非法 → null（随机；非法的由组件挡在按钮上）；
 * 评委空串 → null；`artist` 恒 null（批次不走 given）；`render_artist_mode` = 表单的 pool / none / style；`render_count` 1..4 或 null（插件默认）；
 * style_* 只在 artist_mode="style" 时取子表单（§12.7），其余模式发契约缺省。
 */
export function renderRequestFields(form: RenderForm): RenderRequestFields {
  if (!form.render) {
    return { render: false, render_character: null, render_seed: null, judge_provider_id: null, artist: null, render_artist_mode: RENDER_ARTIST_MODE_DEFAULT, render_count: null, ...styleRenderDefaults() };
  }
  const character = form.character.trim();
  const judge = form.judge_provider_id.trim();
  const mode = normalizeBatchArtistMode(form.artist_mode);
  return {
    render: true,
    render_character: character || null,
    render_seed: parseSeed(form.seed) ?? null,
    judge_provider_id: judge || null,
    artist: null,
    render_artist_mode: mode,
    render_count: normalizeRenderCount(form.count),
    ...styleRequestFields(form.style, mode),
  };
}

/** 单条重画（`reverse_render_run`）的 §12.5 / §12.6 三个参数 + §12.7 的六个 style_* 键。 */
export interface RerenderParams { artist: string | null; artist_mode: RenderArtistMode; count: number | null; style: StyleRenderFields }

/**
 * 单条重画的参数：画师来源 = 明确给的 `artistMode`，否则给了画师串就是 given（画风反推"用它重画"），否则沿用批次表单的 pool / none / style
 * （单题面板的"重画"按钮——与批次配置相同）；given 却没有非空画师串 → 退回批次表单的来源（后端会 bad_request）；
 * `artist` 只在 given 时送出（去首尾空白），其余模式 null（插件本来就忽略）；张数总是批次表单当前选的；
 * style_* 只在最终来源是 style 时取子表单（没有子表单 → 契约缺省），其余模式发契约缺省。
 */
export function rerenderParams(form: Pick<RenderForm, "artist_mode" | "count"> & Partial<Pick<RenderForm, "style">>, artist: string | null = null, artistMode: RenderArtistMode | null = null): RerenderParams {
  const a = artist?.trim() || null;
  let mode: RenderArtistMode = artistMode ?? (a ? "given" : normalizeBatchArtistMode(form.artist_mode));
  if (mode === "given" && !a) mode = normalizeBatchArtistMode(form.artist_mode);
  return { artist: mode === "given" ? a : null, artist_mode: mode, count: normalizeRenderCount(form.count), style: styleRequestFields(form.style, mode) };
}

/** 成对评委下拉的选项：空值 = 插件默认在最前，其余按插件给的顺序去重去空；当前值不在列表里（插件里删了 / 还没取到）也保留一项。 */
export function judgeOptions(pluginIds: readonly string[] | null, current: string): { value: string; label: string; disabled?: boolean }[] {
  const out: { value: string; label: string; disabled?: boolean }[] = [{ value: "", label: JUDGE_DEFAULT_LABEL }];
  const seen = new Set<string>();
  for (const id of pluginIds ?? []) {
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push({ value: id, label: id });
  }
  const cur = current.trim();
  if (cur && !seen.has(cur)) out.push({ value: cur, label: `${cur}（已不在可选名单）`, disabled: true });
  return out;
}

/** 单条重画后把返回的 RunRow 换进结果表（按 run_id）；不在表里则原样返回同一个数组。 */
export function replaceRun(runs: readonly RunRow[], row: RunRow): RunRow[] {
  const i = runs.findIndex((r) => r.run_id === row.run_id);
  if (i < 0) return runs as RunRow[];
  const out = runs.slice();
  out[i] = row;
  return out;
}

// ---- 单题面板的对齐信息（§12.6）----

export const SIZE_SOURCE_TAGS = "tags";
export const SIZE_SOURCE_LABEL_TAGS = "按标签";
export const SIZE_SOURCE_LABEL_RATIO = "按原图比例";

/**
 * 尺寸来源：null / 空 → null（老插件 / 没重画）；"tags" → 按标签（缺原图宽高退到 `size_for_tags`，契约里唯一点名的值）；
 * 其它非空（插件现发 "ratio"）→ 按原图比例（与线上 `_size_for_image` 同一函数）。大小写与首尾空白不敏感。
 */
export function sizeSourceLabel(source: string | null | undefined): string | null {
  const s = source?.trim().toLowerCase();
  if (!s) return null;
  return s === SIZE_SOURCE_TAGS ? SIZE_SOURCE_LABEL_TAGS : SIZE_SOURCE_LABEL_RATIO;
}

/** 单题面板"尺寸"一行：`832x1216 · 按原图比例`；老插件只有尺寸没来源就只给尺寸；只有来源没尺寸给来源；都没有 → null。 */
export function fmtRenderSize(r: Pick<RunRow, "render_size" | "render_size_source">): string | null {
  const size = r.render_size?.trim() || null;
  const src = sizeSourceLabel(r.render_size_source);
  if (!size) return src;
  return src ? `${size} · ${src}` : size;
}

/** 画师来源徽标文字：pool → 画风池抽中；given → 指定画师；none → 不带画风；style（§12.7）→ 检索命中。 */
export const ARTIST_SOURCE_LABEL: Record<RenderArtistMode, string> = { pool: "画风池抽中", given: "指定画师", none: "不带画风", style: "检索命中" };

/** 画师来源：null / 空 → null（老插件 / 没重画）；认识的给中文；不认识的原样显示（去首尾空白、小写）。 */
export function artistSourceLabel(source: string | null | undefined): string | null {
  const s = source?.trim().toLowerCase();
  if (!s) return null;
  return (ARTIST_SOURCE_LABEL as Record<string, string>)[s] ?? s;
}

/** §12.6：线上口径下清洗后 < 5 条时的红字（实验室照常出图，只标）。 */
export const WOULD_REJECT_NOTE = "线上口径下清洗后不足 5 条：插件 /反推 会直接报错，实验室照常出图——这条 run 的标签在线上跑不出来";
/** 新插件回了对齐信息但一个词都没多剔时的灰字。 */
export const NO_PLUGIN_DROPPED_NOTE = "线上口径不会多剔词（与实验室清洗一致）";

/** 这条 run 有没有 §12.6 的对齐信息（新插件回了 `size_source` / `artist_source` 任一）：老插件的记录不显示"线上不会多剔词"。 */
export function hasAlignInfo(r: Pick<RunRow, "render_size_source" | "render_artist_source">): boolean {
  return !!(r.render_size_source || r.render_artist_source);
}

/**
 * 最优候选下标：先按 seed 与顶层 `render_seed` 对（顶层 file / seed / quality_* 就是最优那张），对不上按 `quality_score` 最高
 * （并列取先出现的，与插件 `sc > best_score` 相同），一张都没分 → 0；没有候选 → null。
 */
export function bestCandidateIndex(candidates: readonly Pick<RenderCandidate, "seed" | "quality_score">[], run: Pick<RunRow, "render_seed">): number | null {
  if (!candidates.length) return null;
  if (run.render_seed !== null && run.render_seed !== undefined) {
    const i = candidates.findIndex((c) => c.seed === run.render_seed);
    if (i >= 0) return i;
  }
  let best = -1;
  let bestScore = Number.NEGATIVE_INFINITY;
  candidates.forEach((c, i) => {
    const s = c.quality_score;
    if (typeof s === "number" && Number.isFinite(s) && s > bestScore) {
      best = i;
      bestScore = s;
    }
  });
  return best >= 0 ? best : 0;
}

/** 候选缩略图下的一行：`seed 123 · 7.5`（缺的用 —）。 */
export function candidateLabel(c: Pick<RenderCandidate, "seed" | "quality_score">): string {
  return `seed ${c.seed ?? "—"} · ${fmtScore(c.quality_score)}`;
}

// ---- §12.7 精准反推：结果表与单题面板 ----

/** 结果表逐题行的小徽标（render_artist_source="style"）。 */
export const STYLE_RUN_BADGE = "检索";
/** 兜底了：命中过但低于门槛 / 根本没命中。 */
export const STYLE_FALLBACK_BELOW_NOTE = "命中但相似度低于最低门槛，未采用它的画师串，已按兜底方式重画";
export const STYLE_FALLBACK_MISS_NOTE = "检索无命中（范围内没有可用的画师串），已按兜底方式重画";

type StyleSource = Pick<RunRow, "render_artist_source"> & Partial<Pick<RunRow, "render_style_entry_id" | "render_style_title" | "render_style_score" | "render_style_decision" | "render_style_fallback">>;

/** 这条 run 的画师串来自画风检索（插件回传的来源经后端翻译为 "style"）。 */
export function isStyleRun(r: Pick<RunRow, "render_artist_source">): boolean {
  return r.render_artist_source === "style";
}

/** 单题面板"命中画风"块要显示的东西；不是精准反推的 run（老记录 / 其它模式）→ null。 */
export interface StyleHitInfo {
  entry_id: string | null;
  title: string;                  // 标题；没有退到 entry_id；都没有 "—"
  score: string;                  // fmtCosine（三位小数）
  decision: Decision | null;
  decision_label: string | null;
  fallback: boolean;
  note: string | null;            // 兜底说明；没兜底 null
}
export function styleHitInfo(r: StyleSource): StyleHitInfo | null {
  const entry = r.render_style_entry_id ?? null;
  const fallback = r.render_style_fallback === true;
  if (!entry && !fallback && !isStyleRun(r)) return null;
  const decision = r.render_style_decision ?? null;
  return {
    entry_id: entry,
    title: r.render_style_title?.trim() || entry || "—",
    score: fmtCosine(r.render_style_score),
    decision,
    decision_label: decision ? (DECISION_LABEL as Record<string, string>)[decision] ?? decision : null,
    fallback,
    note: fallback ? (entry ? STYLE_FALLBACK_BELOW_NOTE : STYLE_FALLBACK_MISS_NOTE) : null,
  };
}

/** 结果表"检索"徽标的 title：`检索命中：标题 · 0.512 · 加入`；兜底了的附兜底说明。 */
export function styleRunTitle(r: StyleSource): string {
  const info = styleHitInfo(r);
  if (!info) return "";
  const parts = [`检索命中：${info.title}`, info.score];
  if (info.decision_label) parts.push(info.decision_label);
  const head = parts.join(" · ");
  return info.note ? `${head}\n${info.note}` : head;
}

/** 汇总表一行的两个新列（老后端缺键时显示 —）：检索命中张数、相似度均值（三位小数，与相似度条同口径）。 */
export function summaryStyleCells(s: Partial<Pick<ProviderSummary, "style_hit_n" | "style_score_avg">>): { style_hit_n: string; style_score_avg: string } {
  return { style_hit_n: fmtCount(s.style_hit_n), style_score_avg: fmtAvg(s.style_score_avg, 3) };
}
