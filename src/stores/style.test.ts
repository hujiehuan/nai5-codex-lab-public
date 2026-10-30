// style store（CONTRACT §12.2 / §12.5）：检索成功 / bad_request 走"去设置页"、下载事件与结束状态、重建索引靠 Channel 收尾。
// 不依赖 DOM：mock 掉 ../api（Channel 是能挂 onmessage 的壳），用 createPinia + setActivePinia 在 node 环境直接跑（同 reverse.test.ts）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

vi.mock("../api", () => ({
  api: {
    styleModelStatus: vi.fn(),
    styleModelsList: vi.fn(),
    styleModelSelect: vi.fn(),
    styleClassifiersList: vi.fn(),
    styleClassifierDownload: vi.fn(),
    styleClassify: vi.fn(),
    styleModelDownload: vi.fn(),
    styleModelDownloadCancel: vi.fn(),
    styleIndexRebuild: vi.fn(),
    styleSearch: vi.fn(),
    syncCancel: vi.fn(),
  },
  errMessage: (e: unknown) => (typeof e === "object" && e !== null && "message" in e ? String((e as { message: unknown }).message) : String(e)),
  Channel: class { onmessage: ((e: unknown) => void) | null = null; },
  convertFileSrc: (p: string) => p,
}));

import { api } from "../api";
import type { DownloadEvent, StyleClassifierStatus, StyleModelStatus, StyleSearchRequest, StyleSearchResult, SyncEvent } from "../types";
import { useStyle } from "./style";
import { useToast } from "./toast";
import { timingView } from "../lib/index-timing";

type Ch<T> = { onmessage: ((e: T) => void) | null };
let clock = 0;

beforeEach(() => {
  setActivePinia(createPinia());
  clock = 0;
  vi.spyOn(performance, "now").mockImplementation(() => clock);
  for (const fn of [api.styleModelStatus, api.styleModelsList, api.styleModelSelect, api.styleClassifiersList, api.styleClassifierDownload, api.styleClassify, api.styleModelDownload, api.styleModelDownloadCancel, api.styleIndexRebuild, api.styleSearch, api.syncCancel]) vi.mocked(fn).mockReset();
  vi.mocked(api.styleModelStatus).mockResolvedValue(status());
  vi.mocked(api.styleModelsList).mockResolvedValue([status()]);
  vi.mocked(api.styleClassifiersList).mockResolvedValue([]);
});
afterEach(() => vi.restoreAllMocks());

function status(extra: Partial<StyleModelStatus> = {}): StyleModelStatus {
  return { model: "clip-vit-base-patch32", label: "CLIP ViT-B/32", description: "CLIP", dim: 512, download_size_bytes: 605247071, default_center: true, dir: "/d/models/clip", present: true, size_bytes: 1, indexed: 10, total: 10, mean_ready: true, device_preference: "auto", active_device: "cpu", metal_available: false, device_message: null, ...extra };
}
function classifier(model = "kaloscope", extra: Partial<StyleClassifierStatus> = {}): StyleClassifierStatus {
  return { model, label: model, description: "artist classifier", present: true, runtime_ready: true, download_size_bytes: 100, dir: `/models/${model}`, ...extra };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const req: StyleSearchRequest = { source: { kind: "entry", entry_id: "e1", ordinal: 0 }, top_k: 10, codex_ids: [], decisions: [], center: true };
function indexEvent(extra: Partial<SyncEvent> = {}): SyncEvent {
  return { phase: "embedding", message: "", release: null, added: 0, changed: 0, images_total: 0, images_done: 0, images_failed: 0, embed_total: 100, embed_done: 0, embed_failed: 0, ...extra };
}

describe("style store · search", () => {
  it("成功：结果落下、错误清空、记住请求；不 toast", async () => {
    const st = useStyle();
    const res: StyleSearchResult = { model: "clip-vit-base-patch32", indexed: 10, query_ms: 3, hits: [] };
    vi.mocked(api.styleSearch).mockResolvedValue(res);
    await st.search(req);
    expect(api.styleSearch).toHaveBeenCalledWith(req);
    expect(st.result).toEqual(res);
    expect(st.searchError).toBeNull();
    expect(st.searchNeedsSetup).toBe(false);
    expect(st.lastRequest).toEqual(req);
    expect(st.searching).toBe(false);
    expect(useToast().items).toHaveLength(0);
  });
  it("bad_request（模型未下载 / 索引为空）：错误留在面板并标记去设置页；别的错误只留文本", async () => {
    const st = useStyle();
    vi.mocked(api.styleSearch).mockRejectedValueOnce({ code: "bad_request", message: "画风模型未下载" });
    await st.search(req);
    expect(st.result).toBeNull();
    expect(st.searchError).toBe("画风模型未下载");
    expect(st.searchNeedsSetup).toBe(true);

    vi.mocked(api.styleSearch).mockRejectedValueOnce({ code: "io", message: "读图失败" });
    await st.search(req);
    expect(st.searchError).toBe("读图失败");
    expect(st.searchNeedsSetup).toBe(false);
    st.clearSearch();
    expect(st.searchError).toBeNull();
    expect(useToast().items).toHaveLength(0);
  });
});

describe("style store · download / rebuild", () => {
  it("首个完成批次和终止时刷新实际设备，中间批次不重复查询；下一轮重新确认设备", async () => {
    const st = useStyle();
    const channels: Ch<SyncEvent>[] = [];
    vi.mocked(api.styleIndexRebuild).mockImplementation(async (_force, c) => { channels.push(c as unknown as Ch<SyncEvent>); });
    vi.mocked(api.styleModelStatus).mockResolvedValue(status({ active_device: "metal", metal_available: true }));
    await st.startRebuild(false);
    channels[0].onmessage?.(indexEvent());
    expect(api.styleModelStatus).not.toHaveBeenCalled();
    channels[0].onmessage?.(indexEvent({ embed_done: 16 }));
    await Promise.resolve();
    expect(st.status?.active_device).toBe("metal");
    expect(api.styleModelStatus).toHaveBeenCalledTimes(1);
    channels[0].onmessage?.(indexEvent({ embed_done: 32 }));
    expect(api.styleModelStatus).toHaveBeenCalledTimes(1);
    channels[0].onmessage?.(indexEvent({ phase: "done", embed_done: 100 }));
    expect(api.styleModelStatus).toHaveBeenCalledTimes(2);
    await st.startRebuild(false);
    channels[1].onmessage?.(indexEvent({ embed_failed: 16 }));
    expect(api.styleModelStatus).toHaveBeenCalledTimes(3);
  });

  it("下载：事件逐条落到 download，命令返回的状态直接用，成功 toast；抛错按失败提示并重读状态", async () => {
    const st = useStyle();
    const ev: DownloadEvent = { file: "model.safetensors", done_bytes: 1, total_bytes: 2, finished: false, error: null };
    vi.mocked(api.styleModelDownload).mockImplementation(async (ch) => {
      (ch as unknown as Ch<DownloadEvent>).onmessage?.(ev);
      return status({ present: true });
    });
    const p = st.startDownload();
    expect(st.downloading).toBe(true);
    await p;
    expect(st.download).toEqual(ev);
    expect(st.status?.present).toBe(true);
    expect(st.downloading).toBe(false);
    expect(useToast().items.at(-1)?.kind).toBe("success");

    vi.mocked(api.styleModelDownload).mockRejectedValueOnce({ code: "network", message: "断了" });
    vi.mocked(api.styleModelStatus).mockResolvedValue(status({ present: false }));
    await st.startDownload();
    expect(st.downloading).toBe(false);
    expect(useToast().items.at(-1)?.message).toContain("断了");
    expect(api.styleModelStatus).toHaveBeenCalled();
  });
  it("重建索引：命令立即返回，rebuilding 直到 done / error 才复位并重读状态；进行中再叫不发第二次", async () => {
    const st = useStyle();
    let ch: Ch<SyncEvent> | null = null;
    vi.mocked(api.styleIndexRebuild).mockImplementation(async (_force, c) => { ch = c as unknown as Ch<SyncEvent>; });
    vi.mocked(api.styleModelStatus).mockResolvedValue(status({ indexed: 3200, total: 3200 }));
    await st.startRebuild(true);
    expect(api.styleIndexRebuild).toHaveBeenCalledWith(true, expect.anything(), undefined);
    expect(st.rebuilding).toBe(true);
    await st.startRebuild(false);
    expect(api.styleIndexRebuild).toHaveBeenCalledTimes(1);

    const base = { message: "", release: null, added: 0, changed: 0, images_total: 0, images_done: 0, images_failed: 0 };
    ch!.onmessage?.({ ...base, phase: "embedding", embed_total: 3200, embed_done: 100, embed_failed: 0 });
    expect(st.rebuilding).toBe(true);
    expect(st.rebuild?.embed_done).toBe(100);
    ch!.onmessage?.({ ...base, phase: "done", embed_total: 3200, embed_done: 3200, embed_failed: 2 });
    expect(st.rebuilding).toBe(false);
    expect(useToast().items.at(-1)?.message).toBe("画风索引 3200 / 3200，失败 2 · 完成");
    await Promise.resolve();
    expect(api.styleModelStatus).toHaveBeenCalled();

    await st.cancelRebuild();
    expect(api.syncCancel).toHaveBeenCalled();
  });
});

describe("style store · 索引计时", () => {
  it("续算只估计本轮剩余条目，失败也计入进度；模型加载不污染批次速度", async () => {
    const st = useStyle();
    st.status = status({ indexed: 112, total: 3208 });
    let ch: Ch<SyncEvent> | null = null;
    vi.mocked(api.styleIndexRebuild).mockImplementation(async (_force, c) => { ch = c as unknown as Ch<SyncEvent>; });
    clock = 1000;
    await st.startRebuild(false);
    expect(st.rebuildTiming?.startedAt).toBe(1000);

    clock = 121_000;
    ch!.onmessage?.(indexEvent({ embed_total: 3096, embed_done: 16 }));
    expect(timingView(st.rebuildTiming!, clock).remainingSeconds).toBeNull();
    clock = 131_000;
    ch!.onmessage?.(indexEvent({ embed_total: 3096, embed_done: 31, embed_failed: 1 }));
    expect(timingView(st.rebuildTiming!, clock)).toMatchObject({
      phase: "running", elapsedSeconds: 130, itemsPerMinute: 96, remainingSeconds: 1915,
    });
  });

  it("取消请求等后端终态才停止计时，提示保留结果；重新开始不会沿用上一轮速度", async () => {
    const st = useStyle();
    const channels: Ch<SyncEvent>[] = [];
    vi.mocked(api.styleIndexRebuild).mockImplementation(async (_force, c) => { channels.push(c as unknown as Ch<SyncEvent>); });
    vi.mocked(api.styleModelStatus).mockResolvedValue(status({ indexed: 32, total: 100 }));
    clock = 1000;
    await st.startRebuild(false);
    clock = 11_000;
    channels[0].onmessage?.(indexEvent({ embed_done: 16 }));
    clock = 21_000;
    channels[0].onmessage?.(indexEvent({ embed_done: 32 }));
    await st.cancelRebuild();
    expect(st.rebuilding).toBe(true);
    expect(st.rebuildTiming?.finishedAt).toBeNull();

    clock = 26_000;
    channels[0].onmessage?.(indexEvent({ phase: "done", message: "已取消", embed_done: 32 }));
    expect(st.rebuilding).toBe(false);
    expect(useToast().items.at(-1)).toMatchObject({ kind: "info", message: "画风索引已取消，已完成结果已保留" });
    expect(timingView(st.rebuildTiming!, 999_000)).toMatchObject({ phase: "finished", elapsedSeconds: 25, remainingSeconds: null });

    clock = 31_000;
    await st.startRebuild(false);
    expect(st.rebuildTiming?.startedAt).toBe(31_000);
    expect(st.rebuildTiming?.finishedAt).toBeNull();
    expect(st.rebuild).toBeNull();
    expect(timingView(st.rebuildTiming!, clock)).toMatchObject({ elapsedSeconds: 0, remainingSeconds: null, itemsPerMinute: null });
    clock = 41_000;
    channels[1].onmessage?.(indexEvent({ embed_total: 68, embed_done: 16 }));
    expect(timingView(st.rebuildTiming!, clock).remainingSeconds).toBeNull();
  });

  it("启动失败和 Channel 错误都会冻结本轮已用时间", async () => {
    const st = useStyle();
    vi.mocked(api.styleIndexRebuild).mockImplementationOnce(async () => {
      clock = 7000;
      throw new Error("索引任务正在运行");
    });
    clock = 2000;
    await st.startRebuild(false);
    expect(st.rebuilding).toBe(false);
    expect(timingView(st.rebuildTiming!, 90_000)).toMatchObject({ phase: "finished", elapsedSeconds: 5 });
    expect(useToast().items.at(-1)?.kind).toBe("error");

    let ch: Ch<SyncEvent> | null = null;
    vi.mocked(api.styleIndexRebuild).mockImplementationOnce(async (_force, c) => { ch = c as unknown as Ch<SyncEvent>; });
    vi.mocked(api.styleModelStatus).mockResolvedValue(status());
    clock = 10_000;
    await st.startRebuild(false);
    clock = 19_000;
    ch!.onmessage?.(indexEvent({ phase: "error", message: "模型加载失败" }));
    expect(st.rebuilding).toBe(false);
    expect(timingView(st.rebuildTiming!, 90_000)).toMatchObject({ phase: "finished", elapsedSeconds: 9, remainingSeconds: null });
    expect(useToast().items.at(-1)?.message).toContain("模型加载失败");
  });
});

describe("style store · 多模型", () => {
  it("切换持久化成功才替换当前模型，失败与忙碌时保留当前模型", async () => {
    const st = useStyle();
    st.status = status();
    st.models = [status()];
    const mega = status({ model: "megastyle-siglip-so400m", label: "MegaStyle", dim: 1152 });
    vi.mocked(api.styleModelSelect).mockResolvedValueOnce(mega);
    await st.selectModel(mega.model);
    expect(st.status?.model).toBe(mega.model);
    expect(st.models.map((m) => m.model)).toEqual(["clip-vit-base-patch32", mega.model]);
    vi.mocked(api.styleModelSelect).mockRejectedValueOnce(new Error("同步正在运行"));
    await st.selectModel("clip-vit-base-patch32");
    expect(st.status?.model).toBe(mega.model);
    st.rebuilding = true;
    await st.selectModel("clip-vit-base-patch32");
    expect(api.styleModelSelect).toHaveBeenCalledTimes(2);
  });

  it("下载或索引指定模型不改变当前模型，任务携带明确模型 ID", async () => {
    const st = useStyle();
    st.status = status();
    st.models = [status(), status({ model: "csd-vit-large-patch14", present: false })];
    vi.mocked(api.styleModelDownload).mockResolvedValue(status({ model: "csd-vit-large-patch14", present: true }));
    await st.startDownload("csd-vit-large-patch14");
    expect(api.styleModelDownload).toHaveBeenCalledWith(expect.anything(), "csd-vit-large-patch14");
    expect(st.status?.model).toBe("clip-vit-base-patch32");
    expect(st.models[1].present).toBe(true);
    await st.startRebuild(false, "csd-vit-large-patch14");
    expect(api.styleIndexRebuild).toHaveBeenCalledWith(false, expect.anything(), "csd-vit-large-patch14");
    expect(st.rebuildModel).toBe("csd-vit-large-patch14");
    expect(st.status?.model).toBe("clip-vit-base-patch32");
  });

  it("对比顺序执行，冻结同图与参数，单组失败继续分类器且分数独立保存", async () => {
    const st = useStyle();
    st.models = [status(), status({ model: "megastyle-siglip-so400m", label: "MegaStyle" }), status({ model: "csd-vit-large-patch14", indexed: 0 })];
    st.classifiers = [classifier(), classifier("camie", { runtime_ready: false })];
    const first = deferred<StyleSearchResult>();
    vi.mocked(api.styleSearch).mockImplementationOnce(() => first.promise).mockRejectedValueOnce(new Error("MegaStyle 计算失败"));
    vi.mocked(api.styleClassify).mockResolvedValue({ model: "kaloscope", label: "Kaloscope", query_ms: 4, hits: [{ artist: "artist:sample", score: 0.92 }] });
    const input: StyleSearchRequest = { ...req, source: { kind: "path", path: "/same.png" }, decisions: ["include"], top_k: 2 };
    const pending = st.compare(input);
    expect(api.styleSearch).toHaveBeenCalledTimes(1);
    expect(st.comparisons.map((g) => g.phase)).toEqual(["searching", "queued"]);
    expect(api.styleClassify).not.toHaveBeenCalled();
    if (input.source.kind === "path") input.source.path = "/changed.png";
    input.top_k = 50;
    input.decisions.push("exclude");
    clock = 13;
    first.resolve({ model: "clip-vit-base-patch32", indexed: 10, query_ms: 12, hits: [] });
    await pending;
    expect(api.styleSearch).toHaveBeenNthCalledWith(2, expect.objectContaining({ model: "megastyle-siglip-so400m", source: { kind: "path", path: "/same.png" }, decisions: ["include"], top_k: 2 }));
    expect(api.styleClassify).toHaveBeenCalledWith({ model: "kaloscope", source: { kind: "path", path: "/same.png" }, top_k: 2 });
    expect(st.comparisons.map((g) => g.phase)).toEqual(["done", "error"]);
    expect(st.comparisons[0].elapsed_ms).toBe(13);
    expect(st.comparisons[1].error).toBe("MegaStyle 计算失败");
    expect(st.classifierComparisons[0].result?.hits).toEqual([{ artist: "artist:sample", score: 0.92 }]);
    expect(st.result).toBeNull();
    expect(st.searching).toBe(false);
  });

  it("只有分类器就绪也能对比，一组识别失败不阻止另一组", async () => {
    const st = useStyle();
    st.models = [status({ present: false })];
    st.classifiers = [classifier(), classifier("camie")];
    vi.mocked(api.styleClassify).mockRejectedValueOnce(new Error("分类器加载失败")).mockResolvedValueOnce({ model: "camie", label: "Camie", query_ms: 8, hits: [] });
    await st.compare(req);
    expect(api.styleSearch).not.toHaveBeenCalled();
    expect(st.classifierComparisons.map((g) => g.phase)).toEqual(["error", "done"]);
    expect(st.searchError).toBeNull();
  });

  it("下载全部只依次下载缺失模型与分类器，始终保留当前模型", async () => {
    const st = useStyle();
    st.status = status();
    const mega = status({ model: "megastyle-siglip-so400m", present: false });
    vi.mocked(api.styleModelsList).mockResolvedValue([status(), mega]);
    vi.mocked(api.styleClassifiersList).mockResolvedValue([classifier("kaloscope", { present: false }), classifier("camie")]);
    const first = deferred<StyleModelStatus>();
    vi.mocked(api.styleModelDownload).mockImplementation(() => first.promise);
    vi.mocked(api.styleClassifierDownload).mockResolvedValue(classifier());
    const pending = st.downloadAll();
    await vi.waitFor(() => expect(api.styleModelDownload).toHaveBeenCalledTimes(1));
    expect(st.busy).toBe(true);
    expect(api.styleClassifierDownload).not.toHaveBeenCalled();
    await st.startDownload("csd-vit-large-patch14");
    expect(api.styleModelDownload).toHaveBeenCalledTimes(1);
    first.resolve({ ...mega, present: true });
    await pending;
    expect(api.styleClassifierDownload).toHaveBeenCalledWith("kaloscope", expect.anything());
    expect(st.downloadQueue).toEqual(["megastyle-siglip-so400m", "kaloscope"]);
    expect(st.downloadPosition).toBe(2);
    expect(st.status?.model).toBe("clip-vit-base-patch32");
    expect(st.busy).toBe(false);
  });

  it("取消下载全部后即使当前下载返回成功也不启动下一模型", async () => {
    const st = useStyle();
    vi.mocked(api.styleModelsList).mockResolvedValue([status({ present: false }), status({ model: "megastyle-siglip-so400m", present: false })]);
    const first = deferred<StyleModelStatus>();
    vi.mocked(api.styleModelDownload).mockImplementation(() => first.promise);
    const pending = st.downloadAll();
    await vi.waitFor(() => expect(api.styleModelDownload).toHaveBeenCalledTimes(1));
    await st.cancelDownload();
    first.resolve(status());
    await pending;
    expect(api.styleModelDownload).toHaveBeenCalledTimes(1);
    expect(api.styleModelDownloadCancel).toHaveBeenCalledOnce();
    expect(st.busy).toBe(false);
  });

  it("分类器权重已在但运行时缺失仍可补齐，共享运行时恢复后不重复下载", async () => {
    const st = useStyle();
    vi.mocked(api.styleModelsList).mockResolvedValue([status()]);
    vi.mocked(api.styleClassifiersList).mockResolvedValueOnce([classifier("kaloscope", { runtime_ready: false }), classifier("camie", { runtime_ready: false })])
      .mockResolvedValue([classifier(), classifier("camie")]);
    vi.mocked(api.styleClassifierDownload).mockResolvedValue(classifier());
    await st.downloadAll();
    expect(api.styleModelDownload).not.toHaveBeenCalled();
    expect(api.styleClassifierDownload).toHaveBeenCalledTimes(1);
    expect(st.readyClassifiers).toHaveLength(2);
    expect(st.busy).toBe(false);
  });

  it("下载全部任一失败就停止后续任务", async () => {
    const st = useStyle();
    vi.mocked(api.styleModelsList).mockResolvedValue([status({ present: false }), status({ model: "megastyle-siglip-so400m", present: false })]);
    vi.mocked(api.styleModelDownload).mockRejectedValue(new Error("下载连接中断"));
    await st.downloadAll();
    expect(api.styleModelDownload).toHaveBeenCalledTimes(1);
    expect(api.styleClassifierDownload).not.toHaveBeenCalled();
    expect(st.busy).toBe(false);
  });
});
