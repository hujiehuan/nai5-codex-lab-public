import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreativeCapabilities, CreativePlan } from "../types-creative";
import { creativeApi } from "../api-creative";
import { invoke } from "../ipc";
import {
  CREATIVE_SIZES,
  creativeCapabilitiesIssue,
  creativePlanIssue,
  creativePlanRequest,
  creativeReferenceIssue,
  isCreativeCapabilities,
  creativeRenderIssue,
  creativeStartRequest,
  creativeTerminal,
  defaultCreativeForm,
  isCreativePlan,
  isCreativeRun,
  referenceInScope,
} from "./creative";
import { LAB_MAX_SEED } from "./lab";

vi.mock("../ipc", () => ({ invoke: vi.fn() }));
const plan: CreativePlan = {
  plan_id: "plan-one",
  parent_plan_id: null,
  created_at: "2026-09-10T00:00:00Z",
  character: "",
  requirements: "无人海滩，远景",
  provider_id: "text-model",
  skill_version: "nai5/test",
  concept: "潮水退去后，贝壳沿海岸线蜿蜒。",
  prompt: "empty beach, wide shot, seashells",
  negative: "people",
  artist: "full style recipe",
  artist_source: "pool",
  character_prompts: [],
  reference_ids: ["a", "b"],
  requirement_checks: [
    { requirement: "无人", evidence: "empty beach / negative people" },
  ],
  warnings: [],
  model: "nai-diffusion-5-full",
  size: "832x1216",
  steps: 28,
  parameters: {
    scale: 5,
    cfg: 0,
    sampler: "k_euler_ancestral",
    noise_schedule: "karras",
    other: "0",
  },
  free_only: true,
  llm_raw: "{}",
};
const cap: CreativeCapabilities = {
  version: 1,
  free_only: true,
  free_sizes: CREATIVE_SIZES.map(({ id, width, height }) => ({
    id,
    width,
    height,
  })),
  providers: ["text-model"],
  characters: [],
  model: "nai-diffusion-5-full",
  steps: 28,
  available: true,
  reason: null,
  render_available: true,
  render_reason: null,
  min_stamina_percent: 5,
  free_limitations: [],
};
beforeEach(() => vi.clearAllMocks());

describe("creative input boundary", () => {
  it("leaves blank character selection to the native plugin-pool boundary and preserves the original requirements", () => {
    const form = {
      ...defaultCreativeForm(),
      provider_id: "text-model",
      requirements: "海滩上跳舞\n必须为全身远景。",
      style_name: "  完整风格  ",
    };
    expect(form).toMatchObject({
      character: "",
      count: 1,
      score: true,
      artist_ratio: 50,
      reference_count: 4,
    });
    const req = creativePlanRequest(form);
    expect(req.character).toBe("");
    expect(req.requirements).toBe(form.requirements);
    expect(req.artist_ratio).toBe(0.5);
    expect(req.style_name).toBe("完整风格");
    expect(req).not.toHaveProperty("seed");
    expect(req).not.toHaveProperty("count");
  });
  it("rejects unknown orientation, oversized reference selection, and missing models before a billable call", () => {
    const form = { ...defaultCreativeForm(), provider_id: "model" };
    expect(() =>
      creativePlanRequest({ ...form, orientation: "paid" as never }),
    ).toThrow();
    expect(() =>
      creativePlanRequest({
        ...form,
        reference_ids: Array.from({ length: 7 }, (_, i) => String(i)),
      }),
    ).toThrow();
    expect(() =>
      creativePlanRequest({ ...form, reference_count: 2 }),
    ).toThrow();
    expect(() => creativePlanRequest({ ...form, provider_id: "" })).toThrow();
    expect(() => creativePlanRequest({ ...form, artist_ratio: NaN })).toThrow();
  });
  it("freezes scope and selected ids, matching path segments rather than string prefixes", () => {
    const form = {
      ...defaultCreativeForm(),
      provider_id: "model",
      scope: [["图包", "海"]],
      reference_ids: ["a", "a"],
    };
    const request = creativePlanRequest(form);
    form.scope[0][1] = "山";
    form.reference_ids.push("b");
    expect(request.scope).toEqual([["图包", "海"]]);
    expect(request.reference_ids).toEqual(["a"]);
    expect(referenceInScope({ path: ["图包", "海边"] }, [["图包", "海"]])).toBe(
      false,
    );
    expect(
      referenceInScope({ path: ["图包", "海", "夜晚"] }, [["图包", "海"]]),
    ).toBe(true);
  });
});

describe("stamina-only rendering", () => {
  it("keeps planning available while rendering capability is unavailable", () => {
    const value = {
      ...cap,
      render_available: false,
      render_reason: "免费资格未就绪",
    };
    expect(creativeCapabilitiesIssue(value)).toBeNull();
    expect(creativeRenderIssue(value)).toBe("免费资格未就绪");
    expect(creativeCapabilitiesIssue(null)).toBeTruthy();
    expect(
      creativeCapabilitiesIssue({ ...cap, free_only: false }),
    ).toBeTruthy();
    expect(creativeCapabilitiesIssue({ ...cap, free_sizes: [] })).toBeTruthy();
  });
  it("accepts only all three fixed free sizes and rejects higher steps or paid plan dimensions", () => {
    expect(CREATIVE_SIZES.map((item) => item.size)).toEqual([
      "832x1216",
      "1216x832",
      "1024x1024",
    ]);
    for (const size of CREATIVE_SIZES)
      expect(creativePlanIssue({ ...plan, size: size.size })).toBeNull();
    expect(
      creativePlanIssue({ ...plan, size: "1920x1088" as never }),
    ).toBeTruthy();
    expect(creativePlanIssue({ ...plan, steps: 29 })).toBeTruthy();
    expect(creativeRenderIssue({ ...cap, steps: 29 })).toBeTruthy();
    expect(creativePlanIssue({ ...plan, free_only: false })).toBeTruthy();
  });
  it("starts from the frozen plan, and same-concept redraw changes only rendering count/seed", async () => {
    const form = {
      ...defaultCreativeForm(),
      count: 3,
      seed: "72",
      score: false,
    };
    const normal = creativeStartRequest(plan, form, "id-normal");
    const again = creativeStartRequest(plan, form, "id-again", true);
    expect(normal).toEqual({
      plan_id: "plan-one",
      request_id: "id-normal",
      count: 3,
      seed: 72,
      score: false,
    });
    expect(again).toEqual({
      plan_id: "plan-one",
      request_id: "id-again",
      count: 1,
      seed: null,
      score: false,
    });
    await creativeApi.start(again);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("creative_start", {
      req: again,
    });
    expect(invoke).not.toHaveBeenCalledWith("creative_plan", expect.anything());
  });
  it("rejects fractional counts, invalid seeds and overflowing seed batches", () => {
    const form = defaultCreativeForm();
    expect(() =>
      creativeStartRequest(plan, { ...form, count: 1.5 }, "id"),
    ).toThrow();
    expect(() =>
      creativeStartRequest(plan, { ...form, seed: "NaN" }, "id"),
    ).toThrow();
    expect(() =>
      creativeStartRequest(
        plan,
        { ...form, count: 2, seed: String(LAB_MAX_SEED) },
        "id",
      ),
    ).toThrow();
    expect(
      creativeStartRequest(
        plan,
        { ...form, count: 8, seed: "invalid" },
        "id",
        true,
      ).seed,
    ).toBeNull();
  });
});

describe("records and edits", () => {
  it("rejects malformed persisted plans before entering editable state", () => {
    expect(isCreativePlan(plan)).toBe(true);
    expect(isCreativePlan({ ...plan, character_prompts: null })).toBe(false);
    expect(isCreativePlan({ ...plan, parameters: { scale: "5" } })).toBe(false);
    expect(
      isCreativePlan({ ...plan, requirement_checks: [{ requirement: "x" }] }),
    ).toBe(false);
    expect(isCreativePlan(null)).toBe(false);
    expect(isCreativeRun({ plan, images: [] })).toBe(false);
  });
  it("submits edits for a new reviewed version and keeps stop bound to the active id", async () => {
    const edit = {
      plan_id: "plan-one",
      prompt: "changed prompt",
      negative: "people",
      character_prompts: [],
    };
    await creativeApi.edit(edit);
    await creativeApi.cancel("active-run");
    expect(invoke).toHaveBeenNthCalledWith(1, "creative_edit", { req: edit });
    expect(invoke).toHaveBeenNthCalledWith(2, "creative_cancel", {
      run_id: "active-run",
    });
    expect(["done", "error", "cancelled"].every(creativeTerminal)).toBe(true);
    expect(creativeTerminal("running")).toBe(false);
  });
});

describe("full-reference compatibility", () => {
  it("keeps sample default and requires explicit full capability", () => {
    expect(defaultCreativeForm().reference_mode).toBe("sample");
    expect(creativeReferenceIssue(cap, "sample")).toBeNull();
    expect(creativeReferenceIssue(cap, "all")).toContain("不支持全量参考");
    expect(
      creativeReferenceIssue({ ...cap, full_reference_supported: true }, "all"),
    ).toBeNull();
    expect(
      isCreativeCapabilities({ ...cap, full_reference_supported: "true" }),
    ).toBe(false);
  });
  it("freezes all scope rather than sending the browser filtered list", () => {
    const form = {
      ...defaultCreativeForm(),
      provider_id: "text-model",
      reference_mode: "all" as const,
      scope: [["scope"]],
      reference_ids: ["priority"],
    };
    const req = creativePlanRequest(form);
    form.scope[0][0] = "changed";
    expect(req.reference_mode).toBe("all");
    expect(req.scope).toEqual([["scope"]]);
    expect(req.reference_ids).toEqual(["priority"]);
    expect(req).not.toHaveProperty("search");
    expect(req).not.toHaveProperty("references");
    expect(() =>
      creativePlanRequest({ ...form, reference_mode: "unknown" as never }),
    ).toThrow();
  });
  it("distinguishes legacy unknown borrowing from a reported empty subset and validates new histories", () => {
    expect(isCreativePlan(plan)).toBe(true);
    const all = {
      ...plan,
      reference_mode: "all" as const,
      reference_ids: Array.from({ length: 18 }, (_, i) => `ref-${i}`),
      used_reference_ids: ["ref-2", "ref-17"],
    };
    expect(isCreativePlan(all)).toBe(true);
    expect(creativePlanIssue(all)).toBeNull();
    expect(isCreativePlan({ ...all, used_reference_ids: [] })).toBe(true);
    expect(isCreativePlan({ ...all, used_reference_ids: null })).toBe(false);
    expect(isCreativePlan({ ...all, used_reference_ids: ["unknown"] })).toBe(
      false,
    );
    expect(
      isCreativePlan({ ...all, used_reference_ids: ["ref-2", "ref-2"] }),
    ).toBe(false);
    expect(isCreativePlan({ ...all, reference_mode: "unknown" })).toBe(false);
  });
});
