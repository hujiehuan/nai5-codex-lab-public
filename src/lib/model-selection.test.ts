import { describe, expect, it } from "vitest";
import {
  missingModelIds,
  modelSelectionIssue,
  parseModelSelection,
  unsupportedModelSelection,
} from "./model-selection";
const valid = {
  enabled: true,
  selected: ["b", "gone", "a"],
  available: ["a", "b", "c"],
  effective: ["b", "a"],
  missing: ["gone"],
};
describe("plugin-owned model selection", () => {
  it("keeps configured order and absent saved IDs without substitution", () => {
    expect(parseModelSelection(valid)).toEqual(valid);
    expect(missingModelIds(["b", "gone", "gone", ""], valid.effective)).toEqual(
      ["gone"],
    );
    expect(modelSelectionIssue(["gone"], valid.effective)).toContain(
      "旧值已保留",
    );
    expect(modelSelectionIssue(["b", "a"], valid.effective)).toBeNull();
  });
  it("a fully missing configured list stays empty", () => {
    const v = { ...valid, selected: ["gone"], effective: [] };
    expect(parseModelSelection(v).effective).toEqual([]);
    expect(() =>
      parseModelSelection({ ...v, effective: v.available }),
    ).toThrow();
  });
  it("disabled uses the full catalog, preserving saved shortlist for later", () => {
    expect(
      parseModelSelection({
        ...valid,
        enabled: false,
        effective: valid.available,
      }).effective,
    ).toEqual(valid.available);
  });
  it.each([
    null,
    {},
    [],
    { ...valid, enabled: "yes" },
    { ...valid, selected: [] },
    { ...valid, available: {} },
    { ...valid, selected: ["b", "b"] },
    { ...valid, missing: [] },
    { ...valid, effective: ["c"] },
    { ...valid, available: [""] },
  ])("rejects malformed or inconsistent data %j", (v) =>
    expect(() => parseModelSelection(v)).toThrow(),
  );
  it("only unsupported endpoints permit fallback; auth/network/invalid data do not", () => {
    expect(unsupportedModelSelection({ code: "not_found" })).toBe(true);
    expect(
      unsupportedModelSelection({
        code: "plugin_rejected",
        message: "HTTP 404: missing",
      }),
    ).toBe(true);
    expect(
      unsupportedModelSelection({
        code: "plugin_rejected",
        message: "HTTP 405: method",
      }),
    ).toBe(true);
    for (const e of [
      { code: "plugin_rejected", message: "HTTP 401" },
      { code: "network" },
      new Error("HTTP 404 payload invalid"),
      { code: "plugin_rejected", message: "HTTP 500" },
    ])
      expect(unsupportedModelSelection(e)).toBe(false);
  });
});
