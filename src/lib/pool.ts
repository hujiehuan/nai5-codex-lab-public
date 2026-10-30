// 「插件当前持有」面板与词典名的纯展示函数（CONTRACT §8）。

/** 已知 codex_id 的中文名，与侧栏 / 题库下拉一致；未知 id 原样返回。 */
export const CODEX_LABEL: Record<string, string> = {
  artist_nai5_personal: "风格词典",
  nai5_community_pack: "精选图包",
  local_artist: "本地风格配方",
  local_pack: "本地图包",
};
export function codexLabel(codexId: string): string {
  return CODEX_LABEL[codexId] ?? codexId;
}

/** codex 展示顺序：风格词典在前、图包随后，本地两本与未知来源列在后面。 */
const CODEX_ORDER: readonly string[] = ["artist_nai5_personal", "nai5_community_pack", "local_artist", "local_pack"];
/** 筛选池 PoolStatus.counts 的键序与中文名（§8.2）。 */
const FILTERED_COUNT_ORDER: readonly string[] = ["nai5", "pack", "kept_old", "dropped_blacklist"];
export const POOL_COUNT_LABEL: Record<string, string> = {
  nai5: "nai5",
  pack: "图包",
  kept_old: "沿用旧池",
  dropped_blacklist: "排除名单剔除",
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

/**
 * PoolStatus.counts 一行摘要。筛选池的键（nai5 / pack / kept_old / dropped_blacklist）按契约顺序译成中文；
 * 完整池的键是 codex_id，按 §8.1 顺序译成书名；其余键原样、字母序垫底。空对象给 "—"。
 */
export function fmtPoolCounts(counts: Record<string, number>): string {
  const keys = orderKeys(Object.keys(counts), [...FILTERED_COUNT_ORDER, ...CODEX_ORDER]);
  if (!keys.length) return "—";
  return keys.map((k) => `${POOL_COUNT_LABEL[k] ?? CODEX_LABEL[k] ?? k} ${counts[k]}`).join(" · ");
}
