// 画图实验室（CONTRACT §9）的纯函数：图包树与 scope 求解、清洗前后 diff、text 拼装、
// 默认请求与表单持久化（localStorage `codexlab.lab.<mode>`）、运行状态小工具。组件（components/lab/）只做渲染与调用。
import type { EntryCard, EntryQuery, LabImage, LabMode, LabPlan, LabRating, LabRun, LabRunRequest, LabRunStatus, Rating } from "../types";

export const LAB_MODES: readonly LabMode[] = ["draw"];
export const LAB_MODE_LABEL: Record<LabMode, string> = { draw: "画图实验室" };
/** 对应的插件指令，只用于界面提示。 */
export const LAB_COMMAND: Record<LabMode, string> = { draw: "/画" };
export const DEFAULT_CHARACTER = "薇拉";
export const RANDOM_CHARACTER = "随机";
export const PACK_CODEX_ID = "nai5_community_pack";
/** path 为空的条目在树里挂在这个虚拟根下（它没有可勾的 scope）。 */
export const UNFILED_NAME = "（无目录）";
export const LAB_PAGE_SIZE = 200;
export const LAB_HISTORY_LIMIT = 30;
export const LAB_POLL_MS = 2000;
export const LAB_MIN_COUNT = 1;
export const LAB_MAX_COUNT = 8;
/** NovelAI seed 上限（uint32）。 */
export const LAB_MAX_SEED = 4294967295;
export const CHAR_HISTORY_MAX = 20;
export const CHAR_HISTORY_KEY = "codexlab.lab.characters";

export const ARTIST_SOURCE_LABEL: Record<LabPlan["artist_source"], string> = { style: "风格=", pool: "画风池", none: "无画师" };
export const RUN_STATUS_LABEL: Record<LabRunStatus, string> = { queued: "排队中", running: "出图中", done: "完成", error: "失败" };
export const IMAGE_STATUS_LABEL: Record<LabImage["status"], string> = { pending: "生成中", done: "完成", error: "失败" };

// ---- 模式与查询 ----

/** §9.2-2 该模式的 rating：只有 safe。 */
export function ratingForMode(_mode: LabMode): Rating {
  return "safe";
}

/** 该模式在树里可见的条目：只认 safe（库里本来只有 safe，这里再保险一次）。 */
export function visibleForMode(entry: Pick<EntryCard, "rating">, mode: LabMode): boolean {
  return entry.rating === ratingForMode(mode);
}

/** 左栏图包树的查询（§9.4）：本地 nai5_community_pack、decision=include、按模式 rating，书序分页。 */
export function packQuery(mode: LabMode, offset: number): EntryQuery {
  return { codex_id: PACK_CODEX_ID, decision: ["include"], rating: [ratingForMode(mode)], sort: "book_order", offset, limit: LAB_PAGE_SIZE };
}

// ---- 图包树 ----

export type PackEntry = Pick<EntryCard, "entry_id" | "title" | "path" | "rating" | "book_order" | "thumb_path" | "tags_first_line">;
export interface PackNode {
  name: string;
  path: string[];        // 到本节点的完整前缀；虚拟根 UNFILED_NAME 为 []
  count: number;         // 子树条目数（含本节点直挂的）
  children: PackNode[];
  entries: PackEntry[];  // 直挂在本目录（path 与本节点相等）的条目，按 book_order
}

/** 目录路径 → Set / Map 的键；用 JSON 而不是 "/"，段名里的斜杠不会撞。 */
export function pathKey(path: readonly string[]): string {
  return JSON.stringify(path);
}

/** 按 path 段分组成树，节点按首次出现（书序）排列，count 逐层累加；path 为空的条目挂到 UNFILED_NAME 虚拟根。 */
export function buildPackTree(entries: readonly PackEntry[]): PackNode[] {
  const roots: PackNode[] = [];
  const sorted = [...entries].sort((a, b) => a.book_order - b.book_order || a.entry_id.localeCompare(b.entry_id));
  let unfiled: PackNode | null = null;
  for (const e of sorted) {
    if (!e.path.length) {
      if (!unfiled) {
        unfiled = { name: UNFILED_NAME, path: [], count: 0, children: [], entries: [] };
        roots.push(unfiled);
      }
      unfiled.count += 1;
      unfiled.entries.push(e);
      continue;
    }
    let level = roots;
    let node: PackNode | null = null;
    const prefix: string[] = [];
    for (const seg of e.path) {
      prefix.push(seg);
      let next = level.find((n) => n.name === seg && n.path.length === prefix.length);
      if (!next) {
        next = { name: seg, path: [...prefix], count: 0, children: [], entries: [] };
        level.push(next);
      }
      next.count += 1;
      node = next;
      level = next.children;
    }
    node!.entries.push(e);
  }
  return roots;
}

/** 树里全部条目数（各根 count 之和）。 */
export function treeTotal(nodes: readonly PackNode[]): number {
  return nodes.reduce((s, n) => s + n.count, 0);
}

/**
 * 勾选集合 → `LabRunRequest.scope`（最小前缀列表）：勾了的目录整棵纳入，不再下探；没勾的下探找勾了的子目录。
 * 虚拟根（path 为空）不能表达为前缀，跳过。顺序按树序。
 */
export function scopeFromChecked(nodes: readonly PackNode[], checked: ReadonlySet<string>): string[][] {
  const out: string[][] = [];
  const walk = (list: readonly PackNode[]) => {
    for (const n of list) {
      if (n.path.length && checked.has(pathKey(n.path))) {
        out.push([...n.path]);
        continue;
      }
      walk(n.children);
    }
  };
  walk(nodes);
  return out;
}

/** 勾选集合覆盖的条目数（scopeFromChecked 的前缀互不包含，直接加 count）。 */
export function countInScope(nodes: readonly PackNode[], checked: ReadonlySet<string>): number {
  let total = 0;
  const walk = (list: readonly PackNode[]) => {
    for (const n of list) {
      if (n.path.length && checked.has(pathKey(n.path))) {
        total += n.count;
        continue;
      }
      walk(n.children);
    }
  };
  walk(nodes);
  return total;
}

/** 本节点没勾但子孙里有勾的（复选框显示半选）。 */
export function hasCheckedDescendant(node: PackNode, checked: ReadonlySet<string>): boolean {
  return node.children.some((c) => checked.has(pathKey(c.path)) || hasCheckedDescendant(c, checked));
}

/** 勾选是否被某个已勾的祖先覆盖（祖先勾了，子目录的勾选无意义，界面上置灰）。 */
export function coveredByAncestor(path: readonly string[], checked: ReadonlySet<string>): boolean {
  for (let i = 1; i < path.length; i += 1) {
    if (checked.has(pathKey(path.slice(0, i)))) return true;
  }
  return false;
}

/** 关键词筛条目：标题 / tags 首行 / 目录名，大小写不敏感；空关键词全部保留。 */
export function filterPackEntries<T extends PackEntry>(entries: readonly T[], keyword: string): T[] {
  const k = keyword.trim().toLowerCase();
  if (!k) return [...entries];
  return entries.filter((e) => `${e.title}\n${e.tags_first_line}\n${e.path.join("/")}`.toLowerCase().includes(k));
}

/** 在树里找一条（点开历史回看时高亮当前 pack_entry_id 用）。 */
export function findPackEntry(nodes: readonly PackNode[], entryId: string): PackEntry | null {
  for (const n of nodes) {
    const hit = n.entries.find((e) => e.entry_id === entryId);
    if (hit) return hit;
    const deep = findPackEntry(n.children, entryId);
    if (deep) return deep;
  }
  return null;
}

// ---- 清洗 diff ----

const WRAPPER_RE = /<\/?[a-z_]+>/gi;
const WEIGHT_RE = /^-?\d+(?:\.\d+)?::|::$/g;

/** 按逗号 / 全角逗号 / 换行切 token，去空。 */
export function splitTags(text: string): string[] {
  return text.replace(WRAPPER_RE, "").split(/[,，\n]/).map((s) => s.trim()).filter(Boolean);
}

/** 与插件 lab_run._norm 同款归一：小写、`_`→空格、剥 `1.2::…::` 权重与 `{}[]` 壳、压空白。 */
export function normTag(tok: string): string {
  let s = tok.replace(WRAPPER_RE, "").trim().toLowerCase().replace(/_/g, " ");
  s = s.replace(WEIGHT_RE, "");
  s = s.replace(/^[{[\]}: ]+|[{[\]}: ]+$/g, "");
  return s.replace(/\s+/g, " ").trim();
}

/**
 * tags_raw 与 tags_used 的差：dropped = 原文里有、清洗后没有的 token（原样文本），kept = 两边都有的。
 * 按归一后的文本做多重集比较，重复 token 各算各的。
 */
export function diffTokens(tags_raw: string, tags_used: string): { dropped: string[]; kept: string[] } {
  const used = new Map<string, number>();
  for (const t of splitTags(tags_used)) {
    const k = normTag(t);
    used.set(k, (used.get(k) ?? 0) + 1);
  }
  const dropped: string[] = [];
  const kept: string[] = [];
  for (const t of splitTags(tags_raw)) {
    const k = normTag(t);
    const n = used.get(k) ?? 0;
    if (n > 0) {
      used.set(k, n - 1);
      kept.push(t);
    } else {
      dropped.push(t);
    }
  }
  return { dropped, kept };
}

// ---- text 拼装 ----

/**
 * 角色 + 描述（+ 风格=）→ `LabRunRequest.text`，与 /画 后面敲的那串同形：首 token 角色（空 → 随机），
 * 风格= 紧随其后（值内空白折成 `_`，插件按 `\S+` 取），描述折成一行放最后。
 */
export function textFromInputs(character: string, desc: string, style?: string): string {
  const parts = [character.trim() || RANDOM_CHARACTER];
  const st = (style ?? "").trim();
  if (st) parts.push(`风格=${st.replace(/\s+/g, "_")}`);
  const d = desc.replace(/\s+/g, " ").trim();
  if (d) parts.push(d);
  return parts.join(" ");
}

// ---- 默认请求 / 表单 ----

/** §9.1 各字段的默认值：角色 薇拉、随机图包、默认范围、筛选、画风池 50%、1 张、评分开、随机种子。 */
export function defaultRequest(mode: LabMode): LabRunRequest {
  return { mode, text: DEFAULT_CHARACTER, entry_id: null, scope: [], artist_ratio: 0.5, count: 1, score: true, seed: null };
}

export function isTerminal(status: LabRunStatus | string | null | undefined): boolean {
  return status === "done" || status === "error";
}

/** 中栏表单的持久化形状（每模式一份，键 `codexlab.lab.<mode>`）。artist_ratio 是 0–100 整数，seed 是输入框原文。 */
export interface LabForm {
  character: string;
  desc: string;
  style: string;
  random: boolean;             // true = 随机图包（entry_id 不发）
  entry_id: string | null;     // 点中的单条
  scope: string[][];           // 勾选目录求出的最小前缀
  artist_ratio: number;        // 0..100
  count: number;               // 1..8
  score: boolean;
  seed: string;                // "" = 随机
  last_run_id: string | null;  // 上次出图的 run，回到页面时自动回看
}

export function labStorageKey(mode: LabMode): string {
  return `codexlab.lab.${mode}`;
}

export function defaultForm(mode: LabMode): LabForm {
  const r = defaultRequest(mode);
  return {
    character: DEFAULT_CHARACTER, desc: "", style: "", random: true, entry_id: null, scope: [],
    artist_ratio: Math.round(r.artist_ratio * 100), count: r.count, score: r.score, seed: "", last_run_id: null,
  };
}

export function clampCount(n: unknown): number {
  const v = typeof n === "number" && Number.isFinite(n) ? Math.round(n) : LAB_MIN_COUNT;
  return Math.min(LAB_MAX_COUNT, Math.max(LAB_MIN_COUNT, v));
}
/** 0–100 整数。 */
export function clampRatio(n: unknown): number {
  const v = typeof n === "number" && Number.isFinite(n) ? Math.round(n) : 50;
  return Math.min(100, Math.max(0, v));
}

/** 种子输入：空 → null（随机）；0..2^32-1 的整数 → 数字；其余无效返回 undefined。 */
export function parseSeed(text: string): number | null | undefined {
  const s = text.trim();
  if (!s) return null;
  if (!/^\d{1,10}$/.test(s)) return undefined;
  const n = Number(s);
  return n <= LAB_MAX_SEED ? n : undefined;
}

function isStringArray(v: unknown): v is string[] {
  return Array.isArray(v) && v.every((x) => typeof x === "string");
}
function isScope(v: unknown): v is string[][] {
  return Array.isArray(v) && v.every(isStringArray);
}

/** localStorage 里可能损坏 / 过期的对象合并到默认值上，类型不对的键忽略。 */
export function normalizeForm(raw: unknown, mode: LabMode): LabForm {
  const out = defaultForm(mode);
  if (typeof raw !== "object" || raw === null) return out;
  const r = raw as Record<string, unknown>;
  if (typeof r.character === "string") out.character = r.character;
  if (typeof r.desc === "string") out.desc = r.desc;
  if (typeof r.style === "string") out.style = r.style;
  if (typeof r.random === "boolean") out.random = r.random;
  if (typeof r.entry_id === "string" || r.entry_id === null) out.entry_id = r.entry_id as string | null;
  if (isScope(r.scope)) out.scope = r.scope.filter((p) => p.length > 0).map((p) => [...p]);
  if (typeof r.artist_ratio === "number") out.artist_ratio = clampRatio(r.artist_ratio);
  if (typeof r.count === "number") out.count = clampCount(r.count);
  if (typeof r.score === "boolean") out.score = r.score;
  if (typeof r.seed === "string") out.seed = parseSeed(r.seed) === undefined ? "" : r.seed.trim();
  if (typeof r.last_run_id === "string" || r.last_run_id === null) out.last_run_id = r.last_run_id as string | null;
  if (!out.random && !out.entry_id) out.random = true;
  return out;
}

export function loadForm(mode: LabMode): LabForm {
  try {
    const text = localStorage.getItem(labStorageKey(mode));
    return normalizeForm(text ? JSON.parse(text) : null, mode);
  } catch {
    return defaultForm(mode);
  }
}

export function saveForm(mode: LabMode, form: LabForm): void {
  try {
    localStorage.setItem(labStorageKey(mode), JSON.stringify(form));
  } catch {
    /* 存不了就算了 */
  }
}

/** 表单 → 请求。随机时不发 entry_id；artist_ratio 换算到 0..1；seed 无效按随机。 */
export function requestFromForm(mode: LabMode, form: LabForm): LabRunRequest {
  const seed = parseSeed(form.seed);
  return {
    mode,
    text: textFromInputs(form.character, form.desc, form.style),
    entry_id: form.random ? null : form.entry_id,
    scope: form.scope.filter((p) => p.length > 0).map((p) => [...p]),
    artist_ratio: clampRatio(form.artist_ratio) / 100,
    count: clampCount(form.count),
    score: form.score,
    seed: seed === undefined ? null : seed,
  };
}

// ---- 角色候选（datalist）----

/** 输入框的候选：默认角色、随机、最近用过的（去重、保序）。 */
export function charOptions(history: readonly string[]): string[] {
  const out: string[] = [];
  for (const c of [DEFAULT_CHARACTER, RANDOM_CHARACTER, ...history]) {
    const t = c.trim();
    if (t && !out.includes(t)) out.push(t);
  }
  return out;
}

/** 出图后把角色记进最近列表：默认与随机不记，去重后放最前，最多 CHAR_HISTORY_MAX 条。 */
export function pushCharHistory(history: readonly string[], name: string): string[] {
  const t = name.trim();
  if (!t || t === DEFAULT_CHARACTER || t === RANDOM_CHARACTER) return [...history];
  return [t, ...history.filter((h) => h !== t)].slice(0, CHAR_HISTORY_MAX);
}

export function loadCharHistory(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(CHAR_HISTORY_KEY) ?? "[]");
    return isStringArray(raw) ? raw.slice(0, CHAR_HISTORY_MAX) : [];
  } catch {
    return [];
  }
}

export function saveCharHistory(history: readonly string[]): void {
  try {
    localStorage.setItem(CHAR_HISTORY_KEY, JSON.stringify(history));
  } catch {
    /* 存不了就算了 */
  }
}

// ---- 运行记录 ----

/** 出图刚提交、还没轮询到状态时的占位：plans → 全部 pending 的 LabImage。 */
export function imagesFromPlans(plans: readonly LabPlan[]): LabImage[] {
  return plans.map((p) => ({ ...p, status: "pending", file: null, error: null, score: null, notes: [], rating: null }));
}

/** `lab_run_start` 返回后立刻能显示的 LabRun（queued），下一次 lab_run_status 会整份覆盖。 */
export function runFromStart(req: LabRunRequest, run_id: string, plans: readonly LabPlan[], now: string): LabRun {
  return {
    run_id, mode: req.mode, request: req, status: "queued", created_at: now, finished_at: null,
    stamina_before: null, stamina_after: null, images: imagesFromPlans(plans), error: null, plugin_version: "",
  };
}

/** 改一张图的评分，返回新对象（乐观更新 / 回滚用）；idx 找不到原样返回。 */
export function withRating(run: LabRun, idx: number, rating: LabRating | null): LabRun {
  const i = run.images.findIndex((im) => im.idx === idx);
  if (i < 0) return run;
  const images = run.images.slice();
  images[i] = { ...images[i], rating };
  return { ...run, images };
}

/** 收藏 / 点踩按钮：再点一次同一个 = 取消。 */
export function nextRating(current: LabRating | null, clicked: LabRating): LabRating | null {
  return current === clicked ? null : clicked;
}

/** 运行进度：完成 / 失败 / 总数（头部一行用）。 */
export function runProgress(run: LabRun): { done: number; failed: number; total: number } {
  let done = 0;
  let failed = 0;
  for (const im of run.images) {
    if (im.status === "done") done += 1;
    else if (im.status === "error") failed += 1;
  }
  return { done, failed, total: run.images.length || run.request.count };
}

// ---- 展示 ----

/** 后端 lab_run_image 给的是相对数据目录的路径（`labruns/<run_id>/<idx>.png`，无前导斜杠），拼成绝对路径才能 convertFileSrc。以 `/`（或盘符）开头视为已是绝对路径，原样返回。 */
export function labImagePath(dataDir: string, rel: string): string {
  if (/^(?:[a-zA-Z]:)?[\\/]/.test(rel)) return rel;
  return `${dataDir.replace(/[\\/]+$/, "")}/${rel}`;
}

export function fmtScore(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "—";
  return Number.isInteger(v) ? String(v) : v.toFixed(1);
}
