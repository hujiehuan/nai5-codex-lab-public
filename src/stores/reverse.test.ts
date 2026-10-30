// reverse store 里题库本地图片的两条路：addLocalImagePath 去重与勾选、addDroppedPaths 分拣 + 只提示一次（CONTRACT §10.3）；
// v0.2.4（§11.4）："只看标签题"过滤、runBatch 带重画四键、单条重画换行；v0.3.0（§12.5）：当前 run 与带画师串的重画；
// v0.3.0（§12.6）：runBatch 带画师来源 / 张数，单条重画沿用表单的来源与张数、"用它重画"= given；
// v0.3.1（§12.7）：来源 style 时 runBatch 与单条重画带子表单的六个 style_* 键，其余模式发契约缺省。
// 不依赖 DOM：mock 掉 ../api，用 createPinia + setActivePinia 在 node 环境直接跑（同 codex.test.ts）。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

vi.mock("../api", () => ({
  api: {
    reversePickLocalImage: vi.fn(),
    // v0.2.5：本地图片先经后端拷进数据目录；测试里原路返回
    reverseImportLocalImage: vi.fn((p: string) => Promise.resolve(p)),
    reverseQuestions: vi.fn(),
    reverseRunBatch: vi.fn(),
    reverseCancelBatch: vi.fn(),
    reverseBatchesList: vi.fn(),
    reverseRenderRun: vi.fn(),
    reverseBatchResults: vi.fn(),
    reverseRunGet: vi.fn(),
  },
  errMessage: (e: unknown) => (e instanceof Error ? e.message : String(e)),
  Channel: class {},
  convertFileSrc: (p: string) => p,
}));

import { api } from "../api";
import { NON_IMAGE_TOAST } from "../lib/dnd";
import { defaultStyleRenderForm, styleRenderDefaults } from "../lib/reverse-summary";
import type { BatchResults, Question, RunDetail, RunEvent, RunRow } from "../types";
import { localQuestionId, useReverse } from "./reverse";
import { useToast } from "./toast";

beforeEach(() => {
  setActivePinia(createPinia());
  for (const fn of [api.reversePickLocalImage, api.reverseQuestions, api.reverseRunBatch, api.reverseCancelBatch, api.reverseBatchesList, api.reverseRenderRun, api.reverseBatchResults, api.reverseRunGet]) vi.mocked(fn).mockReset();
  vi.mocked(api.reverseBatchesList).mockResolvedValue([]);
  vi.mocked(api.reverseCancelBatch).mockResolvedValue(undefined);
});

function question(id: string, extra: Partial<Question> = {}): Question {
  return { question_id: `${id}#0`, entry_id: id, ordinal: 0, title: id, rating: "safe", thumb_path: null, local_path: null, truth_source: "tags", truth_preview: "1girl, smile", ...extra };
}
function runRow(extra: Partial<RunRow> = {}): RunRow {
  return {
    run_id: 1, question_id: "e1#0", provider_id: 1, provider: "gpt", status: "ok", latency_ms: 100, error: null, cleaned_tags: ["smile"],
    metric: null, thumb_path: null, truth_kind: "tags", render_path: null, render_seed: null, render_prompt: null, quality_score: null, quality_notes: [],
    similarity_score: null, similarity_reason: null, render_error: null, leak_appearance: 0, leak_banned: 0, render_artist: null,
    render_artist_source: null, render_size: null, render_size_source: null, plugin_dropped: [], plugin_would_reject: false, render_candidates: [],
    render_style_entry_id: null, render_style_title: null, render_style_score: null, render_style_decision: null, render_style_fallback: false, ...extra,
  };
}
/** §12.7 六个 style_* 键的契约缺省（非 style 模式也发）。 */
const SD = styleRenderDefaults();
function batchResults(runs: RunRow[]): BatchResults {
  return {
    batch: { batch_id: "b1", created_at: "2026-09-07T00:00:00Z", finished_at: null, status: "done", template: "t", ruleset: "r", providers: ["gpt"], question_count: runs.length, note: null },
    runs,
    summary: [],
  };
}
function detail(run: RunRow): RunDetail {
  return { run, raw_response: null, parsed: null, pred_raw: [], pred: [], truth: [], truth_eval: [], expected_miss: [], hits: [], misses: [], extras: [], expected_hits: [], contradictions: [], request: {}, judgment: null };
}

describe("reverse store · addLocalImagePath", () => {
  it("加进列表并勾选；再加同一路径不重复，仍勾选", async () => {
    const rev = useReverse();
    await rev.addLocalImagePath("/a/1.png");
    expect(rev.localImages).toEqual(["/a/1.png"]);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/1.png"))).toBe(true);

    rev.toggleQuestion(localQuestionId("/a/1.png"));
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/1.png"))).toBe(false);

    await rev.addLocalImagePath("/a/1.png");
    expect(rev.localImages).toEqual(["/a/1.png"]);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/1.png"))).toBe(true);
  });
  it("空串不做任何事", async () => {
    const rev = useReverse();
    await rev.addLocalImagePath("");
    expect(rev.localImages).toEqual([]);
    expect(rev.selectedQuestionIds.size).toBe(0);
  });
  it("removeLocalImage 同时去掉勾选", async () => {
    const rev = useReverse();
    await rev.addLocalImagePath("/a/1.png");
    await rev.addLocalImagePath("/a/2.png");
    rev.removeLocalImage("/a/1.png");
    expect(rev.localImages).toEqual(["/a/2.png"]);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/1.png"))).toBe(false);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/2.png"))).toBe(true);
  });
});

describe("reverse store · addLocalImage（文件对话框）", () => {
  it("选到路径 → 走 addLocalImagePath；取消 → 不动", async () => {
    const rev = useReverse();
    vi.mocked(api.reversePickLocalImage).mockResolvedValueOnce("/a/pick.jpg").mockResolvedValueOnce(null);
    await rev.addLocalImage();
    expect(rev.localImages).toEqual(["/a/pick.jpg"]);
    await rev.addLocalImage();
    expect(rev.localImages).toEqual(["/a/pick.jpg"]);
    expect(useToast().items).toEqual([]);
  });
  it("对话框抛错 → 错误 toast，列表不变", async () => {
    const rev = useReverse();
    vi.mocked(api.reversePickLocalImage).mockRejectedValueOnce({ code: "io", message: "对话框失败" });
    await rev.addLocalImage();
    expect(rev.localImages).toEqual([]);
    const items = useToast().items;
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe("error");
    expect(items[0].message).toContain("选择本地图片失败");
  });
});

describe("reverse store · addDroppedPaths（拖放）", async () => {
  it("图片逐个加入并勾选，非图片只提示一次", async () => {
    const rev = useReverse();
    const toast = useToast();
    const r = await rev.addDroppedPaths(["/a/1.png", "/a/notes.txt", "/a/2.JPG", "/a/dir", "/a/1.png"]);
    expect(r.images).toEqual(["/a/1.png", "/a/2.JPG"]);
    expect(r.others).toEqual(["/a/notes.txt", "/a/dir"]);
    expect(rev.localImages).toEqual(["/a/1.png", "/a/2.JPG"]);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/1.png"))).toBe(true);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/2.JPG"))).toBe(true);
    expect(toast.items).toHaveLength(1);
    expect(toast.items[0].message).toBe(NON_IMAGE_TOAST);
    expect(toast.items[0].kind).toBe("info");
  });
  it("全是图片 → 不提示；已在列表里的只勾选不重复加", async () => {
    const rev = useReverse();
    await rev.addLocalImagePath("/a/1.png");
    rev.toggleQuestion(localQuestionId("/a/1.png"));
    await rev.addDroppedPaths(["/a/1.png", "/a/3.webp"]);
    expect(rev.localImages).toEqual(["/a/1.png", "/a/3.webp"]);
    expect(rev.selectedQuestionIds.has(localQuestionId("/a/1.png"))).toBe(true);
    expect(useToast().items).toEqual([]);
  });
  it("全是非图片 → 提示一次，列表不变；空数组 → 什么都不做", async () => {
    const rev = useReverse();
    const toast = useToast();
    await rev.addDroppedPaths(["/a/x.txt", "/a/y.pdf"]);
    expect(rev.localImages).toEqual([]);
    expect(toast.items).toHaveLength(1);
    await rev.addDroppedPaths([]);
    expect(rev.localImages).toEqual([]);
    expect(toast.items).toHaveLength(1);
  });
});

describe("reverse store · 只看标签题（§11.4）", () => {
  it("visibleQuestions 隐藏散文题（后端 prose 键或 truth_preview 退化判定）、proseCount 计数、全选只勾可见的", () => {
    const rev = useReverse();
    rev.questions = [
      question("a"),
      question("b", { prose: true }),
      question("c", { truth_preview: "artist:x, <artwork>, <scene>, a girl stands by the window in soft morning light" }),
      question("d", { prose: false, truth_preview: "<artwork>, <scene>" }),
    ];
    expect(rev.proseCount).toBe(2);
    expect(rev.visibleQuestions.map((q) => q.entry_id)).toEqual(["a", "b", "c", "d"]);
    rev.setQuestionFilter({ tags_only: true });
    expect(rev.visibleQuestions.map((q) => q.entry_id)).toEqual(["a", "d"]);
    rev.selectAllQuestions();
    expect([...rev.selectedQuestionIds].sort()).toEqual(["a#0", "d#0"]);
    // 纯前端过滤：只改 tags_only 不重拉题库
    expect(api.reverseQuestions).not.toHaveBeenCalled();
  });
  it("其它筛选键照旧重拉；tags_only 一起改也只拉一次", async () => {
    const rev = useReverse();
    vi.mocked(api.reverseQuestions).mockResolvedValue([question("z")]);
    rev.setQuestionFilter({ keyword: "x", tags_only: true });
    await Promise.resolve();
    expect(api.reverseQuestions).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.reverseQuestions).mock.calls[0][0]).not.toHaveProperty("tags_only");
    expect(rev.questionFilter.tags_only).toBe(true);
  });
});

describe("reverse store · runBatch 带重画四键（§11.4）+ 画师来源 / 张数（§12.6）", () => {
  async function arm(rev: ReturnType<typeof useReverse>) {
    rev.templateId = 1;
    rev.rulesetId = 2;
    rev.providerIds = [3];
    await rev.addLocalImagePath("/a/1.png");
    vi.mocked(api.reverseRunBatch).mockResolvedValue({ batch_id: "b1" });
  }
  it("不勾重画：render=false、其余 null（render_artist_mode 仍发契约缺省 pool，style_* 发契约缺省）；note 空 → undefined", async () => {
    const rev = useReverse();
    await arm(rev);
    expect(rev.canRun).toBe(true);
    await rev.runBatch("  ");
    expect(api.reverseRunBatch).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.reverseRunBatch).mock.calls[0][0]).toEqual({
      question_ids: ["local:/a/1.png"], template_id: 1, ruleset_id: 2, provider_ids: [3], note: undefined,
      render: false, render_character: null, render_seed: null, judge_provider_id: null, artist: null, render_artist_mode: "pool", render_count: null, ...SD,
    });
    expect(rev.currentBatchId).toBe("b1");
    expect(rev.running).toBe(true);
  });
  it("勾了重画：角色 / 固定种子 / 评委 / 画师来源 / 张数按表单送出，空评委 → null，artist 恒 null", async () => {
    const rev = useReverse();
    await arm(rev);
    rev.renderForm = { render: true, character: " 阿莉亚 ", seed: "42", judge_provider_id: "", artist_mode: "none", count: 3, style: defaultStyleRenderForm() };
    await rev.runBatch("对照");
    const req = vi.mocked(api.reverseRunBatch).mock.calls[0][0];
    expect(req.note).toBe("对照");
    expect(req.render).toBe(true);
    expect(req.render_character).toBe("阿莉亚");
    expect(req.render_seed).toBe(42);
    expect(req.judge_provider_id).toBeNull();
    expect(req.artist).toBeNull();
    expect(req.render_artist_mode).toBe("none");
    expect(req.render_count).toBe(3);
    expect(req).toMatchObject(SD);
  });
  it("§12.7 精准反推：来源 style 时六个 style_* 键按子表单送出（空 / 全勾决策 → []，门槛空 → null）", async () => {
    const rev = useReverse();
    await arm(rev);
    rev.renderForm = { render: true, character: "", seed: "", judge_provider_id: "", artist_mode: "style", count: null, style: { model: null, top_k: 8, decisions: ["include", "undecided"], min_score: "0.35", fallback: "none", center: false } };
    await rev.runBatch("");
    expect(vi.mocked(api.reverseRunBatch).mock.calls[0][0]).toMatchObject({
      render: true, render_artist_mode: "style", render_count: null, artist: null,
      style_model: null, style_top_k: 8, style_decisions: ["include", "undecided"], style_min_score: 0.35, style_fallback: "none", style_center: false,
    });
    // 第一批还没收到终止事件（running=true）：先复位再发第二批
    rev.running = false;
    rev.renderForm.style.decisions = [];
    rev.renderForm.style.min_score = "";
    await rev.runBatch("");
    expect(vi.mocked(api.reverseRunBatch).mock.calls[1][0]).toMatchObject({ style_decisions: [], style_min_score: null, style_model: null, style_top_k: 8 });
  });
});

describe("reverse store · 停止批次与异步状态恢复", () => {
  function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<T>((a, b) => { resolve = a; reject = b; });
    return { promise, resolve, reject };
  }
  function arm() {
    const rev = useReverse();
    rev.templateId = 1;
    rev.rulesetId = 2;
    rev.providerIds = [3, 4];
    rev.selectedQuestionIds = new Set(["e1#0", "e2#0"]);
    vi.mocked(api.reverseRunBatch).mockResolvedValue({ batch_id: "b1" });
    vi.mocked(api.reverseBatchResults).mockResolvedValue(batchResults([runRow()]));
    return rev;
  }
  function emit(status: RunEvent["status"], done = 0, batchId = "b1", call = 0) {
    vi.mocked(api.reverseRunBatch).mock.calls[call][1].onmessage({
      batch_id: batchId, status, run_id: null, done, total: 4, message: null,
    });
  }

  it("停止请求只发一次；确认前保持运行，取消终态保留进度与结果并提示已停止", async () => {
    const rev = arm();
    await rev.runBatch("");
    emit("run_done", 1);
    const stop = deferred<void>();
    vi.mocked(api.reverseCancelBatch).mockReturnValueOnce(stop.promise);
    const request = rev.cancelBatch();
    await rev.cancelBatch();
    expect(api.reverseCancelBatch).toHaveBeenCalledExactlyOnceWith("b1");
    expect(rev.cancelling).toBe(true);
    expect(rev.running).toBe(true);
    stop.resolve(undefined);
    await request;
    expect(rev.cancelling).toBe(true);
    expect(rev.canRun).toBe(false);
    emit("batch_cancelled", 1);
    await Promise.resolve();
    expect(rev.running).toBe(false);
    expect(rev.cancelling).toBe(false);
    expect(rev.activeBatchId).toBeNull();
    expect(rev.runProgress).toMatchObject({ status: "batch_cancelled", done: 1, total: 4 });
    expect(rev.results?.runs).toHaveLength(1);
    expect(useToast().items).toHaveLength(1);
    expect(useToast().items[0]).toMatchObject({ kind: "info", message: "批次已停止：1/4，已完成结果已保存" });
  });

  it("停止请求失败恢复可重试状态，批次仍在运行", async () => {
    const rev = arm();
    await rev.runBatch("");
    vi.mocked(api.reverseCancelBatch).mockRejectedValueOnce(new Error("连接中断"));
    await rev.cancelBatch();
    expect(rev.running).toBe(true);
    expect(rev.cancelling).toBe(false);
    expect(rev.activeBatchId).toBe("b1");
    expect(useToast().items[0].message).toContain("停止批次失败，可重试");
    await rev.cancelBatch();
    expect(api.reverseCancelBatch).toHaveBeenCalledTimes(2);
  });

  it("启动返回前可用 started 事件的 ID 停止；启动返回不会覆盖终态", async () => {
    const rev = arm();
    const start = deferred<{ batch_id: string }>();
    vi.mocked(api.reverseRunBatch).mockReturnValueOnce(start.promise);
    const request = rev.runBatch("");
    expect(rev.running).toBe(true);
    expect(rev.activeBatchId).toBeNull();
    await rev.cancelBatch();
    expect(api.reverseCancelBatch).not.toHaveBeenCalled();
    emit("started");
    expect(rev.activeBatchId).toBe("b1");
    await rev.cancelBatch();
    expect(api.reverseCancelBatch).toHaveBeenCalledWith("b1");
    emit("batch_cancelled");
    await rev.selectBatch("history");
    start.resolve({ batch_id: "b1" });
    await request;
    expect(rev.running).toBe(false);
    expect(rev.activeBatchId).toBeNull();
    expect(rev.currentBatchId).toBe("history");
  });

  it("无 started 事件的极速终态也能结束，重复或迟到事件不改结果", async () => {
    const rev = arm();
    const start = deferred<{ batch_id: string }>();
    vi.mocked(api.reverseRunBatch).mockReturnValueOnce(start.promise);
    const request = rev.runBatch("");
    emit("batch_cancelled", 1);
    start.resolve({ batch_id: "b1" });
    await request;
    emit("batch_done", 4);
    emit("run_done", 3);
    expect(rev.runProgress?.status).toBe("batch_cancelled");
    expect(rev.running).toBe(false);
    expect(useToast().items).toHaveLength(1);
  });

  it("浏览历史和刷新历史不会误判运行结束，停止始终针对正在执行的批次", async () => {
    const rev = arm();
    await rev.runBatch("");
    const history = { ...batchResults([]), batch: { ...batchResults([]).batch, batch_id: "history" } };
    vi.mocked(api.reverseBatchResults).mockResolvedValue(history);
    await rev.selectBatch("history");
    vi.mocked(api.reverseBatchesList).mockResolvedValue([history.batch, { ...history.batch, batch_id: "b1", status: "running" }]);
    await rev.loadBatches();
    expect(rev.running).toBe(true);
    expect(rev.activeBatchId).toBe("b1");
    await rev.cancelBatch();
    expect(api.reverseCancelBatch).toHaveBeenCalledWith("b1");
    emit("batch_cancelled", 1);
    await Promise.resolve();
    expect(rev.currentBatchId).toBe("history");
    expect(rev.results?.batch.batch_id).toBe("history");
  });

  it("等待启动期间选择历史，迟到的启动响应不会切回正在运行的批次", async () => {
    const rev = arm();
    const start = deferred<{ batch_id: string }>();
    vi.mocked(api.reverseRunBatch).mockReturnValueOnce(start.promise);
    const request = rev.runBatch("");
    await rev.selectBatch("history");
    start.resolve({ batch_id: "b1" });
    await request;
    expect(rev.currentBatchId).toBe("history");
    expect(rev.activeBatchId).toBe("b1");
  });

  it("取消终止事件丢失后按数据库恢复进度和操作按钮，不显示完成成功", async () => {
    const rev = arm();
    await rev.runBatch("");
    await rev.cancelBatch();
    const saved = { ...batchResults([runRow(), runRow({ run_id: 2 })]), batch: { ...batchResults([]).batch, status: "cancelled" } };
    vi.mocked(api.reverseBatchesList).mockResolvedValue([saved.batch]);
    vi.mocked(api.reverseBatchResults).mockResolvedValue(saved);
    await rev.loadBatches();
    expect(rev.running).toBe(false);
    expect(rev.cancelling).toBe(false);
    expect(rev.runProgress).toMatchObject({ status: "batch_cancelled", done: 2, total: 4 });
    expect(rev.results?.runs).toHaveLength(2);
    emit("batch_cancelled", 2);
    expect(useToast().items).toHaveLength(1);
    expect(useToast().items[0].kind).toBe("info");
  });

  it("轮询恢复时保留没有结果行的既有进度，不被较少的已保存行数覆盖", async () => {
    const rev = arm();
    await rev.runBatch("");
    emit("run_failed", 3);
    vi.mocked(api.reverseBatchesList).mockResolvedValue([{ ...batchResults([]).batch, status: "cancelled" }]);
    await rev.loadBatches();
    expect(rev.runProgress).toMatchObject({ status: "batch_cancelled", done: 3, total: 4 });
  });

  it("旧批次的事件和未返回的停止失败不能影响下一批次", async () => {
    const rev = arm();
    await rev.runBatch("");
    const stop = deferred<void>();
    vi.mocked(api.reverseCancelBatch).mockReturnValueOnce(stop.promise);
    const request = rev.cancelBatch();
    emit("batch_cancelled", 1);
    vi.mocked(api.reverseRunBatch).mockResolvedValueOnce({ batch_id: "b2" });
    await rev.runBatch("");
    emit("batch_done", 4);
    emit("run_done", 2, "wrong", 1);
    stop.reject(new Error("旧请求超时"));
    await request;
    expect(rev.running).toBe(true);
    expect(rev.activeBatchId).toBe("b2");
    expect(rev.cancelling).toBe(false);
    expect(rev.runProgress).toBeNull();
    expect(useToast().items).toHaveLength(1);
  });

  it("终止前自然完成沿用完成提示，迟到的停止失败不覆盖完成", async () => {
    const rev = arm();
    await rev.runBatch("");
    const stop = deferred<void>();
    vi.mocked(api.reverseCancelBatch).mockReturnValueOnce(stop.promise);
    const request = rev.cancelBatch();
    emit("batch_done", 4);
    stop.reject(new Error("请求超时"));
    await request;
    expect(rev.runProgress).toMatchObject({ status: "batch_done", done: 4, total: 4 });
    expect(useToast().items).toHaveLength(1);
    expect(useToast().items[0].kind).toBe("success");
  });

  it("启动失败可重试；若已收到 started 则保留停止入口等待后台确认", async () => {
    const rev = arm();
    vi.mocked(api.reverseRunBatch).mockRejectedValueOnce(new Error("启动失败"));
    await rev.runBatch("");
    expect(rev.running).toBe(false);
    expect(rev.activeBatchId).toBeNull();
    const start = deferred<{ batch_id: string }>();
    vi.mocked(api.reverseRunBatch).mockReturnValueOnce(start.promise);
    const request = rev.runBatch("");
    emit("started", 0, "b1", 1);
    start.reject(new Error("响应丢失"));
    await request;
    expect(rev.running).toBe(true);
    expect(rev.activeBatchId).toBe("b1");
    await rev.cancelBatch();
    expect(api.reverseCancelBatch).toHaveBeenCalledWith("b1");
  });
});

describe("reverse store · rerender（§11.4 单条重画）", () => {
  it("成功：返回的行换进结果表与打开的详情，成功 toast，随后静默重拉结果", async () => {
    const rev = useReverse();
    const a = runRow({ run_id: 1 });
    const b = runRow({ run_id: 2 });
    rev.currentBatchId = "b1";
    rev.results = batchResults([a, b]);
    rev.runDetail = detail(a);
    rev.renderForm = { render: false, character: "薇拉", seed: "", judge_provider_id: " gpt-4o ", artist_mode: "pool", count: null, style: defaultStyleRenderForm() };
    const a2 = runRow({ run_id: 1, render_path: "/d/labruns/reverse/1.jpg", render_seed: 7, quality_score: 7.5, similarity_score: 8 });
    vi.mocked(api.reverseRenderRun).mockResolvedValue(a2);
    vi.mocked(api.reverseBatchResults).mockResolvedValue(batchResults([a2, b]));
    const out = await rev.rerender(1);
    expect(out).toBe(a2);
    expect(api.reverseRenderRun).toHaveBeenCalledWith(1, "gpt-4o", null, "pool", null, SD);
    expect(rev.results?.runs[0]).toEqual(a2);
    expect(rev.results?.runs[1]).toEqual(b);
    expect(rev.runDetail?.run).toEqual(a2);
    expect(api.reverseBatchResults).toHaveBeenCalledWith("b1");
    expect(rev.rerenderingRunId).toBeNull();
    const items = useToast().items;
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe("success");
  });
  it("插件把失败写在 render_error 里：行照换、按失败提示；命令本身抛错：行不动、错误 toast", async () => {
    const rev = useReverse();
    const a = runRow({ run_id: 1 });
    rev.currentBatchId = "b1";
    rev.results = batchResults([a]);
    vi.mocked(api.reverseRenderRun).mockResolvedValueOnce(runRow({ run_id: 1, render_error: "插件不可达" }));
    vi.mocked(api.reverseBatchResults).mockRejectedValue(new Error("offline"));
    await rev.rerender(1);
    expect(rev.results?.runs[0].render_error).toBe("插件不可达");
    let items = useToast().items;
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe("error");
    expect(items[0].message).toContain("插件不可达");

    vi.mocked(api.reverseRenderRun).mockRejectedValueOnce(new Error("run 不存在"));
    const out = await rev.rerender(1);
    expect(out).toBeNull();
    expect(rev.results?.runs[0].render_error).toBe("插件不可达");
    items = useToast().items;
    expect(items).toHaveLength(2);
    expect(items[1].message).toContain("run 不存在");
    expect(rev.rerenderingRunId).toBeNull();
  });
  it("同一时刻只放一个重画：进行中再叫直接返回 null，不发第二次请求", async () => {
    const rev = useReverse();
    rev.results = batchResults([runRow({ run_id: 1 }), runRow({ run_id: 2 })]);
    let resolve!: (r: RunRow) => void;
    vi.mocked(api.reverseRenderRun).mockReturnValueOnce(new Promise<RunRow>((r) => { resolve = r; }));
    const first = rev.rerender(1);
    expect(rev.rerenderingRunId).toBe(1);
    expect(await rev.rerender(2)).toBeNull();
    expect(api.reverseRenderRun).toHaveBeenCalledTimes(1);
    resolve(runRow({ run_id: 1, render_path: "/1.jpg" }));
    await first;
    expect(rev.rerenderingRunId).toBeNull();
  });
});

describe("reverse store · currentRun 与带画师串的重画（§12.5）", () => {
  it("openRun 记住当前 run；closeRun 不清；同批次不清、换批次清；新批次启动也清", async () => {
    const rev = useReverse();
    vi.mocked(api.reverseRunGet).mockResolvedValue(detail(runRow({ run_id: 5 })));
    vi.mocked(api.reverseBatchResults).mockResolvedValue(batchResults([]));
    rev.currentBatchId = "b1";
    await rev.openRun(5);
    expect(rev.currentRun?.run_id).toBe(5);
    rev.closeRun();
    expect(rev.runDetail).toBeNull();
    expect(rev.currentRun?.run_id).toBe(5);
    await rev.selectBatch("b1");
    expect(rev.currentRun?.run_id).toBe(5);
    await rev.selectBatch("b2");
    expect(rev.currentRun).toBeNull();
  });
  it("rerender(runId, artist, \"given\") 把画师串与 given 发出去；不给时沿用表单的来源与张数；返回的行同时换掉 currentRun", async () => {
    const rev = useReverse();
    const a = runRow({ run_id: 1 });
    rev.results = batchResults([a]);
    rev.currentRun = a;
    rev.renderForm = { render: true, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "none", count: 2, style: defaultStyleRenderForm() };
    const a2 = runRow({ run_id: 1, render_path: "/1.jpg", render_artist: "artist:foo", render_artist_source: "given" });
    vi.mocked(api.reverseRenderRun).mockResolvedValue(a2);
    await rev.rerender(1, "artist:foo", "given");
    expect(api.reverseRenderRun).toHaveBeenCalledWith(1, null, "artist:foo", "given", 2, SD);
    expect(rev.currentRun).toEqual(a2);
    expect(rev.results?.runs[0]).toEqual(a2);
    // 单题面板的"重画"：与批次配置相同（none / 2 张），画师串不送
    await rev.rerender(1);
    expect(api.reverseRenderRun).toHaveBeenLastCalledWith(1, null, null, "none", 2, SD);
    // 只给画师串不给模式 = given（老调用方式）
    await rev.rerender(1, "artist:bar");
    expect(api.reverseRenderRun).toHaveBeenLastCalledWith(1, null, "artist:bar", "given", 2, SD);
    expect(api.reverseBatchResults).not.toHaveBeenCalled();
  });
  it("§12.6：given 却是空串 → 退回表单的来源、不送画师串；表单改回 pool / 插件默认张数后重画也跟着变", async () => {
    const rev = useReverse();
    rev.results = batchResults([runRow({ run_id: 3 })]);
    rev.renderForm = { render: true, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "none", count: 4, style: defaultStyleRenderForm() };
    vi.mocked(api.reverseRenderRun).mockResolvedValue(runRow({ run_id: 3, render_path: "/3.jpg" }));
    await rev.rerender(3, "   ", "given");
    expect(api.reverseRenderRun).toHaveBeenLastCalledWith(3, null, null, "none", 4, SD);
    rev.renderForm = { ...rev.renderForm, artist_mode: "pool", count: null };
    await rev.rerender(3);
    expect(api.reverseRenderRun).toHaveBeenLastCalledWith(3, null, null, "pool", null, SD);
  });
  it("§12.7：表单来源是 style 时单题面板“重画”带子表单的 style_*，画风反推“用它重画”（given）仍发缺省；返回行的命中字段照换", async () => {
    const rev = useReverse();
    const a = runRow({ run_id: 4 });
    rev.results = batchResults([a]);
    rev.currentRun = a;
    rev.renderForm = { render: true, character: "薇拉", seed: "", judge_provider_id: "", artist_mode: "style", count: 1, style: { model: null, top_k: 3, decisions: ["include"], min_score: "0.3", fallback: "none", center: true } };
    const hit = runRow({ run_id: 4, render_path: "/4.jpg", render_artist: "artist:foo", render_artist_source: "style", render_style_entry_id: "e9", render_style_title: "画师甲", render_style_score: 0.51, render_style_decision: "include" });
    vi.mocked(api.reverseRenderRun).mockResolvedValue(hit);
    await rev.rerender(4);
    expect(api.reverseRenderRun).toHaveBeenLastCalledWith(4, null, null, "style", 1, { style_model: null, style_top_k: 3, style_decisions: ["include"], style_min_score: 0.3, style_fallback: "none", style_center: true });
    expect(rev.results?.runs[0].render_style_entry_id).toBe("e9");
    expect(rev.currentRun?.render_artist_source).toBe("style");
    await rev.rerender(4, "artist:bar", "given");
    expect(api.reverseRenderRun).toHaveBeenLastCalledWith(4, null, "artist:bar", "given", 1, SD);
  });
});
