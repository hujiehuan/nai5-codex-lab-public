// 画风索引与反推画风（CONTRACT §12）的纯函数：相似度 → 进度条与数字、命中卡片的文字 / 徽标 / 画师串、来源选项
//（当前 run 的原图 / 题库勾选 / 本地图片）、面板表单 → StyleSearchRequest、模型状态一行、下载与建索引的进度条。
// StylePanel.vue / SettingsView.vue / SyncView.vue 只做渲染与调用。
import type { Decision, DownloadEvent, Question, RunRow, StyleHit, StyleModelStatus, StyleSearchRequest, StyleSearchSource, SyncEvent } from "../types";
import { DECISION_LABEL } from "./decision";
import { fmtBytes } from "./format";
import { codexLabel } from "./pool";
import { ALL_DECISIONS } from "./query";
import { ARTIST_SOURCES } from "./artist-sources";

export const STYLE_TOP_K_DEFAULT = 10;
export const STYLE_TOP_K_MIN = 1;
export const STYLE_TOP_K_MAX = 50;
/** 画风索引与检索包含法典和 NAX 两个画师来源，社区图包不参与。 */
export const STYLE_CODEX_IDS: readonly string[] = ARTIST_SOURCES.map((source) => source.id);
export const STYLE_MODEL_DEFAULT = "clip-vit-base-patch32";
/** §12.0：权重约 600 MB，一次性。 */
export const STYLE_MODEL_SIZE_HINT = "约 600 MB，从 Hugging Face 一次性下载";
export const EMBED_PHASE_LABEL = "画风索引";
export const NO_SOURCE_HINT = "先在题库勾一道题或添加本地图片";
export const NO_RUN_HINT = "先在结果表点开一条 run";
export const NO_ARTIST_HINT = "这条命中没有完整画风配方";
export const RERENDER_BUSY_HINT = "上一次重画还没结束";
export const RERENDER_NO_TAGS_HINT = "当前 run 没有清洗后的标签，无法重画";
export const SETUP_LINK_LABEL = "去设置页下载 / 重建索引 →";

// ---- 相似度 ----

/** 余弦相似度 → 进度条宽度 0..1：负的（减库均值后不相似）按 0，> 1 按 1，非数按 0。数字本身另用 fmtCosine 原样显示。 */
export function scoreUnit(score: number): number {
  if (!Number.isFinite(score)) return 0;
  return Math.min(1, Math.max(0, score));
}
/** 0..100 的整数百分比（CSS width 用）。 */
export function scorePercent(score: number): number {
  return Math.round(scoreUnit(score) * 100);
}
/** 原始余弦值，固定 3 位小数（可为负）；非数 "—"。 */
export function fmtCosine(score: number | null | undefined): string {
  if (score === null || score === undefined || !Number.isFinite(score)) return "—";
  return score.toFixed(3);
}

// ---- 命中卡片 ----

/** 原始画风配方只去首尾空白，保留权重、质量词、年份、顺序与内部格式。 */
export function hitArtist(hit: Pick<StyleHit, "style_prompt">): string | null {
  return hit.style_prompt.trim() || null;
}
/** 分类器标签复制：保留标签内容，只补 artist: 前缀，不拼接预测分或权重。 */
export function classifierArtistTag(artist: string): string {
  const tag = artist.trim();
  return !tag || tag.startsWith("artist:") ? tag : `artist:${tag}`;
}
export function hitPath(hit: Pick<StyleHit, "path">): string {
  return hit.path.join(" / ");
}
/** 卡片 title 的一行：`标题 · 目录 / 子目录`；没有目录只有标题。 */
export function hitLabel(hit: Pick<StyleHit, "title" | "path">): string {
  const p = hitPath(hit);
  return p ? `${hit.title} · ${p}` : hit.title;
}
/** 徽标文字：词典中文名、决策、散文（有则加）。 */
export function hitBadges(hit: Pick<StyleHit, "codex_id" | "decision" | "prose">): string[] {
  const out = [codexLabel(hit.codex_id), DECISION_LABEL[hit.decision]];
  if (hit.prose) out.push("散文");
  return out;
}
/** §12.0："不加入"用灰色标出但不藏。 */
export function hitMuted(hit: Pick<StyleHit, "decision">): boolean {
  return hit.decision === "exclude";
}

// ---- 来源 ----

export interface StyleSourceOption {
  id: string;                     // 下拉的值：`run:<run_id>` / question_id / `local:<path>`
  label: string;
  thumb_path: string | null;      // 本地图片就是它自己
  source: StyleSearchSource;
}

/** question_id → 来源：`local:<绝对路径>` → path；`<entry_id>#<ordinal>` → entry（最后一个 # 之后是 ordinal，缺 / 非数按 0）；空 → null。 */
export function sourceFromQuestionId(id: string | null | undefined): StyleSearchSource | null {
  if (!id) return null;
  if (id.startsWith("local:")) {
    const path = id.slice("local:".length);
    return path ? { kind: "path", path } : null;
  }
  const i = id.lastIndexOf("#");
  if (i < 0) return { kind: "entry", entry_id: id, ordinal: 0 };
  const entry_id = id.slice(0, i);
  if (!entry_id) return null;
  const n = Number(id.slice(i + 1));
  return { kind: "entry", entry_id, ordinal: Number.isInteger(n) && n >= 0 ? n : 0 };
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}
function questionLabel(q: Pick<Question, "title" | "ordinal">): string {
  return q.ordinal ? `${q.title} #${q.ordinal + 1}` : q.title;
}

/**
 * 来源下拉的选项：当前 run 的原图（有的话）排最前，其后按题库勾选的先后（Set 的插入序）列出图包题与本地图片；
 * 同一道题只出现一次（run 那项优先）。题不在当前题库列表里（筛选换过）也保留，标题退化为 question_id。
 */
export function styleSourceOptions(
  selectedIds: Iterable<string>,
  questions: readonly Question[],
  run: Pick<RunRow, "run_id" | "question_id" | "thumb_path"> | null,
): StyleSourceOption[] {
  const out: StyleSourceOption[] = [];
  const seen = new Set<string>();
  const byId = new Map(questions.map((q) => [q.question_id, q] as const));
  const describe = (id: string, src: StyleSearchSource): { label: string; thumb: string | null } => {
    if (src.kind === "path") return { label: `本地 · ${basename(src.path)}`, thumb: src.path };
    const q = byId.get(id);
    return q ? { label: questionLabel(q), thumb: q.thumb_path } : { label: id, thumb: null };
  };
  if (run?.question_id) {
    const src = sourceFromQuestionId(run.question_id);
    if (src) {
      const d = describe(run.question_id, src);
      out.push({ id: `run:${run.run_id}`, label: `当前 run #${run.run_id} 的原图 · ${d.label}`, thumb_path: run.thumb_path ?? d.thumb, source: src });
      seen.add(run.question_id);
    }
  }
  for (const id of selectedIds) {
    if (seen.has(id)) continue;
    seen.add(id);
    const src = sourceFromQuestionId(id);
    if (!src) continue;
    const d = describe(id, src);
    out.push({ id, label: d.label, thumb_path: d.thumb, source: src });
  }
  return out;
}

/** 缺省选中：当前 run 的原图优先，否则最近勾的那个（列表最后一个）；没有选项 → null。 */
export function defaultSourceId(options: readonly StyleSourceOption[]): string | null {
  if (!options.length) return null;
  if (options[0].id.startsWith("run:")) return options[0].id;
  return options[options.length - 1].id;
}

// ---- 表单 → 请求 ----

/** 面板表单：固定画风词典 / 勾选的决策列表（全勾 = 请求里发空数组）、top_k、是否减库均值。 */
export interface StyleForm { model: string | null; top_k: number; codex_ids: string[]; decisions: Decision[]; center: boolean }

/** §12.2 缺省：top_k 10、画风词典、三种决策都勾、减库均值开。每次给新对象。 */
export function defaultStyleForm(): StyleForm {
  return { model: null, top_k: STYLE_TOP_K_DEFAULT, codex_ids: [...STYLE_CODEX_IDS], decisions: [...ALL_DECISIONS], center: true };
}

/** top_k 收进 1..50 的整数；非数 → 缺省 10。 */
export function clampTopK(n: unknown): number {
  const v = typeof n === "number" && Number.isFinite(n) ? Math.round(n) : STYLE_TOP_K_DEFAULT;
  return Math.min(STYLE_TOP_K_MAX, Math.max(STYLE_TOP_K_MIN, v));
}

/** 挡在按钮上的错误：一本词典 / 一种决策都不勾时（空数组在契约里是"不限"，与用户意图相反）。 */
export function styleFormError(form: Pick<StyleForm, "codex_ids" | "decisions">): string | null {
  if (!STYLE_CODEX_IDS.some((id) => form.codex_ids.includes(id))) return "至少勾一本词典";
  if (!ALL_DECISIONS.some((d) => form.decisions.includes(d))) return "至少勾一种决策";
  return null;
}

/**
 * 表单 → StyleSearchRequest：codex_ids 固定画风词典；decisions 按契约顺序去重，全勾 → `[]`（全部决策）；
 * top_k 收进 1..50；来源拷贝一份。
 */
export function buildStyleRequest(source: StyleSearchSource, form: StyleForm): StyleSearchRequest {
  const decisions = ALL_DECISIONS.filter((d) => form.decisions.includes(d));
  return {
    ...(form.model ? { model: form.model } : {}),
    source: { ...source },
    top_k: clampTopK(form.top_k),
    codex_ids: [...STYLE_CODEX_IDS],
    decisions: decisions.length === ALL_DECISIONS.length ? [] : decisions,
    center: !!form.center,
  };
}

/** §12.2 各字段缺省的请求：top_k 10、画风词典、全部决策（[]）、center=true。 */
export function defaultStyleRequest(source: StyleSearchSource): StyleSearchRequest {
  return buildStyleRequest(source, defaultStyleForm());
}

// ---- 模型状态 ----

/** 一行摘要：`已下载 605.0 MB · 已索引 1890 / 3200 · 均值就绪` / `未下载 · 已索引 0 / 3200`。 */
export function styleStatusLine(s: StyleModelStatus): string {
  const parts = [s.present ? `已下载 ${fmtBytes(s.size_bytes)}` : "未下载", `已索引 ${s.indexed} / ${s.total}`];
  if (s.present && s.mean_ready) parts.push("均值就绪");
  return parts.join(" · ");
}
/** 已索引 / 应索引，应索引为 0 时 null。 */
export function indexRatio(s: Pick<StyleModelStatus, "indexed" | "total">): number | null {
  if (!(s.total > 0)) return null;
  return Math.min(1, Math.max(0, s.indexed / s.total));
}
/** 能检索：权重在且至少有一条向量（否则后端 bad_request）。 */
export function styleReady(s: Pick<StyleModelStatus, "present" | "indexed"> | null | undefined): boolean {
  return !!s && s.present && s.indexed > 0;
}

// ---- 进度 ----

/** 下载进度 0..1；还没事件 / 没 Content-Length（total_bytes=0）→ null（跑马灯）。 */
export function downloadRatio(ev: Pick<DownloadEvent, "done_bytes" | "total_bytes"> | null | undefined): number | null {
  if (!ev || !(ev.total_bytes > 0)) return null;
  return Math.min(1, Math.max(0, ev.done_bytes / ev.total_bytes));
}
/** 进度条下的字：`pytorch_model.bin 100.0 MB / 600.0 MB（17%）`；没总量只给已下；失败 `文件：原因`；结束 `文件 完成`。 */
export function downloadLabel(ev: DownloadEvent | null | undefined): string {
  if (!ev) return "准备下载…";
  if (ev.error) return `${ev.file}：${ev.error}`;
  if (ev.finished) return `${ev.file} 完成`;
  const r = downloadRatio(ev);
  if (r === null) return `${ev.file} ${fmtBytes(ev.done_bytes)}`;
  return `${ev.file} ${fmtBytes(ev.done_bytes)} / ${fmtBytes(ev.total_bytes)}（${Math.round(r * 100)}%）`;
}
/** embedding 阶段的进度 0..1；没 embed_total（其它阶段 / 老后端 / 轮询到的 SyncStatus）→ null。 */
export function embedRatio(ev: Pick<SyncEvent, "embed_total" | "embed_done"> | null | undefined): number | null {
  if (!ev || typeof ev.embed_total !== "number" || !(ev.embed_total > 0)) return null;
  return Math.min(1, Math.max(0, (ev.embed_done ?? 0) / ev.embed_total));
}
/** `画风索引 120 / 3200，失败 2`；没计数 → `画风索引…`。 */
export function embedLabel(ev: Pick<SyncEvent, "embed_total" | "embed_done" | "embed_failed"> | null | undefined): string {
  if (!ev || typeof ev.embed_total !== "number") return `${EMBED_PHASE_LABEL}…`;
  const failed = ev.embed_failed ? `，失败 ${ev.embed_failed}` : "";
  return `${EMBED_PHASE_LABEL} ${ev.embed_done ?? 0} / ${ev.embed_total}${failed}`;
}

// ---- 错误 ----

/** style_search 的 bad_request = 模型未下载 / 索引为空（§12.2）：面板里给"去设置页"的链接。 */
export function isStyleSetupError(e: unknown): boolean {
  return typeof e === "object" && e !== null && (e as { code?: unknown }).code === "bad_request";
}
