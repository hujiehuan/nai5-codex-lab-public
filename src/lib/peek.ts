// 按住看大图（v0.1.3 peek）：在卡片图片上按住主键不动 250 ms → 全窗口覆盖层显示大图，松开即回到原界面。
// 这里只有状态机（纯函数，可单测）；计时器、window 监听与覆盖层在 CodexView / PeekOverlay。
//
//   idle ──down(主键、无修饰键)──▶ pending ──tick(t ≥ t0 + holdMs)──▶ peeking ──up / cancel──▶ idle（armed）
//                                    │ move 超过 movePx / cancel ──▶ idle（click 按原逻辑放行）
//                                    └──up(不足 holdMs)──▶ idle，返回 "click"
//
// armed：peek 结束后紧随而来的那次 click 要吞掉（不切换三态）。click 不一定落在同一张卡上（甚至不落在任何卡上），
// 所以不能用"吞掉下一次 click"的一次性开关；armed 一直留到下一次 pointerdown（down / disarm）才清。

export const PEEK_HOLD_MS = 250;
export const PEEK_MOVE_PX = 6;

export type PeekAction = "none" | "peek" | "click" | "suppress-click";
export type PeekPhase = "idle" | "pending" | "peeking";

/** pointerdown 的按键与修饰键；缺省视为主键、无修饰 */
export interface PeekButton {
  button?: number;
  shiftKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
}

/** 只有主键且无 ⇧ / ⌘ / Ctrl / ⌥ 才可能进入 peek；右键、修饰键点击保持原有行为 */
export function isPeekEligible(b: PeekButton): boolean {
  return (b.button ?? 0) === 0 && !b.shiftKey && !b.metaKey && !b.ctrlKey && !b.altKey;
}

/** 指针离按下点的直线距离是否超过阈值（正好等于不算） */
export function movedBeyond(x0: number, y0: number, x1: number, y1: number, threshold = PEEK_MOVE_PX): boolean {
  const dx = x1 - x0;
  const dy = y1 - y0;
  return dx * dx + dy * dy > threshold * threshold;
}

export interface PeekTracker {
  readonly phase: PeekPhase;
  /** peek 结束后是否还欠着一次要吞掉的 click */
  readonly armed: boolean;
  /** pointerdown：清掉上一轮的 armed；符合条件则进入 pending。总是返回 "none"。 */
  down(x: number, y: number, t: number, button?: PeekButton): PeekAction;
  /** pointermove：pending 期间移出阈值就取消（回 idle，之后的 click 按原逻辑）；peeking 期间随便动。总是返回 "none"。 */
  move(x: number, y: number): PeekAction;
  /** 计时器到点：pending 且已按满 holdMs → "peek"（只返回一次）。 */
  tick(t: number): PeekAction;
  /**
   * pointerup：pending 且不足 holdMs → "click"（短按）；peeking → "suppress-click"（并 armed）。
   * pending 但已按满 holdMs（计时器没来得及 tick）同样 suppress：按住这么久不该当成点击去改决策。
   */
  up(t: number): PeekAction;
  /** pointercancel / Escape / 窗口失焦：退出 peek。peeking 中被取消同样 armed——按键可能还没松，click 随后会来。 */
  cancel(): PeekAction;
  /** 任何位置的新一次 pointerdown：不再欠 click（没经过 down 的按下也要清，否则会误吞下一次正常点击）。 */
  disarm(): void;
  /** click 事件到了：armed 则吞掉（一次性），否则放行。 */
  consumeClick(): "click" | "suppress-click";
}

export function createPeekTracker(opts: { holdMs?: number; movePx?: number } = {}): PeekTracker {
  const holdMs = opts.holdMs ?? PEEK_HOLD_MS;
  const movePx = opts.movePx ?? PEEK_MOVE_PX;
  let phase: PeekPhase = "idle";
  let armed = false;
  let x0 = 0;
  let y0 = 0;
  let t0 = 0;

  return {
    get phase() {
      return phase;
    },
    get armed() {
      return armed;
    },
    down(x, y, t, button = {}) {
      armed = false;
      if (!isPeekEligible(button)) {
        phase = "idle";
        return "none";
      }
      phase = "pending";
      x0 = x;
      y0 = y;
      t0 = t;
      return "none";
    },
    move(x, y) {
      if (phase === "pending" && movedBeyond(x0, y0, x, y, movePx)) phase = "idle";
      return "none";
    },
    tick(t) {
      if (phase !== "pending" || t - t0 < holdMs) return "none";
      phase = "peeking";
      return "peek";
    },
    up(t) {
      const was = phase;
      phase = "idle";
      if (was === "peeking" || (was === "pending" && t - t0 >= holdMs)) {
        armed = true;
        return "suppress-click";
      }
      if (was === "pending") return "click";
      return armed ? "suppress-click" : "none";
    },
    cancel() {
      const was = phase;
      phase = "idle";
      if (was === "peeking") {
        armed = true;
        return "suppress-click";
      }
      return "none";
    },
    disarm() {
      armed = false;
    },
    consumeClick() {
      const a = armed;
      armed = false;
      return a ? "suppress-click" : "click";
    },
  };
}
