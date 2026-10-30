import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const { afterSync, refreshStyleStatus } = vi.hoisted(() => ({ afterSync: vi.fn(), refreshStyleStatus: vi.fn() }));
vi.mock("./codex", () => ({ useCodex: () => ({ afterSync }) }));
vi.mock("./style", () => ({ useStyle: () => ({ refreshStatus: refreshStyleStatus }) }));
vi.mock("../api", () => ({
  api: { syncStart: vi.fn(), syncStatus: vi.fn(), syncLogs: vi.fn(), syncCancel: vi.fn() },
  errMessage: (e: unknown) => e instanceof Error ? e.message : String(e),
  Channel: class { onmessage: ((e: unknown) => void) | null = null; },
}));

import { api } from "../api";
import type { SyncEvent, SyncStatus } from "../types";
import { timingView } from "../lib/index-timing";
import { useSync } from "./sync";
import { useToast } from "./toast";

type Ch = { onmessage: ((e: SyncEvent) => void) | null };
let clock = 0;

function status(extra: Partial<SyncStatus> = {}): SyncStatus {
  return { running: true, phase: "images", release_local: null, release_remote: null, images_total: 100, images_done: 0, images_failed: 0, message: "", ...extra };
}
function event(extra: Partial<SyncEvent> = {}): SyncEvent {
  return { phase: "embedding", message: "", release: null, added: 0, changed: 0, images_total: 100, images_done: 100, images_failed: 0, embed_total: 100, embed_done: 0, embed_failed: 0, ...extra };
}

beforeEach(() => {
  setActivePinia(createPinia());
  clock = 0;
  vi.spyOn(performance, "now").mockImplementation(() => clock);
  vi.mocked(api.syncStart).mockReset();
  vi.mocked(api.syncStatus).mockReset().mockResolvedValue(status());
  vi.mocked(api.syncLogs).mockReset().mockResolvedValue([]);
  vi.mocked(api.syncCancel).mockReset();
  afterSync.mockReset();
  refreshStyleStatus.mockReset();
});
afterEach(() => vi.restoreAllMocks());

describe("sync store · 画风索引计时", () => {
  it("一个来源失败时保留完成提示、显示原因并刷新成功来源的书目", async () => {
    const st = useSync();
    let ch: Ch | null = null;
    vi.mocked(api.syncStart).mockImplementation(async (_force, c) => { ch = c as unknown as Ch; });
    await st.start(false);
    const message = "NAX 同步失败：网络中断；已保留本地缓存，法典同步完成";
    vi.mocked(api.syncStatus).mockResolvedValue(status({ running: false, phase: "done", message }));
    ch!.onmessage?.(event({ phase: "done", added: 2, changed: 1, message }));
    await st.refresh();

    expect(st.lastDone?.message).toBe(message);
    expect(st.running).toBe(false);
    expect(afterSync).toHaveBeenCalledTimes(1);
    expect(useToast().items).toContainEqual(expect.objectContaining({ kind: "error", message: expect.stringContaining(message) }));
    expect(useToast().items.some((item) => item.kind === "success")).toBe(false);
  });

  it("只在首个完成索引批次和终止时刷新计算设备，轮询兜底不重复刷新", async () => {
    const st = useSync();
    let ch: Ch | null = null;
    vi.mocked(api.syncStart).mockImplementation(async (_force, c) => { ch = c as unknown as Ch; });
    await st.start(false);
    ch!.onmessage?.(event({ phase: "images" }));
    ch!.onmessage?.(event());
    expect(refreshStyleStatus).not.toHaveBeenCalled();
    ch!.onmessage?.(event({ embed_done: 15, embed_failed: 1 }));
    expect(refreshStyleStatus).toHaveBeenCalledTimes(1);
    ch!.onmessage?.(event({ embed_done: 31, embed_failed: 1 }));
    expect(refreshStyleStatus).toHaveBeenCalledTimes(1);
    vi.mocked(api.syncStatus).mockResolvedValue(status({ running: false, phase: "done" }));
    ch!.onmessage?.(event({ phase: "done", embed_done: 99, embed_failed: 1 }));
    await st.refresh();
    await st.refresh();
    expect(refreshStyleStatus).toHaveBeenCalledTimes(2);
  });

  it("图片下载再久也不算进索引耗时；仅 embedding 开始计时，完成后冻结", async () => {
    const st = useSync();
    let ch: Ch | null = null;
    vi.mocked(api.syncStart).mockImplementation(async (_force, c) => { ch = c as unknown as Ch; });
    clock = 1000;
    await st.start(false);
    expect(st.indexTiming).toBeNull();
    clock = 181_000;
    ch!.onmessage?.(event({ phase: "images", images_done: 90, embed_total: 0 }));
    expect(st.indexTiming).toBeNull();

    clock = 182_000;
    ch!.onmessage?.(event());
    expect(st.indexTiming?.startedAt).toBe(182_000);
    clock = 202_000;
    ch!.onmessage?.(event({ embed_done: 16 }));
    expect(timingView(st.indexTiming!, clock).remainingSeconds).toBeNull();
    clock = 212_000;
    ch!.onmessage?.(event({ embed_done: 30, embed_failed: 2 }));
    expect(timingView(st.indexTiming!, clock)).toMatchObject({
      phase: "running", elapsedSeconds: 30, itemsPerMinute: 96, remainingSeconds: 42.5,
    });

    vi.mocked(api.syncStatus).mockResolvedValue(status({ running: false, phase: "done" }));
    clock = 250_000;
    ch!.onmessage?.(event({ phase: "done", embed_done: 98, embed_failed: 2 }));
    expect(st.channelActive).toBe(false);
    expect(timingView(st.indexTiming!, 999_000)).toMatchObject({ phase: "finished", elapsedSeconds: 68, remainingSeconds: null });
    await st.refresh();
    expect(afterSync).toHaveBeenCalledTimes(1);
  });

  it("终态事件丢失时轮询也能终止计时，下一轮同步清除旧估时", async () => {
    const st = useSync();
    let ch: Ch | null = null;
    vi.mocked(api.syncStart).mockImplementation(async (_force, c) => { ch = c as unknown as Ch; });
    await st.start(false);
    clock = 10_000;
    ch!.onmessage?.(event());
    clock = 20_000;
    ch!.onmessage?.(event({ embed_done: 16 }));
    clock = 30_000;
    ch!.onmessage?.(event({ embed_done: 32 }));
    expect(timingView(st.indexTiming!, clock).remainingSeconds).toBeGreaterThan(0);

    vi.mocked(api.syncStatus).mockResolvedValue(status({ running: false, phase: "done", message: "已取消" }));
    clock = 35_000;
    await st.refresh();
    expect(st.running).toBe(false);
    expect(st.progress?.message).toBe("已取消");
    expect(timingView(st.indexTiming!, 99_000)).toMatchObject({ phase: "finished", elapsedSeconds: 25, remainingSeconds: null });
    clock = 40_000;
    await st.refresh();
    expect(st.indexTiming?.finishedAt).toBe(35_000);
    expect(afterSync).toHaveBeenCalledTimes(1);

    vi.mocked(api.syncStatus).mockResolvedValue(status());
    await st.start(false);
    expect(st.indexTiming).toBeNull();
    clock = 100_000;
    ch!.onmessage?.(event({ embed_total: 68 }));
    expect(st.indexTiming?.startedAt).toBe(100_000);
    expect(timingView(st.indexTiming!, clock)).toMatchObject({ elapsedSeconds: 0, remainingSeconds: null, itemsPerMinute: null });
  });

  it("Channel 错误和启动异常都不能留下继续计时的索引任务", async () => {
    const st = useSync();
    let ch: Ch | null = null;
    vi.mocked(api.syncStart).mockImplementation(async (_force, c) => { ch = c as unknown as Ch; });
    await st.start(false);
    clock = 10_000;
    ch!.onmessage?.(event());
    vi.mocked(api.syncStatus).mockResolvedValue(status({ running: false, phase: "error" }));
    clock = 15_000;
    ch!.onmessage?.(event({ phase: "error", message: "编码失败" }));
    expect(timingView(st.indexTiming!, 99_000)).toMatchObject({ phase: "finished", elapsedSeconds: 5 });
    await st.refresh();
    expect(afterSync).not.toHaveBeenCalled();

    vi.mocked(api.syncStart).mockImplementationOnce(async (_force, c) => {
      clock = 20_000;
      (c as unknown as Ch).onmessage?.(event());
      clock = 23_000;
      throw new Error("连接中断");
    });
    await st.start(false);
    expect(st.running).toBe(false);
    expect(st.starting).toBe(false);
    expect(timingView(st.indexTiming!, 99_000)).toMatchObject({ phase: "finished", elapsedSeconds: 3 });
  });
});
