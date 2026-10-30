// 页面整体缩放（v0.1.3）：⌘= / ⌘+ 放大，⌘- 缩小，⌘0 复位。
// 用 CSS `zoom` 作用在 <html> 上：WebKit 会把布局与 ResizeObserver 报告的尺寸一起按比例缩放，
// 所以虚拟网格的列数计算无需改动。值持久化在 localStorage `codexlab.zoom`。

export const ZOOM_MIN = 0.7;
export const ZOOM_MAX = 2.0;
export const ZOOM_STEP = 0.1;
export const ZOOM_KEY = "codexlab.zoom";

export function clampZoom(z: number): number {
  if (!Number.isFinite(z)) return 1;
  const r = Math.round(z * 10) / 10;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, r));
}

export function stepZoom(current: number, direction: 1 | -1): number {
  return clampZoom(current + direction * ZOOM_STEP);
}

/**
 * 把键盘事件翻译成缩放动作；不是缩放快捷键返回 null。
 * 带 ⇧ 的组合（⌘⇧= / ⌘⇧-）留给词典页的卡片档位（CodexView，v0.1.3），这里不接。
 */
export function zoomActionForKey(e: { metaKey: boolean; ctrlKey: boolean; shiftKey?: boolean; key: string; code: string }): "in" | "out" | "reset" | null {
  if (!(e.metaKey || e.ctrlKey) || e.shiftKey) return null;
  if (e.key === "=" || e.key === "+" || e.code === "Equal" || e.code === "NumpadAdd") return "in";
  if (e.key === "-" || e.key === "_" || e.code === "Minus" || e.code === "NumpadSubtract") return "out";
  if (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0") return "reset";
  return null;
}

export function loadZoom(): number {
  try {
    const raw = localStorage.getItem(ZOOM_KEY);
    return raw ? clampZoom(Number(raw)) : 1;
  } catch {
    return 1;
  }
}

export function saveZoom(z: number): void {
  try {
    localStorage.setItem(ZOOM_KEY, String(z));
  } catch {
    /* 无存储环境时忽略 */
  }
}

export function applyZoom(z: number): void {
  if (typeof document === "undefined") return;
  // TS 的 CSSStyleDeclaration 类型里没有 zoom，WebKit 支持该属性。
  (document.documentElement.style as unknown as Record<string, string>).zoom = String(z);
}
