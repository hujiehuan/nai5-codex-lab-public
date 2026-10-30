import { describe, expect, it } from "vitest";
import { cycleDecision, keyToDecision, shiftCounts } from "./decision";

describe("decision cycle", () => {
  it("cycles undecided → include → exclude → undecided", () => {
    expect(cycleDecision("undecided")).toBe("include");
    expect(cycleDecision("include")).toBe("exclude");
    expect(cycleDecision("exclude")).toBe("undecided");
  });

  it("returns to the start after three steps", () => {
    let d: "include" | "exclude" | "undecided" = "include";
    for (let i = 0; i < 3; i += 1) d = cycleDecision(d);
    expect(d).toBe("include");
  });

  it("maps A/X/U keys", () => {
    expect(keyToDecision("a")).toBe("include");
    expect(keyToDecision("X")).toBe("exclude");
    expect(keyToDecision("u")).toBe("undecided");
    expect(keyToDecision("z")).toBeNull();
  });

  it("shifts counts without going negative", () => {
    expect(shiftCounts({ include: 1, exclude: 2, undecided: 3 }, "undecided", "include")).toEqual({ include: 2, exclude: 2, undecided: 2 });
    expect(shiftCounts({ include: 0, exclude: 0, undecided: 0 }, "include", "exclude")).toEqual({ include: 0, exclude: 1, undecided: 0 });
    expect(shiftCounts({ include: 1, exclude: 1, undecided: 1 }, "include", "include")).toEqual({ include: 1, exclude: 1, undecided: 1 });
  });
});
