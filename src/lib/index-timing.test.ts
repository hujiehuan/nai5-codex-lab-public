import { describe, expect, it } from "vitest";
import {
  createTiming, finishTiming, formatElapsed, formatRate, formatRemaining,
  recordProgress, timingView,
} from "./index-timing";

describe("index timing", () => {
  it("waits for two advancing batches and excludes startup from the speed", () => {
    let state = createTiming(0);
    state = recordProgress(state, 100, 0, 20_000);
    expect(timingView(state, 20_000)).toMatchObject({
      phase: "estimating", remainingSeconds: null, elapsedSeconds: 20,
    });
    state = recordProgress(state, 100, 10, 40_000);
    expect(timingView(state, 40_000).remainingSeconds).toBeNull();
    state = recordProgress(state, 100, 20, 50_000);
    expect(timingView(state, 50_000)).toEqual({
      phase: "running", remainingSeconds: 80, itemsPerMinute: 60, elapsedSeconds: 50,
    });
    expect(timingView(state, 55_000)).toMatchObject({ remainingSeconds: 80, elapsedSeconds: 55 });
  });

  it("uses weighted throughput when the batch sizes differ", () => {
    let state = recordProgress(createTiming(0), 100, 10, 10_000);
    state = recordProgress(state, 100, 30, 20_000);
    state = recordProgress(state, 100, 35, 30_000);
    expect(timingView(state, 30_000)).toMatchObject({
      itemsPerMinute: 75, remainingSeconds: 52,
    });
  });

  it("adapts to recent speed after old slow batches leave the six-sample window", () => {
    let state = recordProgress(createTiming(0), 200, 10, 10_000);
    state = recordProgress(state, 200, 20, 40_000);
    for (let i = 1; i <= 6; i++) {
      state = recordProgress(state, 200, 20 + i * 10, 40_000 + i * 1000);
    }
    expect(timingView(state, 46_000)).toMatchObject({ itemsPerMinute: 600, remainingSeconds: 12 });
  });

  it("counts failures as processed work and uses only this run's remaining pool", () => {
    // 之前完成的 112 张不在本轮 total 中。
    let state = recordProgress(createTiming(0), 3096, 14 + 2, 10_000);
    state = recordProgress(state, 3096, 28 + 4, 26_000);
    expect(timingView(state, 26_000)).toMatchObject({ itemsPerMinute: 60, remainingSeconds: 3064 });
  });

  it("does not let duplicate or older progress corrupt the measured speed", () => {
    let state = recordProgress(createTiming(0), 100, 10, 10_000);
    state = recordProgress(state, 100, 10, 14_000);
    state = recordProgress(state, 100, 0, 15_000);
    state = recordProgress(state, 100, 20, 20_000);
    expect(timingView(state, 20_000)).toMatchObject({ itemsPerMinute: 60, remainingSeconds: 80 });
    const duplicate = recordProgress(state, 100, 20, 79_000);
    expect(timingView(duplicate, 80_000)).toMatchObject({ phase: "stalled", remainingSeconds: null });
  });

  it("hides outdated ETA after a stall and resumes when another batch arrives", () => {
    let state = recordProgress(createTiming(0), 100, 10, 10_000);
    state = recordProgress(state, 100, 20, 20_000);
    expect(timingView(state, 79_999).phase).toBe("running");
    expect(timingView(state, 80_000)).toMatchObject({
      phase: "stalled", remainingSeconds: null, itemsPerMinute: null,
    });
    state = recordProgress(state, 100, 30, 85_000);
    expect(timingView(state, 85_000)).toMatchObject({ phase: "running", remainingSeconds: 262.5 });
  });

  it("allows three normal batch durations for a slow machine before calling it stalled", () => {
    let state = recordProgress(createTiming(0), 100, 10, 10_000);
    state = recordProgress(state, 100, 20, 50_000);
    expect(timingView(state, 169_999).phase).toBe("running");
    expect(timingView(state, 170_000).phase).toBe("stalled");
    expect(timingView(createTiming(0), 60_000).phase).toBe("stalled");
  });

  it("shows finishing without a zero-second ETA until the terminal event arrives", () => {
    let state = recordProgress(createTiming(0), 20, 10, 10_000);
    state = recordProgress(state, 20, 20, 20_000);
    expect(timingView(state, 20_000)).toMatchObject({ phase: "finishing", remainingSeconds: null });
    state = finishTiming(state, 25_000);
    expect(timingView(state, 90_000)).toMatchObject({
      phase: "finished", remainingSeconds: null, elapsedSeconds: 25,
    });
    expect(recordProgress(state, 30, 30, 30_000)).toBe(state);
  });

  it("freezes cancellation timing and starts resumed work with fresh samples", () => {
    let state = recordProgress(createTiming(0), 100, 10, 10_000);
    state = recordProgress(state, 100, 20, 20_000);
    state = finishTiming(state, 25_000);
    expect(timingView(state, 90_000)).toMatchObject({
      phase: "finished", remainingSeconds: null, elapsedSeconds: 25,
    });
    const resumed = recordProgress(createTiming(100_000), 80, 10, 110_000);
    expect(timingView(resumed, 110_000)).toMatchObject({
      phase: "estimating", remainingSeconds: null, elapsedSeconds: 10,
    });
  });

  it("handles empty runs, zero-duration samples and malformed inputs without invalid numbers", () => {
    expect(timingView(recordProgress(createTiming(0), 0, 0, 0), 0)).toMatchObject({
      phase: "estimating", remainingSeconds: null,
    });
    let state = recordProgress(createTiming(0), 100, 10, 0);
    state = recordProgress(state, 100, 20, 0);
    expect(timingView(state, 0)).toMatchObject({ phase: "estimating", itemsPerMinute: null });
    state = recordProgress(state, 100, 30, 10_000);
    expect(timingView(state, 10_000)).toMatchObject({ itemsPerMinute: 120, remainingSeconds: 35 });
    expect(recordProgress(state, Number.NaN, 40, 20_000)).toBe(state);
    expect(recordProgress(state, 100, Number.POSITIVE_INFINITY, 20_000)).toBe(state);
    expect(timingView(createTiming(Number.NaN), Number.NaN).elapsedSeconds).toBe(0);
    expect(timingView(state, -1).elapsedSeconds).toBe(10);
  });

  it("preserves earlier state objects so Vue can replace its ref safely", () => {
    const original = createTiming(0);
    const first = recordProgress(original, 100, 10, 10_000);
    recordProgress(first, 100, 20, 20_000);
    expect(original.processed).toBe(0);
    expect(first.processed).toBe(10);
    expect(first.samples).toEqual([]);
  });
});

describe("index timing labels", () => {
  it("formats elapsed hours and days without wrapping at an hour or a day", () => {
    expect(formatElapsed(0)).toBe("已用 0 秒");
    expect(formatElapsed(62.8)).toBe("已用 1 分钟 2 秒");
    expect(formatElapsed(3661)).toBe("已用 1 小时 1 分钟 1 秒");
    expect(formatElapsed(90_000)).toBe("已用 25 小时");
  });
  it("keeps remaining time approximate and preserves sub-minute resolution", () => {
    expect(formatRemaining(12.4)).toBe("预计剩余约 12 秒");
    expect(formatRemaining(0.1)).toBe("预计剩余约 1 秒");
    expect(formatRemaining(125)).toBe("预计剩余约 2 分钟");
    expect(formatRemaining(3725)).toBe("预计剩余约 1 小时 2 分钟");
    expect(formatRate(32.84)).toBe("32.8 张/分钟");
    expect(formatRate(60)).toBe("60 张/分钟");
    expect(formatRate(0.001)).toBe("不足 0.1 张/分钟");
  });
  it("never renders NaN or Infinity in a label", () => {
    expect(formatElapsed(Number.NaN)).toBe("已用 —");
    expect(formatRemaining(Number.POSITIVE_INFINITY)).toBe("预计剩余约 —");
    expect(formatRate(Number.NaN)).toBe("— 张/分钟");
  });
});
