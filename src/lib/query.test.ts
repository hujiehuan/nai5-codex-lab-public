import { describe, expect, it } from "vitest";
import {
  applyFilterPatch,
  buildEntryQuery,
  clampPage,
  defaultFilter,
  defaultSortDir,
  effectiveSortDir,
  filterStorageKey,
  isFilterActive,
  normalizeFilter,
  pageCount,
  pageForNewSize,
  pageOffset,
  type CodexFilter,
} from "./query";

describe("codex filter defaults", () => {
  it("has no rating dimension (the library only holds safe entries)", () => {
    expect("rating" in defaultFilter()).toBe(false);
  });

  it("uses the agreed storage key", () => {
    expect(filterStorageKey("artist_nai5_personal")).toBe("codexlab.filter.artist_nai5_personal");
  });

  it("starts on page 1, 100 per page, book order in its default direction", () => {
    const f = defaultFilter();
    expect(f.page).toBe(1);
    expect(f.page_size).toBe(100);
    expect(f.sort).toBe("book_order");
    expect(f.sort_dir).toBeUndefined();
  });
});

describe("normalizeFilter", () => {
  const defaults = defaultFilter();

  it("ignores garbage and unknown keys", () => {
    expect(normalizeFilter("nope", defaults)).toEqual(defaults);
    expect(normalizeFilter({ bogus: 1, keyword: 42 }, defaults)).toEqual(defaults);
  });

  it("keeps valid values and filters invalid enum members", () => {
    const f = normalizeFilter({
      decision: ["include", "maybe"],
      batch: "8.25",
      path_prefix: ["单画师词典", "作者乙"],
      keyword: "foo",
      multi_image_only: true,
      dup_only: "yes",
      sort: "title",
    }, defaults);
    expect(f.decision).toEqual(["include"]);
    expect(f.batch).toBe("8.25");
    expect(f.path_prefix).toEqual(["单画师词典", "作者乙"]);
    expect(f.keyword).toBe("foo");
    expect(f.multi_image_only).toBe(true);
    expect(f.dup_only).toBe(false);
    expect(f.sort).toBe("title");
  });

  it("accepts the v0.1.2 sort keys, sort_dir, page and page_size", () => {
    const f = normalizeFilter({ sort: "synced_at", sort_dir: "desc", page: 7, page_size: 50 }, defaults);
    expect(f.sort).toBe("synced_at");
    expect(f.sort_dir).toBe("desc");
    expect(f.page).toBe(7);
    expect(f.page_size).toBe(50);
    expect(normalizeFilter({ sort: "first_seen", sort_dir: "asc", page_size: 200 }, defaults)).toMatchObject({ sort: "first_seen", sort_dir: "asc", page_size: 200 });
  });

  it("falls back to defaults for illegal sort / sort_dir / page / page_size", () => {
    const f = normalizeFilter({ sort: "random", sort_dir: "sideways", page: 0, page_size: 75 }, defaults);
    expect(f.sort).toBe("book_order");
    expect(f.sort_dir).toBeUndefined();
    expect(f.page).toBe(1);
    expect(f.page_size).toBe(100);
    expect(normalizeFilter({ page: 2.5 }, defaults).page).toBe(1);
    expect(normalizeFilter({ page: "3" }, defaults).page).toBe(1);
    expect(normalizeFilter({ page: -4, page_size: "200" }, defaults)).toMatchObject({ page: 1, page_size: 100 });
  });

  it("drops a stale rating key from older stored filters", () => {
    expect("rating" in normalizeFilter({ rating: ["safe"] }, defaults)).toBe(false);
  });

  it("does not share array references with the defaults", () => {
    const f = normalizeFilter(null, defaults);
    f.decision.push("include");
    expect(defaults.decision).toEqual([]);
  });
});

describe("sort direction", () => {
  it("follows the CONTRACT §7 defaults per key", () => {
    expect(defaultSortDir("book_order")).toBe("asc");
    expect(defaultSortDir("title")).toBe("asc");
    expect(defaultSortDir("synced_at")).toBe("asc");
    expect(defaultSortDir("decided_at")).toBe("desc");
    expect(defaultSortDir("first_seen")).toBe("desc");
  });

  it("lets an explicit sort_dir override the default", () => {
    expect(effectiveSortDir({ sort: "decided_at", sort_dir: undefined })).toBe("desc");
    expect(effectiveSortDir({ sort: "decided_at", sort_dir: "asc" })).toBe("asc");
    expect(effectiveSortDir({ sort: "title", sort_dir: "desc" })).toBe("desc");
  });
});

describe("paging arithmetic", () => {
  it("derives offset from page and page_size", () => {
    expect(pageOffset({ page: 1, page_size: 100 })).toBe(0);
    expect(pageOffset({ page: 3, page_size: 50 })).toBe(100);
  });

  it("counts pages, never below one", () => {
    expect(pageCount(0, 100)).toBe(1);
    expect(pageCount(100, 100)).toBe(1);
    expect(pageCount(101, 100)).toBe(2);
    expect(pageCount(3216, 200)).toBe(17);
    expect(pageCount(10, 0)).toBe(1);
  });

  it("clamps an out-of-range page back to the last page", () => {
    expect(clampPage(5, 120, 100)).toBe(2);
    expect(clampPage(2, 120, 100)).toBe(2);
    expect(clampPage(0, 120, 100)).toBe(1);
    expect(clampPage(3, 0, 100)).toBe(1);
    expect(clampPage(NaN, 120, 100)).toBe(1);
    expect(clampPage(2.9, 500, 100)).toBe(2);
  });

  it("keeps the first visible item when page_size changes", () => {
    // 第 3 页 / 每页 100 → 首条序号 200 → 每页 50 是第 5 页，每页 200 是第 2 页
    expect(pageForNewSize(3, 100, 50)).toBe(5);
    expect(pageForNewSize(3, 100, 200)).toBe(2);
    expect(pageForNewSize(1, 100, 50)).toBe(1);
    // 第 2 页 / 每页 50 → 首条 50 → 每页 200 是第 1 页
    expect(pageForNewSize(2, 50, 200)).toBe(1);
    // 第 4 页 / 每页 200 → 首条 600 → 每页 100 是第 7 页
    expect(pageForNewSize(4, 200, 100)).toBe(7);
    expect(pageForNewSize(4, 200, 0)).toBe(1);
  });
});

describe("applyFilterPatch", () => {
  const base = { ...defaultFilter(), page: 4 };

  it("resets page to 1 when any filter or sort key actually changes", () => {
    const patches: Partial<CodexFilter>[] = [
      { keyword: "x" },
      { decision: ["include"] },
      { batch: "8.25" },
      { first_seen_release: "2026-09-01" },
      { path_prefix: ["a"] },
      { multi_image_only: true },
      { dup_only: true },
      { sort: "title" },
      { sort_dir: "desc" },
    ];
    for (const p of patches) expect(applyFilterPatch(base, p).page, JSON.stringify(p)).toBe(1);
  });

  it("keeps the page when a patch repeats the current value", () => {
    expect(applyFilterPatch(base, { keyword: "" }).page).toBe(4);
    expect(applyFilterPatch(base, { decision: [] }).page).toBe(4);
    expect(applyFilterPatch(base, { sort_dir: undefined }).page).toBe(4);
    expect(applyFilterPatch(base, {}).page).toBe(4);
  });

  it("accepts a bare page change as is", () => {
    expect(applyFilterPatch(base, { page: 9 }).page).toBe(9);
  });

  it("converts the page when page_size changes so the first item stays in view", () => {
    // 第 4 页 / 每页 100 → 首条 300 → 每页 50 是第 7 页，每页 200 是第 2 页
    expect(applyFilterPatch(base, { page_size: 50 })).toMatchObject({ page: 7, page_size: 50 });
    expect(applyFilterPatch(base, { page_size: 200 })).toMatchObject({ page: 2, page_size: 200 });
    expect(applyFilterPatch(base, { page_size: 100 }).page).toBe(4);
    // 同一个 patch 里筛选也变了：归 1 优先
    expect(applyFilterPatch(base, { page_size: 50, keyword: "x" }).page).toBe(1);
  });
});

describe("buildEntryQuery", () => {
  const defaults = defaultFilter();

  it("derives offset/limit from page and page_size and omits sort_dir when unset", () => {
    const q = buildEntryQuery("artist_nai5_personal", { ...defaults, page: 3, page_size: 100 });
    expect(q).toEqual({ codex_id: "artist_nai5_personal", offset: 200, limit: 100, sort: "book_order" });
    expect("sort_dir" in q).toBe(false);
  });

  it("sends sort and sort_dir when a direction is chosen", () => {
    const q = buildEntryQuery("x", { ...defaults, sort: "synced_at", sort_dir: "desc", page: 1, page_size: 50 });
    expect(q.sort).toBe("synced_at");
    expect(q.sort_dir).toBe("desc");
    expect(q.offset).toBe(0);
    expect(q.limit).toBe(50);
  });

  it("never sends rating", () => {
    expect("rating" in buildEntryQuery("x", defaults)).toBe(false);
  });

  it("passes every active filter through", () => {
    const q = buildEntryQuery("x", {
      ...defaults,
      decision: ["undecided"],
      batch: "8.25",
      first_seen_release: "2026-09-01",
      path_prefix: ["a", "b"],
      keyword: "  inkwell ",
      multi_image_only: true,
      dup_only: true,
    });
    expect(q.decision).toEqual(["undecided"]);
    expect(q.batch).toBe("8.25");
    expect(q.first_seen_release).toBe("2026-09-01");
    expect(q.path_prefix).toEqual(["a", "b"]);
    expect(q.keyword).toBe("inkwell");
    expect(q.multi_image_only).toBe(true);
    expect(q.dup_only).toBe(true);
  });

  it("reports whether anything but sort and paging differs from defaults", () => {
    expect(isFilterActive(defaults, defaults)).toBe(false);
    expect(isFilterActive({ ...defaults, sort: "title", sort_dir: "desc", page: 3, page_size: 50 }, defaults)).toBe(false);
    expect(isFilterActive({ ...defaults, keyword: "x" }, defaults)).toBe(true);
  });
});
