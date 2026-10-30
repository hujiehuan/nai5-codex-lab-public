// 完整池推送结果与"插件当前持有"面板的纯展示函数（CONTRACT §8）。
import type { FullCounts } from "../types";
import { DECISION_LABEL } from "./decision";

/** 已知 codex_id 的中文名，与侧栏 / 题库下拉一致；未知 id 原样返回。 */
export const CODEX_LABEL: Record<string, string> = {
  artist_nai5_personal: "画师词典",
  nax_v5_artist: "NAX · V5 画师测试库",
  nai5_community_pack: "精选图包",
  suozhang_nai5: "所长法典 · N5 常规",
  local_artist: "本地画师",
  local_pack: "本地图包",
};
export function codexLabel(codexId: string): string {
  return CODEX_LABEL[codexId] ?? codexId;
}

/** codex 展示顺序：画师来源在前、图包随后，本地两本与未知来源列在后面。 */
const CODEX_ORDER: readonly string[] = ["artist_nai5_personal", "nax_v5_artist", "nai5_community_pack", "local_artist", "local_pack"];
/** 筛选池 PoolStatus.counts 的键序与中文名（§8.2）。 */
const FILTERED_COUNT_ORDER: readonly string[] = ["nai5", "pack", "kept_old", "dropped_blacklist"];
export const POOL_COUNT_LABEL: Record<string, string> = {
  nai5: "nai5",
  pack: "图包",
  kept_old: "沿用旧池",
  dropped_blacklist: "黑名单剔除",
};

/** 去重后按 order 里的位置排，不在 order 里的按字母序垫底。 */
function orderKeys(keys: Iterable<string>, order: readonly string[]): string[] {
  const rank = (k: string) => {
    const i = order.indexOf(k);
    return i < 0 ? order.length : i;
  };
  return [...new Set(keys)].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

/** 按 §8.1 的固定顺序排 codex_id。 */
export function orderedCodexIds(ids: Iterable<string>): string[] {
  return orderKeys(ids, CODEX_ORDER);
}

export const EMPTY_FULL_COUNTS: Readonly<FullCounts> = Object.freeze({ total: 0, include: 0, exclude: 0, undecided: 0, blacklisted: 0 });

/** 各 codex 计数逐字段求和（合计行）。返回新对象，不动入参。 */
export function sumFullCounts(counts: Record<string, FullCounts>): FullCounts {
  const sum: FullCounts = { ...EMPTY_FULL_COUNTS };
  for (const c of Object.values(counts)) {
    sum.total += c.total;
    sum.include += c.include;
    sum.exclude += c.exclude;
    sum.undecided += c.undecided;
    sum.blacklisted += c.blacklisted;
  }
  return sum;
}

/** 一行摘要：`共 N · 加入 a · 不加入 b · 未定 c · 黑名单 d`。 */
export function fmtFullCounts(c: FullCounts): string {
  return `共 ${c.total} · ${DECISION_LABEL.include} ${c.include} · ${DECISION_LABEL.exclude} ${c.exclude} · ${DECISION_LABEL.undecided} ${c.undecided} · 黑名单 ${c.blacklisted}`;
}

/**
 * PoolStatus.counts 一行摘要。筛选池的键（nai5 / pack / kept_old / dropped_blacklist）按契约顺序译成中文；
 * 完整池的键是 codex_id，按 §8.1 顺序译成书名；其余键原样、字母序垫底。空对象给 "—"。
 */
export function fmtPoolCounts(counts: Record<string, number>): string {
  const keys = orderKeys(Object.keys(counts), [...FILTERED_COUNT_ORDER, ...CODEX_ORDER]);
  if (!keys.length) return "—";
  return keys.map((k) => `${POOL_COUNT_LABEL[k] ?? CODEX_LABEL[k] ?? k} ${counts[k]}`).join(" · ");
}
