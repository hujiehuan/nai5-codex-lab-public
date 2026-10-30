// 虚拟网格的纯计算：卡片档位与尺寸、列数、可视行、焦点移动、单元定位。无 DOM 依赖，方便单测。

// ---- 卡片档位（v0.1.3）----

export type CardSize = "s" | "m" | "l" | "xl";
export const CARD_SIZES: readonly CardSize[] = ["s", "m", "l", "xl"];
export const CARD_SIZE_LABEL: Record<CardSize, string> = { s: "小", m: "中", l: "大", xl: "特大" };
export const DEFAULT_CARD_SIZE: CardSize = "m";
export const CARD_SIZE_KEY = "codexlab.cardSize";

/** 卡片边框 1px（include 态是 2px，EntryCard 里靠缩图片区 2px 补回，总高不变）。 */
export const CARD_BORDER = 1;
export const GRID_PADDING = 16;
export const OVERSCAN_ROWS = 2;

export interface CardMetrics {
  size: CardSize;
  /** 卡片外宽（含边框） */
  cardWidth: number;
  /** 图片区高度：正方形，= cardWidth */
  thumbHeight: number;
  /** 文字区高度（不含边框）：上下内边距 12 + 三行 lineHeight + 两个 2px 行距 */
  bodyHeight: number;
  /** 文字区每行行高（xl 字号略大，行高随之放大） */
  lineHeight: number;
  /** 卡片外高（含边框） = thumbHeight + bodyHeight + 2 × CARD_BORDER */
  cardHeight: number;
  /** 卡片间距 */
  gap: number;
  /** 容器内边距 */
  padding: number;
  /** 行距 = cardHeight + gap */
  rowHeight: number;
}

const SIZE_SPEC: Record<CardSize, { width: number; gap: number; lineHeight: number }> = {
  s: { width: 200, gap: 10, lineHeight: 18 },
  m: { width: 240, gap: 12, lineHeight: 18 },
  l: { width: 320, gap: 14, lineHeight: 18 },
  xl: { width: 400, gap: 16, lineHeight: 20 },
};

export function isCardSize(v: unknown): v is CardSize {
  return typeof v === "string" && (CARD_SIZES as readonly string[]).includes(v);
}

/**
 * 档位 → 尺寸。s 200 / m 240（原常量）/ l 320 / xl 400 宽；图片区正方形，文字区固定高度
 * （三行 × lineHeight + 内边距 12 + 行距 4），因此 m 档仍是 240 × 312、行距 324，与 v0.1.2 完全一致。
 */
export function cardMetrics(size: CardSize): CardMetrics {
  const spec = SIZE_SPEC[size];
  const bodyHeight = 12 + 3 * spec.lineHeight + 4;
  const cardHeight = spec.width + bodyHeight + 2 * CARD_BORDER;
  return {
    size,
    cardWidth: spec.width,
    thumbHeight: spec.width,
    bodyHeight,
    lineHeight: spec.lineHeight,
    cardHeight,
    gap: spec.gap,
    padding: GRID_PADDING,
    rowHeight: cardHeight + spec.gap,
  };
}

/** 放大 / 缩小一档，到头不动。 */
export function stepCardSize(size: CardSize, dir: 1 | -1): CardSize {
  const i = CARD_SIZES.indexOf(size);
  const next = Math.max(0, Math.min(CARD_SIZES.length - 1, (i < 0 ? CARD_SIZES.indexOf(DEFAULT_CARD_SIZE) : i) + dir));
  return CARD_SIZES[next];
}

/**
 * 键盘：⌘⇧= / ⌘⇧+ 放大一档，⌘⇧- / ⌘⇧_ 缩小一档（按物理键 code 判断，不受 ⇧ 改变 key 的影响）。
 * 特意带 ⇧，把不带 ⇧ 的 ⌘= / ⌘- 留给整页缩放。不是本快捷键返回 null。
 */
export function cardSizeKeyAction(
  e: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey?: boolean; key: string; code: string },
): "bigger" | "smaller" | null {
  if (!(e.metaKey || e.ctrlKey) || !e.shiftKey || e.altKey) return null;
  if (e.code === "Equal" || e.code === "NumpadAdd" || e.key === "+" || e.key === "=") return "bigger";
  if (e.code === "Minus" || e.code === "NumpadSubtract" || e.key === "_" || e.key === "-") return "smaller";
  return null;
}

export function loadCardSize(): CardSize {
  try {
    const raw = localStorage.getItem(CARD_SIZE_KEY);
    return isCardSize(raw) ? raw : DEFAULT_CARD_SIZE;
  } catch {
    return DEFAULT_CARD_SIZE;
  }
}

export function saveCardSize(size: CardSize): void {
  try {
    localStorage.setItem(CARD_SIZE_KEY, size);
  } catch {
    /* 存不了就算了 */
  }
}

// ---- 布局 ----

export interface RowRange { start: number; end: number }
export type MoveDir = "left" | "right" | "up" | "down";

/** 按容器宽度算列数：每列 card + gap，最少 1 列。 */
export function computeColumns(containerWidth: number, m: CardMetrics): number {
  const inner = containerWidth - m.padding * 2;
  if (!Number.isFinite(inner) || inner <= 0) return 1;
  return Math.max(1, Math.floor((inner + m.gap) / (m.cardWidth + m.gap)));
}

export function rowCount(itemCount: number, columns: number): number {
  if (itemCount <= 0 || columns <= 0) return 0;
  return Math.ceil(itemCount / columns);
}

/** 可视行区间（end 不含），上下各多 overscan 行。 */
export function visibleRowRange(
  scrollTop: number,
  viewportHeight: number,
  m: CardMetrics,
  totalRows: number,
  overscan = OVERSCAN_ROWS,
): RowRange {
  if (totalRows <= 0 || m.rowHeight <= 0) return { start: 0, end: 0 };
  const top = Math.max(0, scrollTop - m.padding);
  const start = Math.max(0, Math.floor(top / m.rowHeight) - overscan);
  const end = Math.min(totalRows, Math.ceil((top + Math.max(0, viewportHeight)) / m.rowHeight) + overscan);
  return { start, end: Math.max(start, end) };
}

/** 网格总高（spacer）：上下内边距 + 各行，最后一行后面没有间距。 */
export function gridHeight(totalRows: number, m: CardMetrics): number {
  if (totalRows <= 0) return 0;
  return m.padding * 2 + totalRows * m.rowHeight - m.gap;
}

export function cellPosition(index: number, columns: number, m: CardMetrics): { top: number; left: number } {
  const cols = Math.max(1, columns);
  const row = Math.floor(index / cols);
  const col = index % cols;
  return { top: m.padding + row * m.rowHeight, left: m.padding + col * (m.cardWidth + m.gap) };
}

export function keyToMove(key: string): MoveDir | null {
  switch (key) {
    case "ArrowLeft": case "h": case "H": return "left";
    case "ArrowRight": case "l": case "L": return "right";
    case "ArrowUp": case "k": case "K": return "up";
    case "ArrowDown": case "j": case "J": return "down";
    default: return null;
  }
}

/**
 * 焦点移动：左右按索引 ±1（到行尾自动换到下一行），上下按列数 ±columns，
 * 越界时钳在首尾。没有焦点时任意方向都落到第 0 项。
 */
export function moveFocus(index: number | null, dir: MoveDir, columns: number, total: number): number | null {
  if (total <= 0) return null;
  if (index === null || index < 0 || index >= total) return 0;
  const cols = Math.max(1, columns);
  let next = index;
  if (dir === "left") next = index - 1;
  else if (dir === "right") next = index + 1;
  else if (dir === "up") next = index - cols;
  else if (dir === "down") next = index + cols;
  if (dir === "up" && next < 0) return index;
  if (dir === "down" && next >= total) {
    // 最后一行不满时，允许落到最后一项
    return Math.floor(index / cols) === Math.floor((total - 1) / cols) ? index : total - 1;
  }
  return Math.max(0, Math.min(total - 1, next));
}

/** 连选范围（含两端），顺序从小到大。 */
export function rangeIndices(a: number, b: number): number[] {
  const lo = Math.min(a, b);
  const hi = Math.max(a, b);
  const out: number[] = [];
  for (let i = lo; i <= hi; i += 1) out.push(i);
  return out;
}

/**
 * 让某行完全可见需要的 scrollTop；已经可见返回 null。
 */
export function scrollTopForRow(row: number, scrollTop: number, viewportHeight: number, m: CardMetrics): number | null {
  const top = m.padding + row * m.rowHeight;
  const bottom = top + m.cardHeight;
  if (top < scrollTop) return Math.max(0, top - m.padding);
  if (bottom > scrollTop + viewportHeight) return Math.max(0, bottom - viewportHeight + m.padding);
  return null;
}

/** 旧档位下视口顶部那一行的首项索引（没焦点时换档位用它当锚点）。 */
export function firstVisibleIndex(scrollTop: number, columns: number, m: CardMetrics): number {
  if (m.rowHeight <= 0) return 0;
  const row = Math.floor(Math.max(0, scrollTop - m.padding) / m.rowHeight);
  return row * Math.max(1, columns);
}

/**
 * 换档位后的 scrollTop：锚点条目（焦点，或旧档位视口首项）的索引不变，但列数、行高都变了。
 * 先让它所在行保持在视口里原来的位置（相对视口顶部的偏移不变），再用 scrollTopForRow 兜底保证整行可见。
 */
export function scrollTopAfterResize(
  anchorIndex: number,
  from: { columns: number; metrics: CardMetrics; scrollTop: number },
  to: { columns: number; metrics: CardMetrics },
  viewportHeight: number,
): number {
  const idx = Math.max(0, anchorIndex);
  const oldRow = Math.floor(idx / Math.max(1, from.columns));
  const newRow = Math.floor(idx / Math.max(1, to.columns));
  const oldTop = from.metrics.padding + oldRow * from.metrics.rowHeight;
  const offset = oldTop - from.scrollTop;
  const newTop = to.metrics.padding + newRow * to.metrics.rowHeight;
  const base = Math.max(0, newTop - offset);
  return scrollTopForRow(newRow, base, viewportHeight, to.metrics) ?? base;
}
