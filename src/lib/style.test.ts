// style.ts 纯函数：相似度映射、命中文字与画师串、来源解析与下拉选项、表单 → 请求（含缺省）、模型状态一行、下载 / 建索引进度、bad_request 判定（CONTRACT §12）。
import { describe, expect, it } from "vitest";
import type { Question, StyleHit, StyleModelStatus } from "../types";
import {
  EMBED_PHASE_LABEL, STYLE_CODEX_IDS, STYLE_TOP_K_DEFAULT, buildStyleRequest, classifierArtistTag, clampTopK, defaultSourceId, defaultStyleForm, defaultStyleRequest,
  downloadLabel, downloadRatio, embedLabel, embedRatio, fmtCosine, hitArtist, hitBadges, hitLabel, hitMuted, hitPath, indexRatio, isStyleSetupError,
  scorePercent, scoreUnit, sourceFromQuestionId, styleFormError, styleReady, styleSourceOptions, styleStatusLine,
} from "./style";

function hit(extra: Partial<StyleHit> = {}): StyleHit {
  return {
    entry_id: "e1", codex_id: "artist_nai5_personal", title: "作者乙", path: ["单画师词典", "作者乙"], decision: "include", blacklisted: false,
    score: 0.42, style_prompt: "artist:foo, 0.8::artist:bar::", core: "artist:foo, 0.8::artist:bar::", pack_tags: null, thumb_path: null, prose: false, ...extra,
  };
}
function question(id: string, extra: Partial<Question> = {}): Question {
  return { question_id: `${id}#0`, entry_id: id, ordinal: 0, title: `题 ${id}`, rating: "safe", thumb_path: `/t/${id}.jpg`, local_path: null, truth_source: "tags", truth_preview: "1girl", ...extra };
}
function status(extra: Partial<StyleModelStatus> = {}): StyleModelStatus {
  return { model: "clip-vit-base-patch32", label: "CLIP ViT-B/32", description: "CLIP", dim: 512, download_size_bytes: 605247071, default_center: true, dir: "/d/models/clip-vit-base-patch32", present: true, size_bytes: 605 * 1024 * 1024, indexed: 1890, total: 3200, mean_ready: true, device_preference: "auto", active_device: "cpu", metal_available: false, device_message: null, ...extra };
}
const MB = 1024 * 1024;

describe("scoreUnit / scorePercent / fmtCosine", () => {
  it("0..1 原样，负数按 0，超过 1 按 1，非数按 0", () => {
    expect(scoreUnit(0.42)).toBeCloseTo(0.42);
    expect(scoreUnit(-0.2)).toBe(0);
    expect(scoreUnit(1.5)).toBe(1);
    expect(scoreUnit(Number.NaN)).toBe(0);
    expect(scorePercent(0.426)).toBe(43);
    expect(scorePercent(-1)).toBe(0);
    expect(scorePercent(1)).toBe(100);
  });
  it("数字原样三位小数，负的也显示；非数 —", () => {
    expect(fmtCosine(0.4264)).toBe("0.426");
    expect(fmtCosine(-0.05)).toBe("-0.050");
    expect(fmtCosine(1)).toBe("1.000");
    expect(fmtCosine(null)).toBe("—");
    expect(fmtCosine(undefined)).toBe("—");
    expect(fmtCosine(Number.NaN)).toBe("—");
  });
});

describe("hitArtist / hitPath / hitLabel / hitBadges / hitMuted", () => {
  it("分类器复制仅补 artist: 前缀，保留画师标签格式", () => {
    expect(classifierArtistTag("  foo_(bar) ")).toBe("artist:foo_(bar)");
    expect(classifierArtistTag("artist:foo")).toBe("artist:foo");
    expect(classifierArtistTag("  ")).toBe("");
  });
  it("完整原串保留全部词、负权重与格式；旧 core/pack_tags 不替代空配方", () => {
    expect(hitArtist(hit())).toBe("artist:foo, 0.8::artist:bar::");
    const recipe = "artist:foo, 0.8::artist:bar::, masterpiece, year 2025,\n-1.2::thick lines::, blue palette";
    expect(hitArtist(hit({ style_prompt: `  ${recipe}  `, core: "artist:foo", pack_tags: "1girl" }))).toBe(recipe);
    expect(hitArtist(hit({ style_prompt: "", core: "artist:foo", pack_tags: "1girl" }))).toBeNull();
    expect(hitArtist(hit({ style_prompt: "  " }))).toBeNull();
  });
  it("标题 · 目录；没目录只有标题；徽标：词典、决策、散文；不加入置灰", () => {
    expect(hitPath(hit())).toBe("单画师词典 / 作者乙");
    expect(hitLabel(hit())).toBe("作者乙 · 单画师词典 / 作者乙");
    expect(hitLabel(hit({ path: [] }))).toBe("作者乙");
    expect(hitBadges(hit())).toEqual(["画师词典", "加入"]);
    expect(hitBadges(hit({ codex_id: "nai5_community_pack", decision: "exclude", prose: true }))).toEqual(["精选图包", "不加入", "散文"]);
    expect(hitBadges(hit({ codex_id: "weird", decision: "undecided" }))).toEqual(["weird", "未定"]);
    expect(hitMuted(hit())).toBe(false);
    expect(hitMuted(hit({ decision: "exclude" }))).toBe(true);
    expect(hitMuted(hit({ decision: "undecided" }))).toBe(false);
  });
});

describe("sourceFromQuestionId", () => {
  it("local: 前缀 → path；entry#ordinal → entry；缺 # 按 ordinal 0；坏值 null", () => {
    expect(sourceFromQuestionId("local:/a/b.png")).toEqual({ kind: "path", path: "/a/b.png" });
    expect(sourceFromQuestionId("e1#2")).toEqual({ kind: "entry", entry_id: "e1", ordinal: 2 });
    expect(sourceFromQuestionId("e1")).toEqual({ kind: "entry", entry_id: "e1", ordinal: 0 });
    expect(sourceFromQuestionId("a#b#3")).toEqual({ kind: "entry", entry_id: "a#b", ordinal: 3 });
    expect(sourceFromQuestionId("e1#x")).toEqual({ kind: "entry", entry_id: "e1", ordinal: 0 });
    expect(sourceFromQuestionId("e1#-1")).toEqual({ kind: "entry", entry_id: "e1", ordinal: 0 });
    expect(sourceFromQuestionId("#0")).toBeNull();
    expect(sourceFromQuestionId("local:")).toBeNull();
    expect(sourceFromQuestionId("")).toBeNull();
    expect(sourceFromQuestionId(null)).toBeNull();
    expect(sourceFromQuestionId(undefined)).toBeNull();
  });
});

describe("styleSourceOptions / defaultSourceId", () => {
  const qs = [question("e1"), question("e2", { ordinal: 1, question_id: "e2#1" })];
  it("按勾选先后列出；图包题带标题与缩略图，本地图片用文件名且缩略图是它自己；不在题库里的退化为 id", () => {
    const opts = styleSourceOptions(new Set(["e2#1", "local:/x/y/pic.png", "gone#0", "e1#0"]), qs, null);
    expect(opts.map((o) => o.id)).toEqual(["e2#1", "local:/x/y/pic.png", "gone#0", "e1#0"]);
    expect(opts[0]).toEqual({ id: "e2#1", label: "题 e2 #2", thumb_path: "/t/e2.jpg", source: { kind: "entry", entry_id: "e2", ordinal: 1 } });
    expect(opts[1]).toEqual({ id: "local:/x/y/pic.png", label: "本地 · pic.png", thumb_path: "/x/y/pic.png", source: { kind: "path", path: "/x/y/pic.png" } });
    expect(opts[2]).toEqual({ id: "gone#0", label: "gone#0", thumb_path: null, source: { kind: "entry", entry_id: "gone", ordinal: 0 } });
    expect(defaultSourceId(opts)).toBe("e1#0");
  });
  it("当前 run 的原图排最前并作为缺省；同一道题在勾选里不再重复；run 是本地图片题也行", () => {
    const opts = styleSourceOptions(new Set(["e1#0", "e2#1"]), qs, { run_id: 7, question_id: "e1#0", thumb_path: "/t/run7.jpg" });
    expect(opts.map((o) => o.id)).toEqual(["run:7", "e2#1"]);
    expect(opts[0].label).toBe("当前 run #7 的原图 · 题 e1");
    expect(opts[0].thumb_path).toBe("/t/run7.jpg");
    expect(opts[0].source).toEqual({ kind: "entry", entry_id: "e1", ordinal: 0 });
    expect(defaultSourceId(opts)).toBe("run:7");

    const local = styleSourceOptions([], qs, { run_id: 8, question_id: "local:/p/q.jpg", thumb_path: null });
    expect(local).toHaveLength(1);
    expect(local[0].source).toEqual({ kind: "path", path: "/p/q.jpg" });
    expect(local[0].thumb_path).toBe("/p/q.jpg");
    expect(local[0].label).toBe("当前 run #8 的原图 · 本地 · q.jpg");
  });
  it("没 run、没勾选 → 空列表、缺省 null；run 没 question_id 也不出现", () => {
    expect(styleSourceOptions([], qs, null)).toEqual([]);
    expect(styleSourceOptions([], qs, { run_id: 1, question_id: null, thumb_path: null })).toEqual([]);
    expect(defaultSourceId([])).toBeNull();
  });
});

describe("defaultStyleForm / clampTopK / styleFormError", () => {
  it("缺省表单：top_k 10、仅画风词典、三种决策都勾、减库均值开；每次新对象", () => {
    const f = defaultStyleForm();
    expect(f).toEqual({ model: null, top_k: 10, codex_ids: ["artist_nai5_personal", "nax_v5_artist"], decisions: ["include", "exclude", "undecided"], center: true });
    expect(STYLE_TOP_K_DEFAULT).toBe(10);
    expect(defaultStyleForm().codex_ids).not.toBe(f.codex_ids);
    expect(STYLE_CODEX_IDS).toEqual(["artist_nai5_personal", "nax_v5_artist"]);
  });
  it("top_k 收进 1..50 的整数；非数按 10", () => {
    expect(clampTopK(10)).toBe(10);
    expect(clampTopK(0)).toBe(1);
    expect(clampTopK(99)).toBe(50);
    expect(clampTopK(7.6)).toBe(8);
    expect(clampTopK(Number.NaN)).toBe(10);
    expect(clampTopK("12")).toBe(10);
    expect(clampTopK(undefined)).toBe(10);
  });
  it("一本都不勾 / 一种决策都不勾各给一句；不认识的值不算", () => {
    expect(styleFormError(defaultStyleForm())).toBeNull();
    expect(styleFormError({ codex_ids: [], decisions: ["include"] })).toBe("至少勾一本词典");
    expect(styleFormError({ codex_ids: ["nope"], decisions: ["include"] })).toBe("至少勾一本词典");
    expect(styleFormError({ codex_ids: ["nax_v5_artist"], decisions: ["include"] })).toBeNull();
    expect(styleFormError({ codex_ids: ["artist_nai5_personal"], decisions: [] })).toBe("至少勾一种决策");
  });
});

describe("buildStyleRequest / defaultStyleRequest", () => {
  const src = { kind: "entry", entry_id: "e1", ordinal: 0 } as const;
  it("词典固定为画风词典，全勾决策发空数组；缺省请求就是 §12.2 的缺省", () => {
    expect(buildStyleRequest(src, defaultStyleForm())).toEqual({ source: src, top_k: 10, codex_ids: ["artist_nai5_personal", "nax_v5_artist"], decisions: [], center: true });
    expect(defaultStyleRequest({ kind: "path", path: "/a.png" })).toEqual({ source: { kind: "path", path: "/a.png" }, top_k: 10, codex_ids: ["artist_nai5_personal", "nax_v5_artist"], decisions: [], center: true });
  });
  it("部分勾选按契约顺序去重、丢掉不认识的；top_k 收口；center 转布尔；来源是拷贝", () => {
    const req = buildStyleRequest(src, { model: null, top_k: 500, codex_ids: ["nai5_community_pack", "bogus", "nai5_community_pack"], decisions: ["undecided", "include", "zzz" as never], center: false });
    expect(req).toEqual({ source: src, top_k: 50, codex_ids: ["artist_nai5_personal", "nax_v5_artist"], decisions: ["include", "undecided"], center: false });
    expect(req.source).not.toBe(src);
    expect(buildStyleRequest(src, { model: null, top_k: 0.4, codex_ids: ["artist_nai5_personal"], decisions: ["exclude"], center: true }).top_k).toBe(1);
  });
});

describe("styleStatusLine / indexRatio / styleReady", () => {
  it("已下载给大小与均值；未下载只给索引数", () => {
    expect(styleStatusLine(status())).toBe("已下载 605.0 MB · 已索引 1890 / 3200 · 均值就绪");
    expect(styleStatusLine(status({ mean_ready: false }))).toBe("已下载 605.0 MB · 已索引 1890 / 3200");
    expect(styleStatusLine(status({ present: false, size_bytes: 0, indexed: 0 }))).toBe("未下载 · 已索引 0 / 3200");
  });
  it("索引比例与可检索", () => {
    expect(indexRatio(status())).toBeCloseTo(1890 / 3200);
    expect(indexRatio(status({ total: 0 }))).toBeNull();
    expect(indexRatio(status({ indexed: 5000 }))).toBe(1);
    expect(styleReady(status())).toBe(true);
    expect(styleReady(status({ indexed: 0 }))).toBe(false);
    expect(styleReady(status({ present: false }))).toBe(false);
    expect(styleReady(null)).toBe(false);
  });
});

describe("downloadRatio / downloadLabel / embedRatio / embedLabel", () => {
  it("下载：有总量给比例与百分比；没总量跑马灯只给已下；失败与完成各一句；没事件是准备中", () => {
    const ev = { file: "model.safetensors", done_bytes: 100 * MB, total_bytes: 600 * MB, finished: false, error: null };
    expect(downloadRatio(ev)).toBeCloseTo(1 / 6);
    expect(downloadLabel(ev)).toBe("model.safetensors 100.0 MB / 600.0 MB（17%）");
    expect(downloadRatio({ ...ev, total_bytes: 0 })).toBeNull();
    expect(downloadLabel({ ...ev, total_bytes: 0 })).toBe("model.safetensors 100.0 MB");
    expect(downloadLabel({ ...ev, finished: true })).toBe("model.safetensors 完成");
    expect(downloadLabel({ ...ev, error: "403" })).toBe("model.safetensors：403");
    expect(downloadLabel(null)).toBe("准备下载…");
    expect(downloadRatio(null)).toBeNull();
    expect(downloadRatio({ done_bytes: 700 * MB, total_bytes: 600 * MB })).toBe(1);
  });
  it("建索引：embed_total 有才有比例；标签带失败数；没计数给省略号", () => {
    expect(embedRatio({ embed_total: 3200, embed_done: 800 })).toBeCloseTo(0.25);
    expect(embedRatio({ embed_total: 0, embed_done: 0 })).toBeNull();
    expect(embedRatio({})).toBeNull();
    expect(embedRatio(null)).toBeNull();
    expect(embedLabel({ embed_total: 3200, embed_done: 120, embed_failed: 2 })).toBe("画风索引 120 / 3200，失败 2");
    expect(embedLabel({ embed_total: 3200, embed_done: 3200, embed_failed: 0 })).toBe("画风索引 3200 / 3200");
    expect(embedLabel({ embed_total: 10 })).toBe("画风索引 0 / 10");
    expect(embedLabel({})).toBe(`${EMBED_PHASE_LABEL}…`);
    expect(embedLabel(undefined)).toBe("画风索引…");
  });
});

describe("isStyleSetupError", () => {
  it("只有 AppError code=bad_request 算（模型未下载 / 索引为空）", () => {
    expect(isStyleSetupError({ code: "bad_request", message: "画风模型未下载" })).toBe(true);
    expect(isStyleSetupError({ code: "network", message: "x" })).toBe(false);
    expect(isStyleSetupError(new Error("bad_request"))).toBe(false);
    expect(isStyleSetupError(null)).toBe(false);
    expect(isStyleSetupError("bad_request")).toBe(false);
  });
});
