import { describe, expect, it } from "vitest";
import { EMPTY_FULL_COUNTS, codexLabel, fmtFullCounts, fmtPoolCounts, orderedCodexIds, sumFullCounts } from "./pool";

describe("pool helpers", () => {
  it("labels known codexes and passes unknown ids through", () => {
    expect(codexLabel("artist_nai5_personal")).toBe("画师词典");
    expect(codexLabel("nax_v5_artist")).toBe("NAX · V5 画师测试库");
    expect(codexLabel("nai5_community_pack")).toBe("精选图包");
    expect(codexLabel("local_pack")).toBe("本地图包");
    expect(codexLabel("something_else")).toBe("something_else");
  });
  it("orders codex ids artist first, pack second, locals next, unknown last alphabetically", () => {
    expect(orderedCodexIds(["zzz", "nai5_community_pack", "aaa", "nax_v5_artist", "artist_nai5_personal", "local_pack"])).toEqual([
      "artist_nai5_personal", "nax_v5_artist", "nai5_community_pack", "local_pack", "aaa", "zzz",
    ]);
    expect(orderedCodexIds(["nai5_community_pack", "nai5_community_pack"])).toEqual(["nai5_community_pack"]);
    expect(orderedCodexIds([])).toEqual([]);
  });
  it("sums full counts field by field", () => {
    const sum = sumFullCounts({
      artist_nai5_personal: { total: 10, include: 4, exclude: 3, undecided: 3, blacklisted: 1 },
      nai5_community_pack: { total: 5, include: 5, exclude: 0, undecided: 0, blacklisted: 0 },
    });
    expect(sum).toEqual({ total: 15, include: 9, exclude: 3, undecided: 3, blacklisted: 1 });
    expect(sumFullCounts({})).toEqual(EMPTY_FULL_COUNTS);
  });
  it("never mutates the shared empty counts", () => {
    const a = sumFullCounts({ x: { total: 1, include: 1, exclude: 0, undecided: 0, blacklisted: 0 } });
    expect(a.total).toBe(1);
    expect(EMPTY_FULL_COUNTS.total).toBe(0);
    expect(Object.isFrozen(EMPTY_FULL_COUNTS)).toBe(true);
  });
  it("formats the full-count summary line", () => {
    expect(fmtFullCounts({ total: 15, include: 9, exclude: 3, undecided: 3, blacklisted: 1 })).toBe("共 15 · 加入 9 · 不加入 3 · 未定 3 · 黑名单 1");
    expect(fmtFullCounts(EMPTY_FULL_COUNTS)).toBe("共 0 · 加入 0 · 不加入 0 · 未定 0 · 黑名单 0");
  });
  it("formats filtered pool counts in contract order with labels", () => {
    expect(fmtPoolCounts({ dropped_blacklist: 2, pack: 30, nai5: 120, kept_old: 4 })).toBe("nai5 120 · 图包 30 · 沿用旧池 4 · 黑名单剔除 2");
  });
  it("formats full pool counts by codex with book names", () => {
    expect(fmtPoolCounts({ nai5_community_pack: 1300, artist_nai5_personal: 1900 })).toBe("画师词典 1900 · 精选图包 1300");
  });
  it("keeps unknown count keys verbatim after known ones and handles empty", () => {
    expect(fmtPoolCounts({ zeta: 1, nai5: 2, alpha: 3 })).toBe("nai5 2 · alpha 3 · zeta 1");
    expect(fmtPoolCounts({})).toBe("—");
  });
});
