import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { StudioPluginSkill } from "../types-studio";
vi.mock("../api-studio", () => ({ studioApi: { syncSkills: vi.fn() } }));
vi.mock("../api", () => ({ errMessage: (e: Error) => e.message || String(e) }));
import { studioApi } from "../api-studio";
import { usePluginSkills } from "./plugin-skills";

function sample(version = "v1"): StudioPluginSkill { return { version: 1, kind: "draw", enabled: true, selected_id: version,
  skill: { id: `plugin-skill-${"a".repeat(64)}`, name: "合成资料", version, sha256: "b".repeat(64), text: "sample",
    source: "https://example.invalid", builtin: false, bytes: 6 } }; }
beforeEach(() => { setActivePinia(createPinia()); vi.mocked(studioApi.syncSkills).mockReset(); });
describe("plugin skill refresh", () => {
  it("reads the selected version anew and removes stale success after an error", async () => {
    const store = usePluginSkills();
    expect(store.states.draw.value).toBeNull();
    vi.mocked(studioApi.syncSkills).mockResolvedValueOnce(sample()).mockResolvedValueOnce(sample("v2")).mockRejectedValueOnce(new Error("HTTP 401"));
    const first = await store.refresh("draw");
    expect(first.skill?.version).toBe("v1");
    expect((await store.refresh("draw")).skill?.version).toBe("v2");
    await expect(store.refresh("draw")).rejects.toThrow("401");
    expect(store.states.draw).toEqual({ value: null, loading: false, error: "HTTP 401" });
    expect(first.skill?.version).toBe("v1");
    expect(studioApi.syncSkills).toHaveBeenCalledTimes(3);
  });
  it("late reads cannot replace a newer displayed version or a newer failure", async () => {
    let first!: (s: StudioPluginSkill) => void;
    vi.mocked(studioApi.syncSkills).mockImplementationOnce(() => new Promise((resolve) => { first = resolve; })).mockResolvedValueOnce(sample("v2"));
    const store = usePluginSkills(), older = store.refresh("draw"), newer = store.refresh("draw");
    expect(store.states.draw.loading).toBe(true);
    expect(store.states.draw.value).toBeNull();
    await newer;
    first(sample("v1"));
    expect((await older).skill?.version).toBe("v1");
    expect(store.states.draw.value?.skill?.version).toBe("v2");
  });
  it("separates draw from reverse and treats disable as a real empty success", async () => {
    const reverse: StudioPluginSkill = { version: 1, kind: "reverse", enabled: false, selected_id: "", skill: null };
    vi.mocked(studioApi.syncSkills).mockImplementation(async (kind) => kind === "reverse" ? reverse : sample());
    const store = usePluginSkills();
    await Promise.all([store.refresh("draw"), store.refresh("reverse")]);
    expect(store.states.draw.value?.enabled).toBe(true);
    expect(store.states.reverse).toEqual({ value: reverse, loading: false, error: null });
  });
  it("malformed and unsupported endpoint results never substitute the builtin", async () => {
    vi.mocked(studioApi.syncSkills).mockResolvedValueOnce({} as StudioPluginSkill).mockRejectedValueOnce(new Error("HTTP 404"));
    const store = usePluginSkills();
    await expect(store.refresh("draw")).rejects.toThrow("不完整");
    await expect(store.refresh("draw")).rejects.toThrow("404");
    expect(store.states.draw.value).toBeNull();
  });
});
