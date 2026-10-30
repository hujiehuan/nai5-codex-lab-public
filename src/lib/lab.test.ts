// lab.ts 纯函数：图包树 / scope / diff / text / 默认请求 / 表单持久化 / 运行小工具。node 环境没有 localStorage，用 Map 顶上。
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LabPlan, LabRun } from "../types";
import {
  CHAR_HISTORY_KEY, DEFAULT_CHARACTER, LAB_MAX_SEED, UNFILED_NAME,
  buildPackTree, charOptions, clampCount, clampRatio, countInScope, coveredByAncestor, defaultForm, defaultRequest, diffTokens,
  filterPackEntries, findPackEntry, fmtScore, hasCheckedDescendant, imagesFromPlans, isTerminal, labImagePath,
  labStorageKey, loadCharHistory, loadForm, nextRating, normTag, normalizeForm, packQuery, parseSeed, pathKey, pushCharHistory,
  ratingForMode, requestFromForm, runFromStart, runProgress, saveCharHistory, saveForm, scopeFromChecked, splitTags, textFromInputs,
  treeTotal, visibleForMode, withRating, type PackEntry,
} from "./lab";

function fakeStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => { map.set(k, String(v)); },
    removeItem: (k: string) => { map.delete(k); },
    clear: () => map.clear(),
    get length() { return map.size; },
    key: (i: number) => [...map.keys()][i] ?? null,
    _map: map,
  };
}
afterEach(() => vi.unstubAllGlobals());

function entry(id: string, path: string[], order: number, rating: PackEntry["rating"] = "safe"): PackEntry {
  return { entry_id: id, title: `t-${id}`, path, rating, book_order: order, thumb_path: null, tags_first_line: `tags of ${id}` };
}
const ENTRIES: PackEntry[] = [
  entry("a", ["图包", "示例甲"], 3),
  entry("b", ["图包", "示例甲"], 1),
  entry("c", ["图包", "日常"], 2),
  entry("d", ["图包", "日常", "校园"], 5),
  entry("e", ["其他"], 4),
  entry("f", [], 6),
];

function plan(idx: number, extra: Partial<LabPlan> = {}): LabPlan {
  return {
    idx, character: "薇拉", pack_entry_id: "a", pack_title: "t-a", pack_path: ["图包", "示例甲"],
    tags_raw: "1girl, long hair, artist:x", tags_used: "1girl, long hair", dropped: ["artist:x"], extra_tags: null,
    prompt: "薇拉, 1girl, long hair", negative: "lowres", artist: "", artist_source: "none", size: "832x1216", seed: 7, model: "nai-diffusion-4-5-full",
    ...extra,
  };
}

describe("mode / query", () => {
  it("only ever shows safe packs", () => {
    expect(ratingForMode("draw")).toBe("safe");
    expect(visibleForMode({ rating: "safe" }, "draw")).toBe(true);
    expect(visibleForMode({ rating: "unknown" as PackEntry["rating"] }, "draw")).toBe(false);
  });
  it("builds the pack query: include only, safe, book order, paged", () => {
    expect(packQuery("draw", 400)).toEqual({
      codex_id: "nai5_community_pack", decision: ["include"], rating: ["safe"], sort: "book_order", offset: 400, limit: 200,
    });
  });
});

describe("buildPackTree", () => {
  it("groups by path segments with cumulative counts, book order inside, unfiled root for empty paths", () => {
    const tree = buildPackTree(ENTRIES);
    expect(tree.map((n) => n.name)).toEqual(["图包", "其他", UNFILED_NAME]);
    const pack = tree[0];
    expect(pack.count).toBe(4);
    expect(pack.path).toEqual(["图包"]);
    expect(pack.entries).toEqual([]);
    expect(pack.children.map((c) => [c.name, c.count])).toEqual([["示例甲", 2], ["日常", 2]]);
    expect(pack.children[0].entries.map((e) => e.entry_id)).toEqual(["b", "a"]);
    const daily = pack.children[1];
    expect(daily.entries.map((e) => e.entry_id)).toEqual(["c"]);
    expect(daily.children[0].path).toEqual(["图包", "日常", "校园"]);
    expect(daily.children[0].entries[0].entry_id).toBe("d");
    expect(tree[1].entries[0].entry_id).toBe("e");
    expect(tree[2].path).toEqual([]);
    expect(tree[2].entries[0].entry_id).toBe("f");
    expect(treeTotal(tree)).toBe(6);
  });
  it("does not mutate the input and handles empty", () => {
    const copy = ENTRIES.map((e) => ({ ...e, path: [...e.path] }));
    buildPackTree(copy);
    expect(copy).toEqual(ENTRIES);
    expect(buildPackTree([])).toEqual([]);
  });
  it("keeps same-named segments at different depths apart", () => {
    const tree = buildPackTree([entry("x", ["A", "A"], 0), entry("y", ["A"], 1)]);
    expect(tree).toHaveLength(1);
    expect(tree[0].count).toBe(2);
    expect(tree[0].entries.map((e) => e.entry_id)).toEqual(["y"]);
    expect(tree[0].children[0].path).toEqual(["A", "A"]);
    expect(tree[0].children[0].entries.map((e) => e.entry_id)).toEqual(["x"]);
  });
  it("finds an entry anywhere in the tree", () => {
    const tree = buildPackTree(ENTRIES);
    expect(findPackEntry(tree, "d")?.title).toBe("t-d");
    expect(findPackEntry(tree, "f")?.title).toBe("t-f");
    expect(findPackEntry(tree, "zzz")).toBeNull();
  });
});

describe("scopeFromChecked", () => {
  const tree = buildPackTree(ENTRIES);
  it("emits minimal prefixes in tree order, checked parent swallows children", () => {
    const checked = new Set([pathKey(["图包", "日常"]), pathKey(["图包", "日常", "校园"]), pathKey(["其他"])]);
    expect(scopeFromChecked(tree, checked)).toEqual([["图包", "日常"], ["其他"]]);
    expect(countInScope(tree, checked)).toBe(3);
  });
  it("descends into unchecked parents and skips the unfiled root", () => {
    const checked = new Set([pathKey(["图包", "示例甲"]), pathKey([])]);
    expect(scopeFromChecked(tree, checked)).toEqual([["图包", "示例甲"]]);
    expect(scopeFromChecked(tree, new Set())).toEqual([]);
    expect(countInScope(tree, new Set())).toBe(0);
  });
  it("reports indeterminate parents and covered children", () => {
    const checked = new Set([pathKey(["图包", "日常", "校园"])]);
    expect(hasCheckedDescendant(tree[0], checked)).toBe(true);
    expect(hasCheckedDescendant(tree[1], checked)).toBe(false);
    expect(coveredByAncestor(["图包", "日常", "校园"], new Set([pathKey(["图包"])]))).toBe(true);
    expect(coveredByAncestor(["图包", "日常", "校园"], checked)).toBe(false);
    expect(coveredByAncestor(["图包"], new Set([pathKey(["图包"])]))).toBe(false);
  });
  it("pathKey survives slashes in segment names", () => {
    expect(pathKey(["a/b", "c"])).not.toBe(pathKey(["a", "b/c"]));
    expect(pathKey(["a", "b"])).toBe(pathKey(["a", "b"]));
  });
});

describe("filterPackEntries", () => {
  it("matches title / tags / path case-insensitively and keeps all on blank", () => {
    expect(filterPackEntries(ENTRIES, "T-D").map((e) => e.entry_id)).toEqual(["d"]);
    expect(filterPackEntries(ENTRIES, "tags of e").map((e) => e.entry_id)).toEqual(["e"]);
    expect(filterPackEntries(ENTRIES, "校园").map((e) => e.entry_id)).toEqual(["d"]);
    expect(filterPackEntries(ENTRIES, "  ")).toHaveLength(6);
    expect(filterPackEntries(ENTRIES, "nothing")).toEqual([]);
  });
});

describe("diffTokens", () => {
  it("splits on commas / fullwidth commas / newlines and strips wrappers", () => {
    expect(splitTags("a, b，c\n d ,, ")).toEqual(["a", "b", "c", "d"]);
    expect(splitTags("<artist>x</artist>, y")).toEqual(["x", "y"]);
  });
  it("normalizes like the plugin: case, underscores, weights, braces", () => {
    expect(normTag("Long_Hair")).toBe("long hair");
    expect(normTag("1.4::blue  sky::")).toBe("blue sky");
    expect(normTag("{{masterpiece}}")).toBe("masterpiece");
    expect(normTag("[bad]")).toBe("bad");
  });
  it("returns dropped (raw only) and kept (both) with original text, multiset-aware", () => {
    const r = diffTokens("1girl, Long_Hair, artist:foo, watermark, 1girl", "1girl, long hair");
    expect(r.kept).toEqual(["1girl", "Long_Hair"]);
    expect(r.dropped).toEqual(["artist:foo", "watermark", "1girl"]);
  });
  it("handles empty sides", () => {
    expect(diffTokens("", "a")).toEqual({ dropped: [], kept: [] });
    expect(diffTokens("a, b", "")).toEqual({ dropped: ["a", "b"], kept: [] });
  });
});

describe("textFromInputs", () => {
  it("puts the character first, style second, one-line description last", () => {
    expect(textFromInputs("薇拉", "")).toBe("薇拉");
    expect(textFromInputs(" 薇拉 ", "  在海边\n吹风  ")).toBe("薇拉 在海边 吹风");
    expect(textFromInputs("薇拉", "夜景", "多娜 多娜")).toBe("薇拉 风格=多娜_多娜 夜景");
    expect(textFromInputs("薇拉", "夜景", "  ")).toBe("薇拉 夜景");
  });
  it("falls back to 随机 when the character is blank", () => {
    expect(textFromInputs("", "海边")).toBe("随机 海边");
    expect(textFromInputs("   ", "")).toBe("随机");
  });
});

describe("defaultRequest / isTerminal", () => {
  it("gives the §9.1 defaults per mode", () => {
    expect(defaultRequest("draw")).toEqual({
      mode: "draw", text: DEFAULT_CHARACTER, entry_id: null, scope: [], artist_ratio: 0.5, count: 1, score: true, seed: null,
    });
  });
  it("terminal = done | error", () => {
    expect(isTerminal("done")).toBe(true);
    expect(isTerminal("error")).toBe(true);
    expect(isTerminal("queued")).toBe(false);
    expect(isTerminal("running")).toBe(false);
    expect(isTerminal(null)).toBe(false);
    expect(isTerminal(undefined)).toBe(false);
  });
});

describe("form", () => {
  it("clamps count and ratio, parses seeds", () => {
    expect(clampCount(0)).toBe(1);
    expect(clampCount(9)).toBe(8);
    expect(clampCount(3.6)).toBe(4);
    expect(clampCount("x")).toBe(1);
    expect(clampRatio(-5)).toBe(0);
    expect(clampRatio(150)).toBe(100);
    expect(clampRatio(NaN)).toBe(50);
    expect(parseSeed("")).toBeNull();
    expect(parseSeed("  42 ")).toBe(42);
    expect(parseSeed(String(LAB_MAX_SEED))).toBe(LAB_MAX_SEED);
    expect(parseSeed(String(LAB_MAX_SEED + 1))).toBeUndefined();
    expect(parseSeed("-1")).toBeUndefined();
    expect(parseSeed("abc")).toBeUndefined();
    expect(parseSeed("1.5")).toBeUndefined();
  });
  it("default form mirrors default request", () => {
    const f = defaultForm("draw");
    expect(f).toEqual({
      character: DEFAULT_CHARACTER, desc: "", style: "", random: true, entry_id: null, scope: [],
      artist_ratio: 50, count: 1, score: true, seed: "", last_run_id: null,
    });
  });
  it("normalizeForm merges valid keys, ignores garbage and stale keys, forces random-without-entry", () => {
    const raw = {
      character: "桃乃", desc: 1, style: "x", random: false, entry_id: "e1", scope: [["图包", "示例甲"], [], "bad"],
      sanitize: false, artist_ratio: 130, count: 99, score: false, seed: "abc", last_run_id: "r1",
    };
    const form = normalizeForm(raw, "draw");
    expect(form.character).toBe("桃乃");
    expect(form.desc).toBe("");
    expect(form.random).toBe(false);
    expect(form.entry_id).toBe("e1");
    expect(form.scope).toEqual([]);          // 混入非法项 → 整个键忽略
    expect("sanitize" in form).toBe(false);  // 旧版表单里退役的键不带进来
    expect(form.artist_ratio).toBe(100);
    expect(form.count).toBe(8);
    expect(form.score).toBe(false);
    expect(form.seed).toBe("");              // 无效种子清空
    expect(form.last_run_id).toBe("r1");
    expect(normalizeForm({ ...raw, scope: [["图包"], []] }, "draw").scope).toEqual([["图包"]]);
    expect(normalizeForm({ random: false, entry_id: null }, "draw").random).toBe(true);
    expect(normalizeForm(null, "draw")).toEqual(defaultForm("draw"));
    expect(normalizeForm("junk", "draw")).toEqual(defaultForm("draw"));
  });
  it("requestFromForm converts ratio / seed / random", () => {
    const f = { ...defaultForm("draw"), character: "薇拉", desc: "海边", random: false, entry_id: "e1", scope: [["图包"]], artist_ratio: 30, count: 3, seed: "123" };
    expect(requestFromForm("draw", f)).toEqual({
      mode: "draw", text: "薇拉 海边", entry_id: "e1", scope: [["图包"]], artist_ratio: 0.3, count: 3, score: true, seed: 123,
    });
    const rnd = requestFromForm("draw", { ...f, random: true, seed: "oops" });
    expect(rnd.entry_id).toBeNull();
    expect(rnd.seed).toBeNull();
    expect(rnd.scope).not.toBe(f.scope);
  });
  it("persists per mode under codexlab.lab.<mode> and survives a broken storage", () => {
    expect(labStorageKey("draw")).toBe("codexlab.lab.draw");
    const storage = fakeStorage({ "codexlab.lab.draw": JSON.stringify({ character: "桃乃", count: 2 }) });
    vi.stubGlobal("localStorage", storage);
    expect(loadForm("draw").character).toBe("桃乃");
    expect(loadForm("draw").count).toBe(2);
    storage._map.delete("codexlab.lab.draw");
    expect(loadForm("draw")).toEqual(defaultForm("draw"));
    saveForm("draw", { ...defaultForm("draw"), count: 4 });
    expect(JSON.parse(storage._map.get("codexlab.lab.draw")!).count).toBe(4);
    storage._map.set("codexlab.lab.draw", "{not json");
    expect(loadForm("draw")).toEqual(defaultForm("draw"));
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("denied"); }, setItem: () => { throw new Error("denied"); } });
    expect(loadForm("draw")).toEqual(defaultForm("draw"));
    expect(() => saveForm("draw", defaultForm("draw"))).not.toThrow();
  });
});

describe("character history", () => {
  it("offers default + random + history, deduped", () => {
    expect(charOptions(["桃乃", "薇拉", " ", "桃乃"])).toEqual(["薇拉", "随机", "桃乃"]);
  });
  it("pushes to the front, skips default / random / blank, caps length", () => {
    expect(pushCharHistory(["a", "b"], "b")).toEqual(["b", "a"]);
    expect(pushCharHistory(["a"], "薇拉")).toEqual(["a"]);
    expect(pushCharHistory(["a"], "随机")).toEqual(["a"]);
    expect(pushCharHistory(["a"], "  ")).toEqual(["a"]);
    const many = Array.from({ length: 25 }, (_, i) => `c${i}`);
    expect(pushCharHistory(many, "new")).toHaveLength(20);
  });
  it("loads / saves through localStorage defensively", () => {
    const storage = fakeStorage({ [CHAR_HISTORY_KEY]: JSON.stringify(["x", 1]) });
    vi.stubGlobal("localStorage", storage);
    expect(loadCharHistory()).toEqual([]);
    saveCharHistory(["y"]);
    expect(loadCharHistory()).toEqual(["y"]);
    storage._map.set(CHAR_HISTORY_KEY, "nope");
    expect(loadCharHistory()).toEqual([]);
  });
});

describe("run helpers", () => {
  const req = defaultRequest("draw");
  it("builds pending images from plans and a queued run from start", () => {
    const imgs = imagesFromPlans([plan(0), plan(1, { seed: 8 })]);
    expect(imgs).toHaveLength(2);
    expect(imgs[1]).toMatchObject({ idx: 1, seed: 8, status: "pending", file: null, error: null, score: null, notes: [], rating: null });
    const run = runFromStart(req, "r1", [plan(0)], "2026-09-07T00:00:00Z");
    expect(run).toMatchObject({ run_id: "r1", mode: "draw", status: "queued", created_at: "2026-09-07T00:00:00Z", finished_at: null, error: null, plugin_version: "" });
    expect(run.request).toBe(req);
    expect(run.images[0].status).toBe("pending");
  });
  it("withRating replaces one image immutably; nextRating toggles", () => {
    const run: LabRun = runFromStart(req, "r1", [plan(0), plan(1)], "t");
    const next = withRating(run, 1, "fav");
    expect(next).not.toBe(run);
    expect(next.images[1].rating).toBe("fav");
    expect(next.images[0].rating).toBeNull();
    expect(run.images[1].rating).toBeNull();
    expect(withRating(run, 9, "dis")).toBe(run);
    expect(nextRating(null, "fav")).toBe("fav");
    expect(nextRating("fav", "fav")).toBeNull();
    expect(nextRating("fav", "dis")).toBe("dis");
  });
  it("counts progress, falling back to request.count before plans arrive", () => {
    const run: LabRun = runFromStart({ ...req, count: 3 }, "r1", [plan(0), plan(1), plan(2)], "t");
    run.images[0] = { ...run.images[0], status: "done" };
    run.images[1] = { ...run.images[1], status: "error", error: "boom" };
    expect(runProgress(run)).toEqual({ done: 1, failed: 1, total: 3 });
    expect(runProgress({ ...run, images: [] })).toEqual({ done: 0, failed: 0, total: 3 });
  });
});

describe("display helpers", () => {
  it("joins the relative image path onto the data dir, leaves absolute paths alone", () => {
    expect(labImagePath("/Users/x/Library/Application Support/moe.vivian.codexlab", "labruns/r1/0.thumb.png"))
      .toBe("/Users/x/Library/Application Support/moe.vivian.codexlab/labruns/r1/0.thumb.png");
    expect(labImagePath("/data/", "labruns/r1/0.png")).toBe("/data/labruns/r1/0.png");
    expect(labImagePath("/data", "/abs/already.png")).toBe("/abs/already.png");
    expect(labImagePath("/data", "C:\\x\\y.png")).toBe("C:\\x\\y.png");
  });
  it("formats scores", () => {
    expect(fmtScore(null)).toBe("—");
    expect(fmtScore(7)).toBe("7");
    expect(fmtScore(7.25)).toBe("7.3");
    expect(fmtScore(NaN)).toBe("—");
  });
});
