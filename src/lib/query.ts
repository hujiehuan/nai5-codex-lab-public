// 词典页筛选状态：默认值、localStorage 序列化校验、EntryQuery 构造、分页换算。纯函数。
import type { Decision, EntryQuery, EntrySort } from "../types";

export const ALL_DECISIONS: Decision[] = ["include", "exclude", "undecided"];
export type SortKey = EntrySort;
export type SortDir = NonNullable<EntryQuery["sort_dir"]>;
export const ALL_SORTS: SortKey[] = ["book_order", "title", "decided_at", "synced_at", "first_seen"];
export const SORT_LABEL: Record<SortKey, string> = {
  book_order: "书序",
  title: "标题",
  decided_at: "决策时间",
  synced_at: "同步时间",
  first_seen: "首次出现",
};
export const PAGE_SIZES = [50, 100, 200] as const;
export type PageSize = (typeof PAGE_SIZES)[number];
export const DEFAULT_PAGE_SIZE: PageSize = 100;

export interface CodexFilter {
  decision: Decision[];          // 空 = 全部
  batch: string | null;
  first_seen_release: string | null;
  path_prefix: string[];
  keyword: string;
  multi_image_only: boolean;
  dup_only: boolean;
  sort: SortKey;
  sort_dir: SortDir | undefined; // undefined = 按 CONTRACT §7 的缺省方向，且不发送
  page: number;                  // 从 1 起
  page_size: PageSize;
}

/** 这些键任一变化都意味着列表换了一套，page 归 1。 */
const PAGE_RESET_KEYS = [
  "decision", "batch", "first_seen_release", "path_prefix", "keyword", "multi_image_only", "dup_only", "sort", "sort_dir",
] as const satisfies readonly (keyof CodexFilter)[];

export function filterStorageKey(codexId: string): string {
  return `codexlab.filter.${codexId}`;
}

export function defaultFilter(): CodexFilter {
  return {
    decision: [],
    batch: null,
    first_seen_release: null,
    path_prefix: [],
    keyword: "",
    multi_image_only: false,
    dup_only: false,
    sort: "book_order",
    sort_dir: undefined,
    page: 1,
    page_size: DEFAULT_PAGE_SIZE,
  };
}

function isStringArray(v: unknown): v is string[] {
  return Array.isArray(v) && v.every((x) => typeof x === "string");
}
function isSortKey(v: unknown): v is SortKey {
  return typeof v === "string" && (ALL_SORTS as string[]).includes(v);
}
export function isPageSize(v: unknown): v is PageSize {
  return typeof v === "number" && (PAGE_SIZES as readonly number[]).includes(v);
}

/** 把 localStorage 里可能损坏/过期的对象合并到默认值上，未知或类型不对的键忽略。 */
export function normalizeFilter(raw: unknown, defaults: CodexFilter): CodexFilter {
  const out: CodexFilter = { ...defaults, path_prefix: [...defaults.path_prefix], decision: [...defaults.decision] };
  if (typeof raw !== "object" || raw === null) return out;
  const r = raw as Record<string, unknown>;
  if (isStringArray(r.decision)) out.decision = r.decision.filter((x): x is Decision => ALL_DECISIONS.includes(x as Decision));
  if (typeof r.batch === "string" || r.batch === null) out.batch = r.batch as string | null;
  if (typeof r.first_seen_release === "string" || r.first_seen_release === null) out.first_seen_release = r.first_seen_release as string | null;
  if (isStringArray(r.path_prefix)) out.path_prefix = r.path_prefix;
  if (typeof r.keyword === "string") out.keyword = r.keyword;
  if (typeof r.multi_image_only === "boolean") out.multi_image_only = r.multi_image_only;
  if (typeof r.dup_only === "boolean") out.dup_only = r.dup_only;
  if (isSortKey(r.sort)) out.sort = r.sort;
  if (r.sort_dir === "asc" || r.sort_dir === "desc") out.sort_dir = r.sort_dir;
  if (typeof r.page === "number" && Number.isInteger(r.page) && r.page >= 1) out.page = r.page;
  if (isPageSize(r.page_size)) out.page_size = r.page_size;
  return out;
}

export function loadFilter(codexId: string, defaults: CodexFilter): CodexFilter {
  try {
    const text = localStorage.getItem(filterStorageKey(codexId));
    if (!text) return normalizeFilter(null, defaults);
    return normalizeFilter(JSON.parse(text), defaults);
  } catch {
    return normalizeFilter(null, defaults);
  }
}

export function saveFilter(codexId: string, filter: CodexFilter): void {
  try {
    localStorage.setItem(filterStorageKey(codexId), JSON.stringify(filter));
  } catch {
    /* 存不了就算了 */
  }
}

// ---- 排序方向 ----

/** CONTRACT §7 缺省方向：book_order / title / synced_at 升序，decided_at / first_seen 降序。 */
export function defaultSortDir(sort: SortKey): SortDir {
  return sort === "decided_at" || sort === "first_seen" ? "desc" : "asc";
}

/** 当前生效的方向（显式 sort_dir 优先，否则缺省）。 */
export function effectiveSortDir(f: Pick<CodexFilter, "sort" | "sort_dir">): SortDir {
  return f.sort_dir ?? defaultSortDir(f.sort);
}

// ---- 分页换算 ----

export function pageOffset(f: Pick<CodexFilter, "page" | "page_size">): number {
  return (f.page - 1) * f.page_size;
}

/** 总页数，至少 1（total=0 也是"第 1/1 页"）。 */
export function pageCount(total: number, pageSize: number): number {
  if (!(pageSize > 0) || !(total > 0)) return 1;
  return Math.max(1, Math.ceil(total / pageSize));
}

/** 把页码钳到 [1, pageCount]：越界（total 变小）就回最后一页。 */
export function clampPage(page: number, total: number, pageSize: number): number {
  const last = pageCount(total, pageSize);
  if (!Number.isFinite(page)) return 1;
  return Math.min(Math.max(1, Math.floor(page)), last);
}

/** 换每页条数时按当前首条的序号换算新页码，位置不丢：page = floor(oldOffset / newSize) + 1。 */
export function pageForNewSize(oldPage: number, oldSize: number, newSize: number): number {
  if (!(newSize > 0)) return 1;
  const oldOffset = Math.max(0, (Math.max(1, oldPage) - 1) * Math.max(0, oldSize));
  return Math.floor(oldOffset / newSize) + 1;
}

function sameValue(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * 合并一个 patch 并处理页码联动：
 * - 筛选 / 排序键（PAGE_RESET_KEYS）任一**实际变化** → page 归 1；
 * - 只改 page_size → 按当前首条序号换算新页码；
 * - 只改 page → 原样接受（钳制交给 store，那里知道 total）。
 */
export function applyFilterPatch(f: CodexFilter, patch: Partial<CodexFilter>): CodexFilter {
  const next: CodexFilter = { ...f, ...patch };
  if (patch.page_size !== undefined && patch.page_size !== f.page_size && patch.page === undefined) {
    next.page = pageForNewSize(f.page, f.page_size, patch.page_size);
  }
  if (PAGE_RESET_KEYS.some((k) => k in patch && !sameValue(patch[k], f[k]))) next.page = 1;
  return next;
}

/**
 * 构造 list_entries 的 query。不发 rating（库里只有 safe）；
 * 空值不发送；offset/limit 由 page/page_size 推出；sort_dir 为 undefined 时不发送（后端按 §7 缺省）。
 */
export function buildEntryQuery(codexId: string, f: CodexFilter): EntryQuery {
  const q: EntryQuery = { codex_id: codexId, offset: pageOffset(f), limit: f.page_size, sort: f.sort };
  if (f.sort_dir) q.sort_dir = f.sort_dir;
  if (f.decision.length) q.decision = [...f.decision];
  if (f.batch) q.batch = f.batch;
  if (f.first_seen_release) q.first_seen_release = f.first_seen_release;
  if (f.path_prefix.length) q.path_prefix = [...f.path_prefix];
  const kw = f.keyword.trim();
  if (kw) q.keyword = kw;
  if (f.multi_image_only) q.multi_image_only = true;
  if (f.dup_only) q.dup_only = true;
  return q;
}

/** 只看筛选条件本身：排序与分页不算"筛选生效"。 */
export function isFilterActive(f: CodexFilter, defaults: CodexFilter): boolean {
  const strip = (x: CodexFilter) => JSON.stringify({ ...x, sort: undefined, sort_dir: undefined, page: undefined, page_size: undefined });
  return strip(f) !== strip(defaults);
}
