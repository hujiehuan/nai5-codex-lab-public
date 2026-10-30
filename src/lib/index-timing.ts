/** 本轮索引计时。调用方传入 performance.now()；不使用全局时钟或定时器。 */
interface ProgressPoint {
  readonly processed: number;
  readonly at: number;
}

interface BatchSample {
  readonly count: number;
  readonly durationMs: number;
}

export interface IndexTiming {
  readonly startedAt: number;
  readonly finishedAt: number | null;
  readonly total: number | null;
  readonly processed: number;
  readonly lastProgress: ProgressPoint | null;
  readonly samples: readonly BatchSample[];
}

export interface IndexTimingView {
  readonly phase: "estimating" | "running" | "stalled" | "finishing" | "finished";
  readonly elapsedSeconds: number;
  readonly remainingSeconds: number | null;
  readonly itemsPerMinute: number | null;
}

const MAX_SAMPLES = 6;
const MIN_STALL_MS = 60_000;

function clockTime(now: number, floor: number): number {
  return Number.isFinite(now) ? Math.max(floor, now) : floor;
}

export function createTiming(now: number): IndexTiming {
  return {
    startedAt: clockTime(now, 0), finishedAt: null, total: null, processed: 0,
    lastProgress: null, samples: [],
  };
}

/** processed = 本轮 done + failed；total 不包含此前已经存在的索引。 */
export function recordProgress(
  state: IndexTiming, total: number, processed: number, now: number,
): IndexTiming {
  if (state.finishedAt !== null) return state;
  if (!Number.isSafeInteger(total) || !Number.isSafeInteger(processed)
      || total < 0 || processed < state.processed) return state;
  const next = { ...state, total: Math.max(total, processed), processed };
  // 重复、无进展的事件不能重置计时基准或推迟无进展提示。
  if (processed === state.processed) return next;
  const at = clockTime(now, state.lastProgress?.at ?? state.startedAt);
  const point = { processed, at };
  // 第一批仅作为基准，排除启动与模型加载造成的耗时。
  if (!state.lastProgress) return { ...next, lastProgress: point };
  const durationMs = at - state.lastProgress.at;
  // 同时刻进度没有可测时长；留给下一次有时长的采样一起统计。
  if (durationMs <= 0) return next;
  const sample = { count: processed - state.lastProgress.processed, durationMs };
  return {
    ...next,
    lastProgress: point,
    samples: [...state.samples, sample].slice(-MAX_SAMPLES),
  };
}

/** 完成、取消和出错共用；此后事件不再更新该轮的计时。 */
export function finishTiming(state: IndexTiming, now: number): IndexTiming {
  if (state.finishedAt !== null) return state;
  return {
    ...state,
    finishedAt: clockTime(now, state.lastProgress?.at ?? state.startedAt),
  };
}

export function timingView(state: IndexTiming, now: number): IndexTimingView {
  const at = state.finishedAt ?? clockTime(now, state.lastProgress?.at ?? state.startedAt);
  const base = {
    elapsedSeconds: Math.max(0, (at - state.startedAt) / 1000),
    remainingSeconds: null,
    itemsPerMinute: null,
  };
  if (state.finishedAt !== null) return { ...base, phase: "finished" };
  // 等后端写完均值、发出终态；不要让 0 秒暗示任务已经完成。
  if (state.total !== null && state.total > 0 && state.processed >= state.total) {
    return { ...base, phase: "finishing" };
  }
  const durationMs = state.samples.reduce((sum, sample) => sum + sample.durationMs, 0);
  const count = state.samples.reduce((sum, sample) => sum + sample.count, 0);
  const averageBatchMs = state.samples.length ? durationMs / state.samples.length : 0;
  if (at - (state.lastProgress?.at ?? state.startedAt)
      >= Math.max(MIN_STALL_MS, 3 * averageBatchMs)) {
    return { ...base, phase: "stalled" };
  }
  if (durationMs <= 0 || count <= 0 || state.total === null || state.total === 0) {
    return { ...base, phase: "estimating" };
  }
  const itemsPerMinute = count / durationMs * 60_000;
  // 只根据最近完成的批次更新，当前批次仍在跑时不做虚假的逐秒倒计时。
  const remainingSeconds = (state.total - state.processed) / count * durationMs / 1000;
  if (!Number.isFinite(itemsPerMinute) || !Number.isFinite(remainingSeconds)) {
    return { ...base, phase: "estimating" };
  }
  return { ...base, phase: "running", remainingSeconds, itemsPerMinute };
}

function elapsedDuration(seconds: number): string {
  const value = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor(value % 3600 / 60);
  const rest = value % 60;
  const parts: string[] = [];
  if (hours) parts.push(`${hours} 小时`);
  if (minutes) parts.push(`${minutes} 分钟`);
  if (rest || parts.length === 0) parts.push(`${rest} 秒`);
  return parts.join(" ");
}

export function formatElapsed(seconds: number): string {
  return `已用 ${Number.isFinite(seconds) ? elapsedDuration(seconds) : "—"}`;
}

export function formatRemaining(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "预计剩余约 —";
  // 较长的 ETA 只显示到分钟，避免显得比采样精度更准确。
  const rounded = seconds < 60
    ? Math.max(1, Math.round(seconds))
    : Math.round(seconds / 60) * 60;
  return `预计剩余约 ${elapsedDuration(rounded)}`;
}

export function formatRate(itemsPerMinute: number): string {
  if (!Number.isFinite(itemsPerMinute) || itemsPerMinute < 0) return "— 张/分钟";
  if (itemsPerMinute > 0 && itemsPerMinute < 0.1) return "不足 0.1 张/分钟";
  return `${Number(itemsPerMinute.toFixed(1))} 张/分钟`;
}
