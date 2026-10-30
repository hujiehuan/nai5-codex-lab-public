import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createCreativeImageLoader,
  CREATIVE_IMAGE_WAIT_MS,
  type CreativeImageState,
} from "./creative-image-loader";

const pending = { code: "creative_image_pending", message: "这张图片尚未生成" };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function setup(fetch: (runId: string, idx: number) => Promise<string>) {
  const states: Record<string, CreativeImageState> = {};
  const update = vi.fn((key: string, state: CreativeImageState) => {
    states[key] = state;
  });
  const loader = createCreativeImageLoader(fetch, update, (e) =>
    String((e as { message: string }).message),
  );
  return { ...loader, states, update };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("creative image publication", () => {
  it("keeps long-running scoring neutral and starts its 60-second deadline only when terminal", async () => {
    const fetch = vi.fn().mockRejectedValue(pending);
    const loader = setup(fetch);
    loader.ensure("run-a", 0, true);
    await vi.advanceTimersByTimeAsync(180_000);
    expect(loader.states["run-a:0"]).toEqual({ status: "pending" });
    expect(fetch.mock.calls.length).toBeGreaterThan(60);
    loader.ensure("run-a", 0, false);
    // A stale status cannot turn a completed run back into an unlimited wait.
    loader.ensure("run-a", 0, true);
    await vi.advanceTimersByTimeAsync(59_999);
    expect(loader.states["run-a:0"]).toEqual({ status: "pending" });
    await vi.advanceTimersByTimeAsync(1);
    expect(loader.states["run-a:0"]).toMatchObject({
      status: "error",
      error: expect.stringContaining("60 秒"),
    });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("retries temporary publication lag independently of completed run polling", async () => {
    const fetch = vi
      .fn()
      .mockRejectedValueOnce(pending)
      .mockResolvedValueOnce("image-a");
    const loader = setup(fetch);
    loader.ensure("run-a", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(loader.states["run-a:0"]).toEqual({ status: "pending" });
    // No more run status calls are required after the task becomes terminal.
    await vi.advanceTimersByTimeAsync(1999);
    expect(fetch).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(loader.states["run-a:0"]).toEqual({
      status: "ready",
      url: "image-a",
    });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("deduplicates status/history refreshes while fetching, waiting, and ready", async () => {
    const first = deferred<string>();
    const fetch = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue("image");
    const loader = setup(fetch);
    loader.ensure("run-a", 0);
    loader.ensure("run-a", 0);
    expect(fetch).toHaveBeenCalledTimes(1);
    first.reject(pending);
    await vi.advanceTimersByTimeAsync(0);
    loader.ensure("run-a", 0);
    loader.retry("run-a", 0);
    expect(fetch).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2000);
    loader.ensure("run-a", 0);
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it.each([
    "not_found",
    "plugin_rejected",
    "network",
    "creative_unsupported",
    "invalid_response",
  ])("shows %s immediately and requires an explicit retry", async (code) => {
    const fetch = vi
      .fn()
      .mockRejectedValueOnce({ code, message: "真实取图失败" })
      .mockResolvedValue("image");
    const loader = setup(fetch);
    loader.ensure("run-a", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(loader.states["run-a:0"]).toEqual({
      status: "error",
      error: "真实取图失败",
    });
    loader.ensure("run-a", 0);
    await vi.advanceTimersByTimeAsync(120_000);
    expect(fetch).toHaveBeenCalledTimes(1);
    loader.retry("run-a", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(loader.states["run-a:0"]).toEqual({ status: "ready", url: "image" });
  });

  it("bounds repeated pending responses to 60 seconds and resets on manual retry", async () => {
    const fetch = vi.fn().mockRejectedValue(pending);
    const loader = setup(fetch);
    loader.ensure("run-a", 0);
    await vi.advanceTimersByTimeAsync(CREATIVE_IMAGE_WAIT_MS);
    expect(loader.states["run-a:0"]).toMatchObject({
      status: "error",
      error: expect.stringContaining("60 秒"),
    });
    const count = fetch.mock.calls.length;
    await vi.advanceTimersByTimeAsync(CREATIVE_IMAGE_WAIT_MS);
    loader.ensure("run-a", 0);
    expect(fetch).toHaveBeenCalledTimes(count);
    fetch.mockResolvedValue("recovered");
    loader.retry("run-a", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(loader.states["run-a:0"]).toEqual({
      status: "ready",
      url: "recovered",
    });
  });

  it("does not overlap a timed-out in-flight image request with manual retry", async () => {
    const slow = deferred<string>();
    const fetch = vi
      .fn()
      .mockRejectedValueOnce(pending)
      .mockReturnValueOnce(slow.promise)
      .mockResolvedValue("fresh");
    const loader = setup(fetch);
    loader.ensure("run-a", 0);
    await vi.advanceTimersByTimeAsync(CREATIVE_IMAGE_WAIT_MS);
    expect(loader.states["run-a:0"].status).toBe("error");
    loader.retry("run-a", 0);
    expect(fetch).toHaveBeenCalledTimes(2);
    slow.resolve("stale");
    await vi.advanceTimersByTimeAsync(0);
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(loader.states["run-a:0"]).toEqual({ status: "ready", url: "fresh" });
    expect(
      loader.update.mock.calls.some(
        ([, state]) => state.status === "ready" && state.url === "stale",
      ),
    ).toBe(false);
  });

  it("isolates simultaneous runs even when they have the same image index", async () => {
    const slow = deferred<string>();
    const loader = setup((runId) =>
      runId === "run-a" ? slow.promise : Promise.resolve("image-b"),
    );
    loader.ensure("run-a", 0);
    loader.ensure("run-b", 0);
    await vi.advanceTimersByTimeAsync(0);
    slow.resolve("image-a");
    await vi.advanceTimersByTimeAsync(0);
    expect(loader.states["run-a:0"]).toEqual({
      status: "ready",
      url: "image-a",
    });
    expect(loader.states["run-b:0"]).toEqual({
      status: "ready",
      url: "image-b",
    });
  });

  it("cleans pending timers and ignores in-flight replies after unmount", async () => {
    const slow = deferred<string>();
    const fetch = vi.fn((runId: string) =>
      runId === "run-a" ? Promise.reject(pending) : slow.promise,
    );
    const loader = setup(fetch);
    loader.ensure("run-a", 0);
    loader.ensure("run-b", 0);
    await vi.advanceTimersByTimeAsync(0);
    loader.dispose();
    const count = loader.update.mock.calls.length;
    slow.resolve("stale");
    await vi.advanceTimersByTimeAsync(120_000);
    loader.ensure("run-c", 0);
    expect(vi.getTimerCount()).toBe(0);
    expect(loader.update).toHaveBeenCalledTimes(count);
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("treats an empty returned path as a read failure rather than loading forever", async () => {
    const loader = setup(async () => "");
    loader.ensure("run-a", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(loader.states["run-a:0"].status).toBe("error");
    expect(vi.getTimerCount()).toBe(0);
  });
});
