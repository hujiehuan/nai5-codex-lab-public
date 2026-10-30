import { describe, expect, it } from "vitest";
import { clampZoom, stepZoom, zoomActionForKey, ZOOM_MAX, ZOOM_MIN } from "./zoom";

describe("zoom", () => {
  it("clamps and rounds to one decimal", () => {
    expect(clampZoom(1.04)).toBe(1);
    expect(clampZoom(0.2)).toBe(ZOOM_MIN);
    expect(clampZoom(9)).toBe(ZOOM_MAX);
    expect(clampZoom(Number.NaN)).toBe(1);
  });
  it("steps by 0.1 within bounds", () => {
    expect(stepZoom(1, 1)).toBe(1.1);
    expect(stepZoom(1, -1)).toBe(0.9);
    expect(stepZoom(ZOOM_MAX, 1)).toBe(ZOOM_MAX);
    expect(stepZoom(ZOOM_MIN, -1)).toBe(ZOOM_MIN);
  });
  it("maps ⌘= ⌘+ ⌘- ⌘0 and ignores others", () => {
    const k = (key: string, code = "", metaKey = true) => zoomActionForKey({ metaKey, ctrlKey: false, key, code });
    expect(k("=", "Equal")).toBe("in");
    expect(k("+", "Equal")).toBe("in");
    expect(k("-", "Minus")).toBe("out");
    expect(k("0", "Digit0")).toBe("reset");
    expect(k("a")).toBeNull();
    expect(k("=", "Equal", false)).toBeNull();
    // ⌘⇧= / ⌘⇧- 是卡片档位的快捷键，不算缩放
    expect(zoomActionForKey({ metaKey: true, ctrlKey: false, shiftKey: true, key: "+", code: "Equal" })).toBeNull();
    expect(zoomActionForKey({ metaKey: true, ctrlKey: false, shiftKey: true, key: "_", code: "Minus" })).toBeNull();
  });
});
