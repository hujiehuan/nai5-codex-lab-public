import { describe, expect, it } from "vitest";
import { isArtistBook } from "./artist-sources";
import { SUOZHANG_SOURCES, isSuozhangBook } from "./codex-sources";
import { codexLabel } from "./pool";

describe("所长法典来源", () => {
  it("只认所长 N5 常规法典，和画师页互不重叠", () => {
    expect(SUOZHANG_SOURCES.map((s) => s.id)).toEqual(["suozhang_nai5"]);
    expect(isSuozhangBook("suozhang_nai5")).toBe(true);
    expect(isSuozhangBook("nai5_community_pack")).toBe(false);
    expect(isSuozhangBook(undefined)).toBe(false);
    for (const s of SUOZHANG_SOURCES) expect(isArtistBook(s.id)).toBe(false);
  });
  it("书名与侧栏一致", () => {
    expect(codexLabel("suozhang_nai5")).toBe("所长法典 · N5 常规");
  });
});
