// reverse-summary.ts 纯函数：truth_kind 徽标 / 提示、可空平均值与计数、泄漏数、重画状态一格、重画表单 → 请求的重画键、评委下拉、替换结果行（CONTRACT §11.4）；
// v0.3.0（§12.6）：画师来源 / 张数归一与下拉、单条重画参数、尺寸来源与画师来源文字、对齐信息判定、最优候选与候选标签；
// v0.3.1（§12.7 精准反推）：style 子表单 → 六个 style_* 键、top_k / 最低相似度 / 兜底归一、结果表徽标与两列、单题面板的命中信息。
import { describe, expect, it } from "vitest";
import type { RenderCandidate, RunRow } from "../types";
import {
  ARTIST_SOURCE_LABEL, BATCH_ARTIST_MODES, DEFAULT_RENDER_FORM, JUDGE_DEFAULT_LABEL, NONE_RUN_NOTE, PROSE_RUN_NOTE, RENDER_ARTIST_MODE_LABEL, RENDER_COUNT_DEFAULT_LABEL,
  RENDER_COUNT_MAX, RENDER_SEED_ERROR, SIZE_SOURCE_LABEL_RATIO, SIZE_SOURCE_LABEL_TAGS, STYLE_FALLBACK_BELOW_NOTE, STYLE_FALLBACK_LABEL, STYLE_FALLBACK_MISS_NOTE,
  STYLE_MIN_SCORE_ERROR, STYLE_RENDER_TOP_K_DEFAULT, STYLE_RENDER_TOP_K_MAX, STYLE_RENDER_TOP_K_MIN, STYLE_RUN_BADGE, TRUTH_KIND_LABEL, artistSourceLabel, bestCandidateIndex,
  candidateLabel, clampStyleTopK, defaultRenderForm, defaultStyleRenderForm, fmtAvg, fmtCount, fmtLeaks, fmtRenderCell, fmtRenderSize, hasAlignInfo, isStyleRun, judgeOptions,
  leakCounts, normalizeArtistMode, normalizeBatchArtistMode, normalizeRenderCount, normalizeStyleFallback, parseMinScore, renderCountOptions, renderRequestFields,
  renderSeedError, renderState, replaceRun, rerenderParams, sizeSourceLabel, styleHitInfo, styleMinScoreError, styleRenderDefaults, styleRequestFields, styleRunTitle,
  summaryRenderCells, summaryStyleCells, truthKindBadge, truthKindNote, truthKindOf,
} from "./reverse-summary";

function row(extra: Partial<RunRow> = {}): RunRow {
  return {
    run_id: 1, question_id: "e1#0", provider_id: 1, provider: "gpt", status: "ok", latency_ms: 100, error: null, cleaned_tags: ["smile"],
    metric: null, thumb_path: null, truth_kind: "tags", render_path: null, render_seed: null, render_prompt: null, quality_score: null, quality_notes: [],
    similarity_score: null, similarity_reason: null, render_error: null, leak_appearance: 0, leak_banned: 0, render_artist: null,
    render_artist_source: null, render_size: null, render_size_source: null, plugin_dropped: [], plugin_would_reject: false, render_candidates: [],
    render_style_entry_id: null, render_style_title: null, render_style_score: null, render_style_decision: null, render_style_fallback: false, ...extra,
  };
}
/** §12.7 六个 style_* 键的契约缺省（非 style 模式也发这一份）。 */
const STYLE_DEFAULTS = { style_model: null, style_top_k: 5, style_decisions: [], style_min_score: null, style_fallback: "pool", style_center: true } as const;
const sf = defaultStyleRenderForm;
function cand(extra: Partial<RenderCandidate> = {}): RenderCandidate {
  return { file: "a.png", seed: null, quality_score: null, quality_notes: [], local_path: null, ...extra };
}
const metric = { hit: 3, missed: 1, extra: 2, expected_hit: 0, precision: 0.6, recall: 0.75, f1: 0.66, appearance_leak: 2, banned_leak: 1, ruleset_drop: 0, contradiction: 0 };

describe("truthKindOf / truthKindBadge / truthKindNote", () => {
  it("三种 truth_kind 原样；缺键 / 不认识的值按本地图片题 → none、其余 → tags", () => {
    expect(truthKindOf(row({ truth_kind: "prose" }))).toBe("prose");
    expect(truthKindOf(row({ truth_kind: "none" }))).toBe("none");
    expect(truthKindOf({ question_id: "e1#0", truth_kind: undefined })).toBe("tags");
    expect(truthKindOf({ question_id: null, truth_kind: undefined })).toBe("none");
    expect(truthKindOf({ question_id: "local:/a/1.png", truth_kind: "weird" })).toBe("none");
  });
  it("标签题不标徽标，散文 / 本地各两字；提示只有散文与本地有", () => {
    expect(truthKindBadge(row())).toBeNull();
    expect(truthKindBadge(row({ truth_kind: "prose" }))).toBe("散文");
    expect(truthKindBadge(row({ truth_kind: "none" }))).toBe("本地");
    expect(TRUTH_KIND_LABEL).toEqual({ tags: "标签", prose: "散文", none: "本地" });
    expect(truthKindNote(row())).toBeNull();
    expect(truthKindNote(row({ truth_kind: "prose" }))).toBe(PROSE_RUN_NOTE);
    expect(truthKindNote(row({ truth_kind: "none" }))).toBe(NONE_RUN_NOTE);
    expect(PROSE_RUN_NOTE).toBe("散文题：不计 P/R/F1，只看重画对照与泄漏");
  });
});

describe("fmtAvg / fmtCount", () => {
  it("可空平均值：null / undefined / NaN → —，否则固定小数位", () => {
    expect(fmtAvg(null)).toBe("—");
    expect(fmtAvg(undefined)).toBe("—");
    expect(fmtAvg(Number.NaN)).toBe("—");
    expect(fmtAvg(7.456)).toBe("7.5");
    expect(fmtAvg(8)).toBe("8.0");
    expect(fmtAvg(8.125, 2)).toBe("8.13");
  });
  it("可空计数", () => {
    expect(fmtCount(0)).toBe("0");
    expect(fmtCount(12)).toBe("12");
    expect(fmtCount(null)).toBe("—");
    expect(fmtCount(undefined)).toBe("—");
  });
  it("汇总行四个新列：老后端缺键全部 —", () => {
    expect(summaryRenderCells({ tags_n: 5, render_n: 2, quality_avg: 7.25, similarity_avg: null })).toEqual({ tags_n: "5", render_n: "2", quality_avg: "7.3", similarity_avg: "—" });
    expect(summaryRenderCells({} as never)).toEqual({ tags_n: "—", render_n: "—", quality_avg: "—", similarity_avg: "—" });
  });
});

describe("leakCounts / fmtLeaks", () => {
  it("顶层 leak_* 优先（散文 / 本地题也有），老后端只有 metric 时退到 metric，都没有 → —", () => {
    expect(leakCounts(row({ leak_appearance: 3, leak_banned: 1, metric }))).toEqual({ appearance: 3, banned: 1 });
    expect(fmtLeaks(row({ truth_kind: "prose", leak_appearance: 2, leak_banned: 0 }))).toBe("2/0");
    const legacy = { metric, leak_appearance: undefined, leak_banned: undefined };
    expect(leakCounts(legacy)).toEqual({ appearance: 2, banned: 1 });
    expect(fmtLeaks({ metric: null })).toBe("—");
  });
});

describe("renderState / fmtRenderCell", () => {
  it("没重画 → none / —；写了 render_error → error / 失败；有图或有分数 → ok / 两个分数", () => {
    expect(renderState(row())).toBe("none");
    expect(fmtRenderCell(row())).toBe("—");
    expect(renderState(row({ render_error: "插件不可达" }))).toBe("error");
    expect(fmtRenderCell(row({ render_error: "插件不可达", render_path: "/x.jpg" }))).toBe("失败");
    expect(renderState(row({ render_path: "/x.jpg" }))).toBe("ok");
    expect(renderState(row({ render_seed: 5 }))).toBe("ok");
    expect(fmtRenderCell(row({ render_path: "/x.jpg", quality_score: 7.5, similarity_score: 8 }))).toBe("质量 7.5 · 像 8");
    expect(fmtRenderCell(row({ render_path: "/x.jpg" }))).toBe("质量 — · 像 —");
  });
});

describe("renderSeedError / renderRequestFields（RunConfig 重画表单）", () => {
  it("种子：空与 0..2^32−1 合法，其它给同一句提示", () => {
    expect(renderSeedError("")).toBeNull();
    expect(renderSeedError(" 42 ")).toBeNull();
    expect(renderSeedError("4294967295")).toBeNull();
    expect(renderSeedError("4294967296")).toBe(RENDER_SEED_ERROR);
    expect(renderSeedError("-1")).toBe(RENDER_SEED_ERROR);
    expect(renderSeedError("abc")).toBe(RENDER_SEED_ERROR);
  });
  it("不勾重画：全关（render_artist_mode 仍发契约缺省 pool，style_* 发契约缺省）；默认表单角色是薇拉、画风池随机、插件默认张数、精准反推子表单缺省", () => {
    expect(DEFAULT_RENDER_FORM).toEqual({
      render: false, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "pool", count: null,
      style: { model: null, top_k: 5, decisions: ["include", "exclude", "undecided"], min_score: "", fallback: "pool", center: true },
    });
    // defaultRenderForm() 给深拷贝：嵌套的 style 子对象与决策数组不和只读参照共用
    const f = defaultRenderForm();
    expect(f).toEqual(DEFAULT_RENDER_FORM);
    expect(f.style).not.toBe(DEFAULT_RENDER_FORM.style);
    expect(f.style.decisions).not.toBe(DEFAULT_RENDER_FORM.style.decisions);
    expect(renderRequestFields({ ...DEFAULT_RENDER_FORM, character: "阿莉亚", seed: "7", judge_provider_id: "gpt", artist_mode: "none", count: 3 })).toEqual({
      render: false, render_character: null, render_seed: null, judge_provider_id: null, artist: null, render_artist_mode: "pool", render_count: null, ...STYLE_DEFAULTS,
    });
  });
  it("勾了：角色去空白、空 → null；种子解析、空 / 非法 → null；评委空串 → null；artist 恒 null；画师来源与张数照表单（§12.6）；非 style 模式 style_* 发缺省", () => {
    expect(renderRequestFields({ render: true, character: " 阿莉亚 ", seed: " 42 ", judge_provider_id: " gpt-4o ", artist_mode: "none", count: 3, style: sf() })).toEqual({
      render: true, render_character: "阿莉亚", render_seed: 42, judge_provider_id: "gpt-4o", artist: null, render_artist_mode: "none", render_count: 3, ...STYLE_DEFAULTS,
    });
    expect(renderRequestFields({ render: true, character: "  ", seed: "", judge_provider_id: "", artist_mode: "pool", count: null, style: sf() })).toEqual({
      render: true, render_character: null, render_seed: null, judge_provider_id: null, artist: null, render_artist_mode: "pool", render_count: null, ...STYLE_DEFAULTS,
    });
    expect(renderRequestFields({ render: true, character: "薇拉", seed: "x", judge_provider_id: "", artist_mode: "pool", count: null, style: sf() }).render_seed).toBeNull();
    // pool 模式下子表单里改过的值不进请求（后端不看，发契约缺省）
    expect(renderRequestFields({ render: true, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "pool", count: null, style: { ...sf(), top_k: 9, min_score: "0.4", fallback: "none", center: false } })).toMatchObject(STYLE_DEFAULTS);
    // 表单里混进不认识的来源 / 越界张数（老 localStorage 之类）：来源退 pool、张数退插件默认
    expect(renderRequestFields({ render: true, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "given" as never, count: 9, style: sf() })).toMatchObject({ render_artist_mode: "pool", render_count: null });
  });
});

describe("normalizeArtistMode / normalizeBatchArtistMode / normalizeRenderCount / renderCountOptions（§12.6）", () => {
  it("画师来源：pool / given / none / style 原样，其余 pool；批次里 given → pool、style 原样；下拉列 pool / none / style", () => {
    expect(normalizeArtistMode("given")).toBe("given");
    expect(normalizeArtistMode("none")).toBe("none");
    expect(normalizeArtistMode("pool")).toBe("pool");
    expect(normalizeArtistMode("style")).toBe("style");
    expect(normalizeArtistMode("")).toBe("pool");
    expect(normalizeArtistMode(undefined)).toBe("pool");
    expect(normalizeArtistMode("GIVEN")).toBe("pool");
    expect(normalizeBatchArtistMode("none")).toBe("none");
    expect(normalizeBatchArtistMode("style")).toBe("style");
    expect(normalizeBatchArtistMode("given")).toBe("pool");
    expect(normalizeBatchArtistMode(null)).toBe("pool");
    expect(BATCH_ARTIST_MODES).toEqual(["pool", "none", "style"]);
    expect(RENDER_ARTIST_MODE_LABEL).toEqual({ pool: "画风池随机", given: "指定画师", none: "不带画风", style: "精准反推（画风检索）" });
  });
  it("张数：1..4 整数原样；null / 0 / 5 / 2.5 / 字符串 → null（插件默认）；下拉 = 插件默认 + 1..4", () => {
    expect(RENDER_COUNT_MAX).toBe(4);
    for (const n of [1, 2, 3, 4]) expect(normalizeRenderCount(n)).toBe(n);
    expect(normalizeRenderCount(null)).toBeNull();
    expect(normalizeRenderCount(0)).toBeNull();
    expect(normalizeRenderCount(5)).toBeNull();
    expect(normalizeRenderCount(2.5)).toBeNull();
    expect(normalizeRenderCount("2")).toBeNull();
    expect(normalizeRenderCount(Number.NaN)).toBeNull();
    expect(renderCountOptions()).toEqual([
      { value: null, label: RENDER_COUNT_DEFAULT_LABEL }, { value: 1, label: "1 张" }, { value: 2, label: "2 张" }, { value: 3, label: "3 张" }, { value: 4, label: "4 张" },
    ]);
    expect(RENDER_COUNT_DEFAULT_LABEL).toBe("插件默认");
  });
});

describe("rerenderParams（单条重画的画师来源 / 画师串 / 张数 / style_*）", () => {
  const D = styleRenderDefaults();
  it("单题面板的“重画”：不给画师串与模式 → 沿用批次表单的来源与张数，artist null，非 style 模式 style_* 缺省", () => {
    expect(rerenderParams({ artist_mode: "pool", count: null })).toEqual({ artist: null, artist_mode: "pool", count: null, style: D });
    expect(rerenderParams({ artist_mode: "none", count: 2 })).toEqual({ artist: null, artist_mode: "none", count: 2, style: D });
    expect(rerenderParams({ artist_mode: "given" as never, count: 7 })).toEqual({ artist: null, artist_mode: "pool", count: null, style: D });
  });
  it("画风反推的“用它重画”：明确 given + 画师串（去首尾空白）；只给画师串不给模式也是 given", () => {
    expect(rerenderParams({ artist_mode: "none", count: 3 }, " artist:foo, artist:bar ", "given")).toEqual({ artist: "artist:foo, artist:bar", artist_mode: "given", count: 3, style: D });
    expect(rerenderParams({ artist_mode: "pool", count: null }, "artist:foo")).toEqual({ artist: "artist:foo", artist_mode: "given", count: null, style: D });
  });
  it("given 却没有非空画师串 → 退回批次表单的来源（后端会 bad_request）；pool / none 明确指定时画师串不送", () => {
    expect(rerenderParams({ artist_mode: "none", count: 1 }, "  ", "given")).toEqual({ artist: null, artist_mode: "none", count: 1, style: D });
    expect(rerenderParams({ artist_mode: "pool", count: null }, null, "given")).toEqual({ artist: null, artist_mode: "pool", count: null, style: D });
    expect(rerenderParams({ artist_mode: "pool", count: 4 }, "artist:foo", "none")).toEqual({ artist: null, artist_mode: "none", count: 4, style: D });
    expect(rerenderParams({ artist_mode: "none", count: 4 }, "artist:foo", "pool")).toEqual({ artist: null, artist_mode: "pool", count: 4, style: D });
  });
  it("§12.7 表单来源是 style：单题面板“重画”带子表单的 style_*；“用它重画”given 仍发缺省；given 空串退回 style 也带子表单；没有子表单发缺省", () => {
    const style = { ...sf(), top_k: 8, decisions: ["include"] as const, min_score: "0.35", fallback: "none" as const, center: false };
    const want = { style_model: null, style_top_k: 8, style_decisions: ["include"], style_min_score: 0.35, style_fallback: "none", style_center: false };
    expect(rerenderParams({ artist_mode: "style", count: 2, style: { ...style, decisions: [...style.decisions] } })).toEqual({ artist: null, artist_mode: "style", count: 2, style: want });
    expect(rerenderParams({ artist_mode: "style", count: 2, style: { ...style, decisions: [...style.decisions] } }, "artist:foo", "given")).toEqual({ artist: "artist:foo", artist_mode: "given", count: 2, style: D });
    expect(rerenderParams({ artist_mode: "style", count: null, style: { ...style, decisions: [...style.decisions] } }, " ", "given")).toEqual({ artist: null, artist_mode: "style", count: null, style: want });
    expect(rerenderParams({ artist_mode: "style", count: null })).toEqual({ artist: null, artist_mode: "style", count: null, style: D });
    // 明确指定 style 也行（来源下拉之外的调用方）
    expect(rerenderParams({ artist_mode: "pool", count: 1, style: { ...style, decisions: [...style.decisions] } }, null, "style")).toEqual({ artist: null, artist_mode: "style", count: 1, style: want });
  });
});

describe("clampStyleTopK / parseMinScore / styleMinScoreError / normalizeStyleFallback（§12.7 子表单）", () => {
  it("top_k 收进 1..20 的整数，非数 → 5", () => {
    expect(STYLE_RENDER_TOP_K_DEFAULT).toBe(5);
    expect(STYLE_RENDER_TOP_K_MIN).toBe(1);
    expect(STYLE_RENDER_TOP_K_MAX).toBe(20);
    expect(clampStyleTopK(5)).toBe(5);
    expect(clampStyleTopK(0)).toBe(1);
    expect(clampStyleTopK(-3)).toBe(1);
    expect(clampStyleTopK(21)).toBe(20);
    expect(clampStyleTopK(2.6)).toBe(3);
    expect(clampStyleTopK("7")).toBe(5);
    expect(clampStyleTopK(Number.NaN)).toBe(5);
    expect(clampStyleTopK(undefined)).toBe(5);
  });
  it("最低相似度：空 → null；0..1 的数 → 数；负数 / >1 / 非数 / 科学记数 → undefined + 同一句提示", () => {
    expect(parseMinScore("")).toBeNull();
    expect(parseMinScore("   ")).toBeNull();
    expect(parseMinScore("0")).toBe(0);
    expect(parseMinScore("1")).toBe(1);
    expect(parseMinScore(" 0.35 ")).toBe(0.35);
    expect(parseMinScore(".5")).toBe(0.5);
    expect(parseMinScore("1.0")).toBe(1);
    expect(parseMinScore("1.01")).toBeUndefined();
    expect(parseMinScore("-0.1")).toBeUndefined();
    expect(parseMinScore("abc")).toBeUndefined();
    expect(parseMinScore("1e-1")).toBeUndefined();
    expect(parseMinScore("0.5.1")).toBeUndefined();
    expect(styleMinScoreError("")).toBeNull();
    expect(styleMinScoreError("0.4")).toBeNull();
    expect(styleMinScoreError("2")).toBe(STYLE_MIN_SCORE_ERROR);
    expect(styleMinScoreError("x")).toBe(STYLE_MIN_SCORE_ERROR);
  });
  it("兜底：none 原样，其余 pool；两种兜底的文字", () => {
    expect(normalizeStyleFallback("none")).toBe("none");
    expect(normalizeStyleFallback("pool")).toBe("pool");
    expect(normalizeStyleFallback("")).toBe("pool");
    expect(normalizeStyleFallback(undefined)).toBe("pool");
    expect(normalizeStyleFallback("NONE")).toBe("pool");
    expect(STYLE_FALLBACK_LABEL).toEqual({ pool: "画风池随机", none: "不带画风" });
  });
});

describe("styleRequestFields（§12.7 子表单 → 六个 style_* 键）", () => {
  it("指定模型随精准反推传递，null 留给批次启动时解析；完整配方重画走 given", () => {
    const form = { ...sf(), model: "csd-vit-large-patch14" };
    expect(styleRequestFields(form, "style").style_model).toBe("csd-vit-large-patch14");
    expect(styleRequestFields({ ...form, model: null }, "style").style_model).toBeNull();
    const recipe = "artist:foo, masterpiece, year 2025,\n-1::thick lines::, blue palette";
    expect(rerenderParams({ artist_mode: "style", count: 1, style: form }, `  ${recipe}  `, "given")).toEqual({ artist: recipe, artist_mode: "given", count: 1, style: STYLE_DEFAULTS });
  });
  it("契约缺省：top_k 5、[]、null、pool、true；非 style 模式 / 没有子表单一律发缺省（每次新对象）", () => {
    expect(styleRenderDefaults()).toEqual(STYLE_DEFAULTS);
    expect(styleRenderDefaults()).not.toBe(styleRenderDefaults());
    expect(styleRenderDefaults().style_decisions).not.toBe(styleRenderDefaults().style_decisions);
    const changed = { ...sf(), top_k: 9, decisions: ["exclude"], min_score: "0.4", fallback: "none", center: false } as const;
    for (const m of ["pool", "none", "given"] as const) expect(styleRequestFields({ ...changed, decisions: [...changed.decisions] }, m)).toEqual(STYLE_DEFAULTS);
    expect(styleRequestFields(null, "style")).toEqual(STYLE_DEFAULTS);
    expect(styleRequestFields(undefined, "style")).toEqual(STYLE_DEFAULTS);
    expect(styleRequestFields(sf(), "style")).toEqual(STYLE_DEFAULTS);
  });
  it("style 模式：top_k 收口、决策按契约顺序去重丢坏值、空 / 全勾都发 []、门槛空 / 非法 → null、兜底归一、减库均值布尔化", () => {
    expect(styleRequestFields({ model: null, top_k: 30, decisions: ["undecided", "include", "weird" as never, "include"], min_score: "0.35", fallback: "none", center: false }, "style")).toEqual({
      style_model: null, style_top_k: 20, style_decisions: ["include", "undecided"], style_min_score: 0.35, style_fallback: "none", style_center: false,
    });
    expect(styleRequestFields({ model: null, top_k: 0, decisions: [], min_score: "", fallback: "pool", center: true }, "style")).toEqual({
      style_model: null, style_top_k: 1, style_decisions: [], style_min_score: null, style_fallback: "pool", style_center: true,
    });
    expect(styleRequestFields({ model: null, top_k: 5, decisions: ["exclude", "undecided", "include"], min_score: "abc", fallback: "x" as never, center: 1 as never }, "style")).toEqual({
      style_model: null, style_top_k: 5, style_decisions: [], style_min_score: null, style_fallback: "pool", style_center: true,
    });
    expect(styleRequestFields({ model: null, top_k: 3, decisions: ["exclude"], min_score: "0", fallback: "pool", center: true }, "style")).toMatchObject({ style_decisions: ["exclude"], style_min_score: 0 });
  });
  it("renderRequestFields 在 style 模式下带子表单的 style_*", () => {
    const form = { render: true, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "style" as const, count: 2, style: { ...sf(), top_k: 3, decisions: ["include" as const], min_score: "0.3" } };
    expect(renderRequestFields(form)).toEqual({
      render: true, render_character: "薇拉", render_seed: null, judge_provider_id: null, artist: null, render_artist_mode: "style", render_count: 2,
      style_model: null, style_top_k: 3, style_decisions: ["include"], style_min_score: 0.3, style_fallback: "pool", style_center: true,
    });
    // 不勾重画时即使来源是 style 也发缺省
    expect(renderRequestFields({ ...form, render: false })).toMatchObject({ render: false, render_artist_mode: "pool", ...STYLE_DEFAULTS });
  });
});

describe("isStyleRun / styleHitInfo / styleRunTitle / summaryStyleCells（§12.7 结果表与单题面板）", () => {
  const hit = { render_artist_source: "style" as const, render_artist: "artist:foo", render_style_entry_id: "e9", render_style_title: "画师甲", render_style_score: 0.5123, render_style_decision: "include" as const, render_style_fallback: false };
  it("只有 render_artist_source=style 算检索 run；徽标两字", () => {
    expect(STYLE_RUN_BADGE).toBe("检索");
    expect(isStyleRun(row(hit))).toBe(true);
    expect(isStyleRun(row({ render_artist_source: "given", render_artist: "artist:foo" }))).toBe(false);
    expect(isStyleRun(row())).toBe(false);
    expect(ARTIST_SOURCE_LABEL.style).toBe("检索命中");
    expect(artistSourceLabel("style")).toBe("检索命中");
  });
  it("命中信息：标题 / 三位小数相似度 / 决策与中文 / 没兜底 note null；标题空退到 entry_id；老记录 / 其它模式 → null", () => {
    expect(styleHitInfo(row(hit))).toEqual({ entry_id: "e9", title: "画师甲", score: "0.512", decision: "include", decision_label: "加入", fallback: false, note: null });
    expect(styleHitInfo(row({ ...hit, render_style_title: "  ", render_style_decision: "exclude" }))).toMatchObject({ title: "e9", decision_label: "不加入" });
    expect(styleHitInfo(row())).toBeNull();
    expect(styleHitInfo(row({ render_artist_source: "pool", render_artist: "artist:x" }))).toBeNull();
    // 老后端缺五键（undefined）但来源是 style：仍显示，全部占位
    expect(styleHitInfo({ render_artist_source: "style" })).toEqual({ entry_id: null, title: "—", score: "—", decision: null, decision_label: null, fallback: false, note: null });
  });
  it("兜底：命中过但低于门槛（有 entry_id）与根本没命中（无 entry_id）各一句；兜底后来源是 pool / none 也显示", () => {
    const below = row({ ...hit, render_artist_source: "pool", render_artist: "artist:pool", render_style_score: 0.12, render_style_fallback: true });
    expect(styleHitInfo(below)).toMatchObject({ entry_id: "e9", score: "0.120", fallback: true, note: STYLE_FALLBACK_BELOW_NOTE });
    const miss = row({ render_artist_source: "none", render_style_fallback: true });
    expect(styleHitInfo(miss)).toEqual({ entry_id: null, title: "—", score: "—", decision: null, decision_label: null, fallback: true, note: STYLE_FALLBACK_MISS_NOTE });
    expect(isStyleRun(below)).toBe(false);
  });
  it("徽标 title：`检索命中：标题 · 0.512 · 加入`，兜底了换行附说明；非检索 run 空串", () => {
    expect(styleRunTitle(row(hit))).toBe("检索命中：画师甲 · 0.512 · 加入");
    expect(styleRunTitle(row({ ...hit, render_style_decision: null }))).toBe("检索命中：画师甲 · 0.512");
    expect(styleRunTitle(row({ ...hit, render_style_fallback: true }))).toBe(`检索命中：画师甲 · 0.512 · 加入\n${STYLE_FALLBACK_BELOW_NOTE}`);
    expect(styleRunTitle(row())).toBe("");
  });
  it("汇总行两列：张数与三位小数均值，老后端缺键全 —", () => {
    expect(summaryStyleCells({ style_hit_n: 4, style_score_avg: 0.41234 })).toEqual({ style_hit_n: "4", style_score_avg: "0.412" });
    expect(summaryStyleCells({ style_hit_n: 0, style_score_avg: null })).toEqual({ style_hit_n: "0", style_score_avg: "—" });
    expect(summaryStyleCells({})).toEqual({ style_hit_n: "—", style_score_avg: "—" });
  });
});

describe("sizeSourceLabel / fmtRenderSize（§12.6 尺寸来源）", () => {
  it("tags → 按标签；其它非空（插件发 ratio、后端夹具用 image）→ 按原图比例；空 / null → null；大小写空白不敏感", () => {
    expect(sizeSourceLabel("tags")).toBe(SIZE_SOURCE_LABEL_TAGS);
    expect(sizeSourceLabel(" TAGS ")).toBe("按标签");
    expect(sizeSourceLabel("ratio")).toBe(SIZE_SOURCE_LABEL_RATIO);
    expect(sizeSourceLabel("image")).toBe("按原图比例");
    expect(sizeSourceLabel("")).toBeNull();
    expect(sizeSourceLabel("  ")).toBeNull();
    expect(sizeSourceLabel(null)).toBeNull();
    expect(sizeSourceLabel(undefined)).toBeNull();
  });
  it("尺寸一行：尺寸 · 来源；老插件只有尺寸；只有来源给来源；都没有 null", () => {
    expect(fmtRenderSize(row({ render_size: "832x1216", render_size_source: "ratio" }))).toBe("832x1216 · 按原图比例");
    expect(fmtRenderSize(row({ render_size: "1216x832", render_size_source: "tags" }))).toBe("1216x832 · 按标签");
    expect(fmtRenderSize(row({ render_size: "1024x1024" }))).toBe("1024x1024");
    expect(fmtRenderSize(row({ render_size: " ", render_size_source: "tags" }))).toBe("按标签");
    expect(fmtRenderSize(row())).toBeNull();
  });
});

describe("artistSourceLabel / hasAlignInfo（§12.6 画师来源）", () => {
  it("pool / given / none / style 给中文；空 → null；不认识的原样（去空白小写）", () => {
    expect(ARTIST_SOURCE_LABEL).toEqual({ pool: "画风池抽中", given: "指定画师", none: "不带画风", style: "检索命中" });
    expect(artistSourceLabel("pool")).toBe("画风池抽中");
    expect(artistSourceLabel("given")).toBe("指定画师");
    expect(artistSourceLabel(" None ")).toBe("不带画风");
    expect(artistSourceLabel("style")).toBe("检索命中");
    expect(artistSourceLabel("pack")).toBe("pack");
    expect(artistSourceLabel("")).toBeNull();
    expect(artistSourceLabel(null)).toBeNull();
    expect(artistSourceLabel(undefined)).toBeNull();
  });
  it("有 size_source 或 artist_source 任一才算新插件的对齐信息", () => {
    expect(hasAlignInfo(row())).toBe(false);
    expect(hasAlignInfo(row({ render_size_source: "tags" }))).toBe(true);
    expect(hasAlignInfo(row({ render_artist_source: "none" }))).toBe(true);
    expect(hasAlignInfo(row({ render_size: "832x1216", render_path: "/x.jpg" }))).toBe(false);
  });
});

describe("bestCandidateIndex / candidateLabel（§12.6 候选）", () => {
  it("先按 seed 对顶层 render_seed；对不上按质量分最高（并列取先出现的）；都没分 → 0；没候选 → null", () => {
    const cs = [cand({ seed: 10, quality_score: 6 }), cand({ seed: 11, quality_score: 8 }), cand({ seed: 12, quality_score: 8 })];
    expect(bestCandidateIndex(cs, { render_seed: 12 })).toBe(2);
    expect(bestCandidateIndex(cs, { render_seed: 99 })).toBe(1);
    expect(bestCandidateIndex(cs, { render_seed: null })).toBe(1);
    expect(bestCandidateIndex([cand({ seed: 1, quality_score: null }), cand({ seed: 2, quality_score: 3 })], { render_seed: null })).toBe(1);
    expect(bestCandidateIndex([cand({ seed: 1 }), cand({ seed: 2 })], { render_seed: null })).toBe(0);
    expect(bestCandidateIndex([cand({ seed: null, quality_score: Number.NaN })], { render_seed: null })).toBe(0);
    expect(bestCandidateIndex([], { render_seed: 1 })).toBeNull();
  });
  it("候选标签：seed 与分数，缺的用 —，整数分不带小数", () => {
    expect(candidateLabel(cand({ seed: 123, quality_score: 7.5 }))).toBe("seed 123 · 7.5");
    expect(candidateLabel(cand({ seed: 7, quality_score: 8 }))).toBe("seed 7 · 8");
    expect(candidateLabel(cand())).toBe("seed — · —");
  });
});

describe("judgeOptions", () => {
  it("插件默认在最前；插件 id 去重去空；当前值不在列表里也保留一项；不可达（null）只剩默认", () => {
    expect(judgeOptions(["gpt", "", "gemini", "gpt"], "")).toEqual([
      { value: "", label: JUDGE_DEFAULT_LABEL }, { value: "gpt", label: "gpt" }, { value: "gemini", label: "gemini" },
    ]);
    expect(judgeOptions(["gpt"], "gone")).toEqual([
      { value: "", label: JUDGE_DEFAULT_LABEL }, { value: "gpt", label: "gpt" }, { value: "gone", label: "gone（已不在可选名单）", disabled: true },
    ]);
    expect(judgeOptions(null, "")).toEqual([{ value: "", label: JUDGE_DEFAULT_LABEL }]);
    expect(judgeOptions(null, "gpt")).toEqual([{ value: "", label: JUDGE_DEFAULT_LABEL }, { value: "gpt", label: "gpt（已不在可选名单）", disabled: true }]);
  });
});

describe("replaceRun", () => {
  it("按 run_id 换一行（新数组，原数组不动）；不在表里返回原数组", () => {
    const a = row({ run_id: 1 });
    const b = row({ run_id: 2 });
    const list = [a, b];
    const b2 = row({ run_id: 2, render_path: "/2.jpg" });
    const out = replaceRun(list, b2);
    expect(out).not.toBe(list);
    expect(out).toEqual([a, b2]);
    expect(list[1]).toBe(b);
    expect(replaceRun(list, row({ run_id: 9 }))).toBe(list);
  });
});
