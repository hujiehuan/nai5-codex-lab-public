import { describe, expect, it } from "vitest";
import { fmtBytes, fmtDelta, fmtMs, fmtPct, lastSegment, padOrder } from "./format";

describe("format helpers", () => {
  it("pads book order to four digits, 1-based", () => {
    expect(padOrder(0)).toBe("0001");
    expect(padOrder(1312)).toBe("1313");
  });
  it("formats percentages and nulls", () => {
    expect(fmtPct(0.7234)).toBe("72.3%");
    expect(fmtPct(null)).toBe("—");
  });
  it("formats latency", () => {
    expect(fmtMs(512)).toBe("512 ms");
    expect(fmtMs(2340)).toBe("2.3 s");
    expect(fmtMs(null)).toBe("—");
  });
  it("formats bytes", () => {
    expect(fmtBytes(100)).toBe("100 B");
    expect(fmtBytes(2048)).toBe("2 KB");
    expect(fmtBytes(3 * 1024 * 1024)).toBe("3.0 MB");
  });
  it("formats deltas with sign", () => {
    expect(fmtDelta(0.05)).toBe("+5.0");
    expect(fmtDelta(-0.125)).toBe("-12.5");
    expect(fmtDelta(null)).toBe("—");
  });
  it("gets the last path segment", () => {
    expect(lastSegment(["单画师词典", "作者乙"])).toBe("作者乙");
    expect(lastSegment([])).toBe("");
  });
});
