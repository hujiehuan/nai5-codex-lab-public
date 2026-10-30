import { describe, expect, it } from "vitest";
import { createPeekTracker, isPeekEligible, movedBeyond, PEEK_HOLD_MS, PEEK_MOVE_PX } from "./peek";

describe("peek tracker", () => {
  it("短按：不足 250 ms 抬起 → click，不吞", () => {
    const p = createPeekTracker();
    expect(p.down(10, 10, 1000)).toBe("none");
    expect(p.phase).toBe("pending");
    expect(p.tick(1100)).toBe("none");
    expect(p.up(1120)).toBe("click");
    expect(p.phase).toBe("idle");
    expect(p.armed).toBe(false);
    expect(p.consumeClick()).toBe("click");
  });

  it("长按：tick 按满 250 ms → peek；随后 up → suppress-click，只吞一次", () => {
    const p = createPeekTracker();
    p.down(10, 10, 1000);
    expect(p.tick(1249)).toBe("none");
    expect(p.tick(1250)).toBe("peek");
    expect(p.phase).toBe("peeking");
    expect(p.tick(1300)).toBe("none"); // 不重复进入
    expect(p.up(1400)).toBe("suppress-click");
    expect(p.phase).toBe("idle");
    expect(p.armed).toBe(true);
    expect(p.consumeClick()).toBe("suppress-click");
    expect(p.consumeClick()).toBe("click"); // 一次性
    expect(p.armed).toBe(false);
  });

  it("peeking 期间移动不取消", () => {
    const p = createPeekTracker();
    p.down(0, 0, 0);
    expect(p.tick(250)).toBe("peek");
    expect(p.move(200, 300)).toBe("none");
    expect(p.phase).toBe("peeking");
    expect(p.up(600)).toBe("suppress-click");
  });

  it("pending 期间移动超过 6 px → 取消计时；之后 tick / up 都是 none，click 按原逻辑放行", () => {
    const p = createPeekTracker();
    p.down(0, 0, 0);
    expect(p.move(4, 4)).toBe("none"); // 5.66 px，还在阈值内
    expect(p.phase).toBe("pending");
    p.move(5, 4); // 6.4 px
    expect(p.phase).toBe("idle");
    expect(p.tick(300)).toBe("none");
    expect(p.up(320)).toBe("none");
    expect(p.armed).toBe(false);
    expect(p.consumeClick()).toBe("click");
  });

  it("修饰键 / 非主键不触发", () => {
    const cases = [{ shiftKey: true }, { metaKey: true }, { ctrlKey: true }, { altKey: true }, { button: 1 }, { button: 2 }];
    for (const b of cases) {
      const p = createPeekTracker();
      expect(isPeekEligible(b)).toBe(false);
      expect(p.down(0, 0, 0, b)).toBe("none");
      expect(p.phase).toBe("idle");
      expect(p.tick(1000)).toBe("none");
      expect(p.up(1000)).toBe("none");
      expect(p.consumeClick()).toBe("click");
    }
    expect(isPeekEligible({})).toBe(true);
    expect(isPeekEligible({ button: 0, shiftKey: false, metaKey: false, ctrlKey: false, altKey: false })).toBe(true);
  });

  it("Escape / 失焦取消 peek：按键还没松，随后的 up 与 click 仍要吞；下一次 down 清掉", () => {
    const p = createPeekTracker();
    p.down(0, 0, 0);
    expect(p.tick(250)).toBe("peek");
    expect(p.cancel()).toBe("suppress-click");
    expect(p.phase).toBe("idle");
    expect(p.up(500)).toBe("suppress-click");
    expect(p.consumeClick()).toBe("suppress-click");
    // Escape 后 click 迟迟没来（松在网格空白处），armed 留着；下一次 down 清掉，不会误吞下一次正常点击
    p.down(0, 0, 0);
    p.tick(250);
    p.cancel();
    expect(p.armed).toBe(true);
    p.down(50, 50, 1000);
    expect(p.armed).toBe(false);
    expect(p.up(1020)).toBe("click");
    // 没经过 down 的按下（卡片文字区 / 空白处）走 disarm
    p.down(0, 0, 0);
    p.tick(250);
    p.cancel();
    p.disarm();
    expect(p.armed).toBe(false);
    expect(p.consumeClick()).toBe("click");
  });

  it("pending 期间 cancel（pointercancel）→ none，不吞", () => {
    const p = createPeekTracker();
    p.down(0, 0, 0);
    expect(p.cancel()).toBe("none");
    expect(p.phase).toBe("idle");
    expect(p.armed).toBe(false);
    expect(p.up(100)).toBe("none");
  });

  it("计时器晚到：没 tick 但抬起时已按满 250 ms → suppress-click（长按不当点击）", () => {
    const p = createPeekTracker();
    p.down(0, 0, 0);
    expect(p.up(PEEK_HOLD_MS + 10)).toBe("suppress-click");
    expect(p.consumeClick()).toBe("suppress-click");
    const q = createPeekTracker();
    q.down(0, 0, 0);
    expect(q.up(PEEK_HOLD_MS - 1)).toBe("click");
  });

  it("自定义阈值", () => {
    const p = createPeekTracker({ holdMs: 100, movePx: 20 });
    p.down(0, 0, 0);
    p.move(10, 10);
    expect(p.phase).toBe("pending");
    expect(p.tick(99)).toBe("none");
    expect(p.tick(100)).toBe("peek");
  });

  it("movedBeyond 用直线距离，正好等于阈值不算", () => {
    expect(PEEK_HOLD_MS).toBe(250);
    expect(PEEK_MOVE_PX).toBe(6);
    expect(movedBeyond(0, 0, PEEK_MOVE_PX, 0)).toBe(false);
    expect(movedBeyond(0, 0, PEEK_MOVE_PX + 0.01, 0)).toBe(true);
    expect(movedBeyond(0, 0, 3, 3)).toBe(false);
    expect(movedBeyond(0, 0, 5, 5)).toBe(true);
    expect(movedBeyond(10, 10, 0, 0, 20)).toBe(false);
  });
});
