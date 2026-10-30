import { describe, expect, it } from "vitest";
import { codexLabel, fmtPoolCounts, orderedCodexIds } from "./pool";

describe("pool helpers", () => {
  it("labels known codexes and passes unknown ids through", () => {
    expect(codexLabel("artist_nai5_personal")).toBe("风格词典");
    expect(codexLabel("nai5_community_pack")).toBe("精选图包");
    expect(codexLabel("local_pack")).toBe("本地图包");
    expect(codexLabel("something_else")).toBe("something_else");
  });
  it("orders codex ids artist first, pack second, locals next, unknown last alphabetically", () => {
    expect(orderedCodexIds(["zzz", "nai5_community_pack", "aaa", "local_artist", "artist_nai5_personal", "local_pack"])).toEqual([
      "artist_nai5_personal", "nai5_community_pack", "local_artist", "local_pack", "aaa", "zzz",
    ]);
    expect(orderedCodexIds(["nai5_community_pack", "nai5_community_pack"])).toEqual(["nai5_community_pack"]);
    expect(orderedCodexIds([])).toEqual([]);
  });
  it("formats filtered pool counts in contract order with labels", () => {
    expect(fmtPoolCounts({ dropped_blacklist: 2, pack: 30, nai5: 120, kept_old: 4 })).toBe("nai5 120 · 图包 30 · 沿用旧池 4 · 排除名单剔除 2");
  });
  it("formats full pool counts by codex with book names", () => {
    expect(fmtPoolCounts({ nai5_community_pack: 1300, artist_nai5_personal: 1900 })).toBe("风格词典 1900 · 精选图包 1300");
  });
  it("keeps unknown count keys verbatim after known ones and handles empty", () => {
    expect(fmtPoolCounts({ zeta: 1, nai5: 2, alpha: 3 })).toBe("nai5 2 · alpha 3 · zeta 1");
    expect(fmtPoolCounts({})).toBe("—");
  });
});
