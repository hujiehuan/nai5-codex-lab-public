import { describe, expect, it } from "vitest";
import { pairMetrics, pairKey, studioSeed, studioTab, validCapabilities, validPlan, validPlanForRequest, validRun, validPreparedInput } from "./studio";

describe("studio comparison truth", () => {
  const images = [{ id: "a", group: "A" }, { id: "b", group: "A" }, { id: "c", group: "B" }, { id: "d", group: "" }];
  it("counts only labelled pairs and does not treat missing predictions as negative", () => {
    expect(pairMetrics(images, [{ left_id: "a", right_id: "b", same: true }, { left_id: "a", right_id: "c", same: false }, { left_id: "a", right_id: "d", same: true }])).toEqual({ labelled: 3, evaluated: 2, correct: 2, accuracy: 1, precision: 1, recall: 1 });
  });
  it("does not double-count reverse or duplicated pair judgments", () => {
    expect(pairKey("a", "b")).toBe(pairKey("b", "a"));
    const m = pairMetrics(images, [{ left_id: "a", right_id: "b", same: true }, { left_id: "b", right_id: "a", same: true }, { left_id: "a", right_id: "c", same: true }]);
    expect(m.evaluated).toBe(2); expect(m.accuracy).toBe(.5); expect(m.precision).toBe(.5);
  });
  it("returns unknown metrics if there is no labelled ground truth", () => {
    expect(pairMetrics([{ id: "a", group: " " }, { id: "b", group: "" }], [{ left_id: "a", right_id: "b", same: false }])).toEqual({ labelled: 0, evaluated: 0, correct: 0, accuracy: null, precision: null, recall: null });
  });
  it("computes recall from false negatives rather than raw cosine scores", () => {
    const m = pairMetrics(images, [{ left_id: "a", right_id: "b", same: false }, { left_id: "a", right_id: "c", same: false }]);
    expect(m.accuracy).toBe(.5); expect(m.recall).toBe(0); expect(m.precision).toBeNull();
  });
});
describe("image+text reference completeness", () => {
  const prepared = { input_id: "frozen", character: "synthetic", characters: [], reference_ids: ["a", "b"], reference_count: 2, reference_image_count: 2 };
  const image = (id: string) => ({ entry_id: id, ordinal: 0, mime: "image/png", sha256: "a".repeat(64), raw_tag: null });
  const plan = () => ({ plan_id: "new-plan", mode: "draw", plans: [{ prompt: "synthetic", negative: "", reference_ids: ["a", "b"] }],
    settings: { reference_image_count: 2, reference_images: [image("a"), image("b")] }, warnings: [] });
  it("refuses to start creation when a frozen pack lacks its image", () => {
    expect(validPreparedInput(prepared, "draw")).toBe(true);
    for (const reference_image_count of [undefined, null, 1, "2", -1]) expect(validPreparedInput({ ...prepared, reference_image_count }, "draw")).toBe(false);
    expect(validPreparedInput({ ...prepared, reference_ids: ["a", "a"] }, "draw")).toBe(false);
  });
  it("keeps reverse input independent from pack image capability", () => {
    expect(validPreparedInput({ ...prepared, reference_count: 0, reference_ids: [], reference_image_count: undefined }, "reverse")).toBe(true);
    expect(validPreparedInput(prepared, "reverse")).toBe(false);
  });
  it("accepts full ordered image manifests and rejects missing, reordered or substituted sources", () => {
    expect(validPlan(plan())).toBe(true);
    for (const images of [[image("a")], [image("b"), image("a")], [image("a"), image("unknown")], [image("a"), image("a")], {}]) {
      const value = plan(); Object.assign(value.settings, { reference_images: images });
      expect(validPlan(value)).toBe(false);
    }
    const invalid = plan(); invalid.settings.reference_images[0].sha256 = "broken";
    expect(validPlan(invalid)).toBe(false);
  });
  it("reads old text-only plans without inventing an image count", () => {
    const old = { ...plan(), settings: {} }; expect(validPlan(old)).toBe(true);
    expect(validPlan({ ...old, settings: { reference_image_count: 2 } })).toBe(false);
  });
  it("does not treat a string capability as visual support", () => {
    const caps = { version: 1, providers: [], characters: [], free_sizes: [], available: true, model: "fixture", steps: 28, defaults: {}, templates: {} };
    expect(validCapabilities(caps)).toBe(true); // legacy server can still serve reverse/history
    expect(validCapabilities({ ...caps, reference_images_supported: true })).toBe(true);
    expect(validCapabilities({ ...caps, reference_images_supported: "true" })).toBe(false);
    expect(validCapabilities({ ...caps, reference_text_supported: true })).toBe(true);
    expect(validCapabilities({ ...caps, reference_text_supported: "true" })).toBe(false);
  });
  it("requires explicit text preparation to report zero images and preserves old image preparation", () => {
    const text = { ...prepared, reference_content: "text", reference_image_count: 0 };
    expect(validPreparedInput(text, "draw", "text")).toBe(true);
    for (const count of [undefined, null, "0", 1, -1]) expect(validPreparedInput({ ...text, reference_image_count: count }, "draw", "text")).toBe(false);
    for (const content of [undefined, "image_text", "other"]) expect(validPreparedInput({ ...text, reference_content: content }, "draw", "text")).toBe(false);
    expect(validPreparedInput(text, "draw", "image_text")).toBe(false);
    expect(validPreparedInput(prepared, "draw", "text")).toBe(false);
    expect(validPreparedInput(prepared, "draw", "image_text")).toBe(true);
  });
  it("validates explicit text plans without permitting images, missing fields or changing frozen sources", () => {
    const text = { ...plan(), settings: { reference_content: "text", reference_image_count: 0, reference_images: [] } };
    expect(validPlan(text)).toBe(true);
    expect(validPlanForRequest(text, { mode: "draw", reference_content: "text" })).toBe(true);
    for (const settings of [{ reference_content: "text" }, { ...text.settings, reference_image_count: 1 },
      { ...text.settings, reference_images: [image("a")] }, { ...text.settings, reference_content: "other" }]) expect(validPlan({ ...text, settings })).toBe(false);
    for (const ids of [[], ["a", "a"], {}, ["a", ""]]) expect(validPlan({ ...text, plans: [{ ...text.plans[0], reference_ids: ids }] })).toBe(false);
    expect(validPlan({ ...text, plans: [...text.plans, { ...text.plans[0], reference_ids: ["b", "a"] }] })).toBe(false);
    expect(validPlanForRequest(text, { mode: "draw", reference_content: "image_text" })).toBe(false);
    expect(validPlanForRequest(text, { mode: "draw" })).toBe(false);
    expect(validPlanForRequest(plan(), { mode: "draw", reference_content: "text" })).toBe(false);
  });
  it("keeps legacy text history readable but never revives it as a new image request", () => {
    const legacy = { ...plan(), settings: {} };
    expect(validPlan(legacy)).toBe(true);
    expect(validPlanForRequest(legacy, { mode: "draw" })).toBe(false);
    expect(validPlanForRequest(legacy, { mode: "draw", reference_content: "image_text" })).toBe(false);
    expect(validPlanForRequest(legacy, { mode: "draw", reference_content: "text" })).toBe(false);
    expect(validPlanForRequest(plan(), { mode: "draw", reference_content: "image_text" })).toBe(true);
    for (const content of [["image_text"], ["text"], {}, null, true]) {
      const imagePlan = plan();
      expect(validPlan({ ...imagePlan, settings: { ...imagePlan.settings, reference_content: content } })).toBe(false);
    }
    expect(validPlanForRequest(plan(), { mode: "reverse", reference_content: "image_text" })).toBe(false);
  });
});
describe("studio input and boundary guards", () => {
  it("accepts blank/random and unsigned seeds without truncation", () => {
    expect(studioSeed(" ")).toBeNull(); expect(studioSeed("0")).toBe(0); expect(studioSeed("4294967295")).toBe(4294967295);
    for (const invalid of ["-1", "1.5", "4294967296", "Infinity", "1e2"]) expect(() => studioSeed(invalid)).toThrow();
  });
  it("normalizes only the supported route modes", () => { expect(studioTab("review")).toBe("review"); expect(studioTab("invalid")).toBe("draw"); });
  it("rejects {} and malformed lists instead of reporting zero/empty success", () => {
    expect(validCapabilities({})).toBe(false); expect(validCapabilities({ version: 1, providers: {}, characters: [], free_sizes: [], available: true, model: "x", steps: 28, defaults: {}, templates: {} })).toBe(false);
    expect(validPlan({ plan_id: "a", mode: "draw", plans: [], settings: {}, warnings: [] })).toBe(false);
    expect(validRun({ run_id: "a", plan_id: "p", status: "done", images: {}, warnings: [], anlas_spent: null })).toBe(false);
    expect(validRun({ run_id: "a", plan_id: "p", status: "done", images: [], warnings: [], anlas_spent: null })).toBe(true);
  });
});
