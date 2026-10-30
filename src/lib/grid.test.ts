import { describe, expect, it } from "vitest";
import {
  CARD_SIZES,
  DEFAULT_CARD_SIZE,
  GRID_PADDING,
  cardMetrics,
  cardSizeKeyAction,
  cellPosition,
  computeColumns,
  firstVisibleIndex,
  gridHeight,
  keyToMove,
  moveFocus,
  rangeIndices,
  rowCount,
  scrollTopAfterResize,
  scrollTopForRow,
  stepCardSize,
  visibleRowRange,
  type CardSize,
} from "./grid";

const M = cardMetrics("m");
const ROW_HEIGHT = M.rowHeight;
const CARD_HEIGHT = M.cardHeight;
const CARD_WIDTH = M.cardWidth;
const GRID_GAP = M.gap;

describe("card metrics", () => {
  it("derives width / square thumb / fixed body per size; m is the v0.1.2 layout", () => {
    expect(cardMetrics("s")).toMatchObject({ cardWidth: 200, thumbHeight: 200, bodyHeight: 70, cardHeight: 272, gap: 10, rowHeight: 282 });
    expect(cardMetrics("m")).toMatchObject({ cardWidth: 240, thumbHeight: 240, bodyHeight: 70, cardHeight: 312, gap: 12, rowHeight: 324 });
    expect(cardMetrics("l")).toMatchObject({ cardWidth: 320, thumbHeight: 320, bodyHeight: 70, cardHeight: 392, gap: 14, rowHeight: 406 });
    expect(cardMetrics("xl")).toMatchObject({ cardWidth: 400, thumbHeight: 400, bodyHeight: 76, cardHeight: 478, gap: 16, rowHeight: 494, lineHeight: 20 });
    for (const s of CARD_SIZES) {
      const m = cardMetrics(s);
      expect(m.size).toBe(s);
      expect(m.padding).toBe(GRID_PADDING);
      expect(m.cardHeight).toBe(m.thumbHeight + m.bodyHeight + 2);
      expect(m.rowHeight).toBe(m.cardHeight + m.gap);
    }
  });

  it("steps one size at a time and stops at the ends", () => {
    expect(stepCardSize("s", 1)).toBe("m");
    expect(stepCardSize("m", 1)).toBe("l");
    expect(stepCardSize("l", 1)).toBe("xl");
    expect(stepCardSize("xl", 1)).toBe("xl");
    expect(stepCardSize("xl", -1)).toBe("l");
    expect(stepCardSize("s", -1)).toBe("s");
    expect(DEFAULT_CARD_SIZE).toBe("m");
  });

  it("maps ⌘⇧= / ⌘⇧- (and numpad ± ) to bigger / smaller, leaves plain ⌘= / ⌘- alone", () => {
    const k = (o: Partial<Parameters<typeof cardSizeKeyAction>[0]>) =>
      cardSizeKeyAction({ metaKey: true, ctrlKey: false, shiftKey: true, key: "", code: "", ...o });
    expect(k({ key: "+", code: "Equal" })).toBe("bigger");
    expect(k({ key: "_", code: "Minus" })).toBe("smaller");
    expect(k({ key: "+", code: "NumpadAdd" })).toBe("bigger");
    expect(k({ key: "-", code: "NumpadSubtract" })).toBe("smaller");
    expect(k({ key: "=", code: "Equal", metaKey: false, ctrlKey: true })).toBe("bigger");
    // 不带 ⇧ 留给整页缩放
    expect(k({ key: "=", code: "Equal", shiftKey: false })).toBeNull();
    expect(k({ key: "-", code: "Minus", shiftKey: false })).toBeNull();
    // 没有 ⌘/Ctrl、或带 ⌥ 都不算
    expect(k({ key: "+", code: "Equal", metaKey: false })).toBeNull();
    expect(k({ key: "+", code: "Equal", altKey: true })).toBeNull();
    expect(k({ key: "A", code: "KeyA" })).toBeNull();
  });
});

describe("virtual grid columns", () => {
  it("fits as many 240px cards as the width allows, minimum one", () => {
    expect(computeColumns(0, M)).toBe(1);
    expect(computeColumns(100, M)).toBe(1);
    // 16 padding each side + 240 = 272 → exactly one column
    expect(computeColumns(272, M)).toBe(1);
    // 2 cards need 240*2 + 12 + 32 = 524
    expect(computeColumns(523, M)).toBe(1);
    expect(computeColumns(524, M)).toBe(2);
    expect(computeColumns(1185, M)).toBe(4);
    expect(computeColumns(1300, M)).toBe(5);
    expect(computeColumns(NaN, M)).toBe(1);
  });

  it("honours custom card width, gap and padding", () => {
    expect(computeColumns(1000, { ...M, cardWidth: 100, gap: 0, padding: 0 })).toBe(10);
    expect(computeColumns(1000, { ...M, cardWidth: 100, gap: 10, padding: 0 })).toBe(9);
  });

  it("gives 6 / 5 / 4 / 3 columns at 1400px for s / m / l / xl", () => {
    const cols = Object.fromEntries(CARD_SIZES.map((s) => [s, computeColumns(1400, cardMetrics(s))]));
    expect(cols).toEqual({ s: 6, m: 5, l: 4, xl: 3 });
    // 每档都要放得下：n 列 + (n-1) 间距 + 两侧内边距 ≤ 1400，再加一列就放不下
    for (const s of CARD_SIZES) {
      const m = cardMetrics(s);
      const n = cols[s];
      expect(n * m.cardWidth + (n - 1) * m.gap + 2 * m.padding).toBeLessThanOrEqual(1400);
      expect((n + 1) * m.cardWidth + n * m.gap + 2 * m.padding).toBeGreaterThan(1400);
    }
  });

  it("computes row count", () => {
    expect(rowCount(0, 4)).toBe(0);
    expect(rowCount(1, 4)).toBe(1);
    expect(rowCount(4, 4)).toBe(1);
    expect(rowCount(5, 4)).toBe(2);
    expect(rowCount(10, 0)).toBe(0);
  });

  it("computes total grid height without a trailing gap", () => {
    expect(gridHeight(0, M)).toBe(0);
    expect(gridHeight(3, M)).toBe(GRID_PADDING * 2 + 3 * ROW_HEIGHT - GRID_GAP);
    expect(gridHeight(1, cardMetrics("xl"))).toBe(GRID_PADDING * 2 + 478);
  });
});

describe("visible rows", () => {
  const rh = ROW_HEIGHT;
  it("renders visible rows plus two overscan rows on each side", () => {
    expect(visibleRowRange(0, rh * 3, M, 100)).toEqual({ start: 0, end: 5 });
    // scrolled to row 10 exactly
    expect(visibleRowRange(GRID_PADDING + rh * 10, rh * 3, M, 100)).toEqual({ start: 8, end: 15 });
  });

  it("clamps at the end and handles empty grids", () => {
    expect(visibleRowRange(GRID_PADDING + rh * 98, rh * 3, M, 100)).toEqual({ start: 96, end: 100 });
    expect(visibleRowRange(0, 500, M, 0)).toEqual({ start: 0, end: 0 });
    expect(visibleRowRange(0, 500, { ...M, rowHeight: 0 }, 10)).toEqual({ start: 0, end: 0 });
  });

  it("rounds partially visible rows in", () => {
    expect(visibleRowRange(GRID_PADDING + rh * 2 + 1, rh + 1, M, 100, 0)).toEqual({ start: 2, end: 4 });
  });

  it("shows fewer rows in a 900px viewport as cards grow: s 6 / m 5 / l 5 / xl 4 (incl. overscan)", () => {
    const ends = Object.fromEntries(CARD_SIZES.map((s) => [s, visibleRowRange(0, 900, cardMetrics(s), 100).end]));
    expect(ends).toEqual({ s: 6, m: 5, l: 5, xl: 4 });
    for (const s of CARD_SIZES) {
      const m = cardMetrics(s);
      expect(visibleRowRange(0, 900, m, 100)).toEqual({ start: 0, end: Math.ceil(900 / m.rowHeight) + 2 });
    }
  });
});

describe("cell position", () => {
  it("places cards by row and column", () => {
    expect(cellPosition(0, 4, M)).toEqual({ top: GRID_PADDING, left: GRID_PADDING });
    expect(cellPosition(3, 4, M)).toEqual({ top: GRID_PADDING, left: GRID_PADDING + 3 * (CARD_WIDTH + GRID_GAP) });
    expect(cellPosition(4, 4, M)).toEqual({ top: GRID_PADDING + CARD_HEIGHT + GRID_GAP, left: GRID_PADDING });
  });

  it("uses each size's own width, gap and row height", () => {
    const xl = cardMetrics("xl");
    expect(cellPosition(4, 3, xl)).toEqual({ top: GRID_PADDING + xl.rowHeight, left: GRID_PADDING + 416 });
    const s = cardMetrics("s");
    expect(cellPosition(7, 6, s)).toEqual({ top: GRID_PADDING + s.rowHeight, left: GRID_PADDING + 210 });
  });
});

describe("keyboard focus", () => {
  it("maps arrows and HJKL", () => {
    expect(keyToMove("ArrowLeft")).toBe("left");
    expect(keyToMove("h")).toBe("left");
    expect(keyToMove("l")).toBe("right");
    expect(keyToMove("k")).toBe("up");
    expect(keyToMove("J")).toBe("down");
    expect(keyToMove("a")).toBeNull();
  });

  it("moves with wrap at row ends and clamps at bounds", () => {
    // 10 items, 4 columns: rows [0..3][4..7][8..9]
    expect(moveFocus(null, "down", 4, 10)).toBe(0);
    expect(moveFocus(3, "right", 4, 10)).toBe(4);
    expect(moveFocus(4, "left", 4, 10)).toBe(3);
    expect(moveFocus(0, "left", 4, 10)).toBe(0);
    expect(moveFocus(9, "right", 4, 10)).toBe(9);
    expect(moveFocus(1, "down", 4, 10)).toBe(5);
    expect(moveFocus(5, "up", 4, 10)).toBe(1);
    expect(moveFocus(1, "up", 4, 10)).toBe(1);
    // 6 down → would be 10 (out of range) but row below exists → last item
    expect(moveFocus(6, "down", 4, 10)).toBe(9);
    // already on last row
    expect(moveFocus(8, "down", 4, 10)).toBe(8);
    expect(moveFocus(0, "down", 4, 0)).toBeNull();
  });

  it("builds inclusive ranges in either direction", () => {
    expect(rangeIndices(2, 5)).toEqual([2, 3, 4, 5]);
    expect(rangeIndices(5, 2)).toEqual([2, 3, 4, 5]);
    expect(rangeIndices(3, 3)).toEqual([3]);
  });

  it("computes the scrollTop needed to reveal a row", () => {
    expect(scrollTopForRow(0, 0, 1000, M)).toBeNull();
    expect(scrollTopForRow(5, 0, 600, M)).toBe(GRID_PADDING + 5 * ROW_HEIGHT + CARD_HEIGHT - 600 + GRID_PADDING);
    expect(scrollTopForRow(2, GRID_PADDING + 5 * ROW_HEIGHT, 600, M)).toBe(2 * ROW_HEIGHT);
  });
});

describe("card size switch keeps the focused entry", () => {
  const WIDTH = 1400;
  const VIEWPORT = 900;
  const cols = (s: CardSize) => computeColumns(WIDTH, cardMetrics(s));
  const from = (s: CardSize, scrollTop: number) => ({ columns: cols(s), metrics: cardMetrics(s), scrollTop });
  const to = (s: CardSize) => ({ columns: cols(s), metrics: cardMetrics(s) });
  /** 焦点所在行在新 scrollTop 下是否整行可见 */
  const fullyVisible = (index: number, s: CardSize, scrollTop: number) => {
    const pos = cellPosition(index, cols(s), cardMetrics(s));
    return pos.top >= scrollTop && pos.top + cardMetrics(s).cardHeight <= scrollTop + VIEWPORT;
  };

  it("first visible index follows the top row of the old layout", () => {
    expect(firstVisibleIndex(0, 5, M)).toBe(0);
    expect(firstVisibleIndex(GRID_PADDING + 3 * ROW_HEIGHT + 5, 5, M)).toBe(15);
    expect(firstVisibleIndex(GRID_PADDING + 3 * ROW_HEIGHT - 1, 5, M)).toBe(10);
  });

  it("m → xl: focus 47 stays index 47, moves from row 9 to row 15 and stays where it was on screen", () => {
    const focus = 47;
    const rowTopM = GRID_PADDING + Math.floor(focus / cols("m")) * ROW_HEIGHT; // row 9
    // 焦点行原本在视口顶部往下 300px
    const next = scrollTopAfterResize(focus, from("m", rowTopM - 300), to("xl"), VIEWPORT);
    const xl = cardMetrics("xl");
    expect(Math.floor(focus / cols("xl"))).toBe(15);
    expect(next).toBe(GRID_PADDING + 15 * xl.rowHeight - 300);
    expect(fullyVisible(focus, "xl", next)).toBe(true);
    // 渲染区间也包含该行
    const r = visibleRowRange(next, VIEWPORT, xl, rowCount(200, cols("xl")));
    expect(r.start).toBeLessThanOrEqual(15);
    expect(r.end).toBeGreaterThan(15);
  });

  it("xl → s: same index lands on row 7 aligned exactly as before", () => {
    const focus = 47;
    const xl = cardMetrics("xl");
    const rowTopXl = GRID_PADDING + Math.floor(focus / cols("xl")) * xl.rowHeight;
    const next = scrollTopAfterResize(focus, from("xl", rowTopXl), to("s"), VIEWPORT);
    expect(next).toBe(GRID_PADDING + 7 * cardMetrics("s").rowHeight);
    expect(fullyVisible(focus, "s", next)).toBe(true);
  });

  it("pulls the row back in when keeping the offset would push it off the bottom or top", () => {
    const focus = 47;
    const rowTopM = GRID_PADDING + 9 * ROW_HEIGHT;
    // 焦点行原本贴在视口底部（偏移 800）：xl 卡更高，保持偏移会溢出 → 对齐到底边
    const low = scrollTopAfterResize(focus, from("m", rowTopM - 800), to("xl"), VIEWPORT);
    const xl = cardMetrics("xl");
    expect(low).toBe(GRID_PADDING + 15 * xl.rowHeight + xl.cardHeight - VIEWPORT + GRID_PADDING);
    expect(fullyVisible(focus, "xl", low)).toBe(true);
    // 焦点行原本在视口上方（已滚过去）：对齐到顶边
    const high = scrollTopAfterResize(focus, from("m", rowTopM + 2000), to("xl"), VIEWPORT);
    expect(high).toBe(15 * xl.rowHeight);
    expect(fullyVisible(focus, "xl", high)).toBe(true);
  });

  it("keeps the focused entry fully visible for every size pair and scroll offset", () => {
    for (const a of CARD_SIZES) {
      for (const b of CARD_SIZES) {
        for (const focus of [0, 13, 47, 199]) {
          for (const offset of [-3000, -200, 0, 300, 850, 4000]) {
            const rowTop = GRID_PADDING + Math.floor(focus / cols(a)) * cardMetrics(a).rowHeight;
            const next = scrollTopAfterResize(focus, from(a, Math.max(0, rowTop - offset)), to(b), VIEWPORT);
            expect(next).toBeGreaterThanOrEqual(0);
            expect(fullyVisible(focus, b, next)).toBe(true);
          }
        }
      }
    }
  });

  it("returns 0 when the anchor is the first item near the top", () => {
    expect(scrollTopAfterResize(0, from("m", 0), to("xl"), VIEWPORT)).toBe(0);
    expect(scrollTopAfterResize(0, from("xl", 0), to("s"), VIEWPORT)).toBe(0);
  });
});
