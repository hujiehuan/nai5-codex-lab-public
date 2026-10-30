import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
vi.mock("../api", () => ({
  api: { pluginModelSelection: vi.fn(), pluginProviders: vi.fn() },
  errMessage: (e: any) => e.message || String(e),
}));
import { api } from "../api";
import { usePluginModels } from "./plugin-models";
const selection = {
  enabled: true,
  selected: ["b", "gone"],
  available: ["a", "b"],
  effective: ["b"],
  missing: ["gone"],
};
beforeEach(() => {
  setActivePinia(createPinia());
  vi.mocked(api.pluginModelSelection).mockReset();
  vi.mocked(api.pluginProviders).mockReset();
});
describe("model selection refresh", () => {
  it("shares the plugin shortlist and keeps all-missing empty", async () => {
    vi.mocked(api.pluginModelSelection)
      .mockResolvedValueOnce(selection)
      .mockResolvedValueOnce({
        ...selection,
        selected: ["gone"],
        effective: [],
      });
    const store = usePluginModels();
    expect(store.ready).toBe(false);
    await store.refresh();
    expect(store.ids).toEqual(["b"]);
    expect(store.issue(["a"])).toContain("名单");
    expect(store.issue(["b"])).toBeNull();
    await store.refresh();
    expect(store.ids).toEqual([]);
    expect(store.ready).toBe(true);
    expect(api.pluginProviders).not.toHaveBeenCalled();
  });
  it.each([
    { code: "network", message: "offline" },
    { code: "plugin_rejected", message: "HTTP 401" },
    {},
  ])("never widens after failed refresh %j", async (error) => {
    vi.mocked(api.pluginModelSelection)
      .mockResolvedValueOnce(selection)
      .mockRejectedValueOnce(error);
    const store = usePluginModels();
    await store.refresh();
    expect(await store.refresh()).toBe(false);
    expect(store.ids).toEqual(["b"]);
    expect(store.ready).toBe(false);
    expect(store.issue(["b"])).toContain("读取失败");
    expect(api.pluginProviders).not.toHaveBeenCalled();
  });
  it("rejects malformed success instead of falling back", async () => {
    vi.mocked(api.pluginModelSelection).mockResolvedValue({});
    const store = usePluginModels();
    await store.refresh();
    expect(store.ready).toBe(false);
    expect(api.pluginProviders).not.toHaveBeenCalled();
  });
  it.each([
    { code: "not_found" },
    { code: "plugin_rejected", message: "HTTP 404: old plugin" },
    { code: "plugin_rejected", message: "HTTP 405: old plugin" },
  ])("uses legacy providers only when unsupported %j", async (error) => {
    vi.mocked(api.pluginModelSelection).mockRejectedValue(error);
    vi.mocked(api.pluginProviders).mockResolvedValue(["a", "b"]);
    const store = usePluginModels();
    await store.refresh();
    expect(store.ready).toBe(true);
    expect(store.legacy).toBe(true);
    expect(store.ids).toEqual(["a", "b"]);
  });
  it("coalesces simultaneous page refresh and blocks use while reading", async () => {
    let finish!: (v: unknown) => void;
    vi.mocked(api.pluginModelSelection).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const store = usePluginModels();
    const first = store.refresh(),
      second = store.refresh();
    expect(api.pluginModelSelection).toHaveBeenCalledTimes(1);
    expect(store.ready).toBe(false);
    expect(store.issue(["b"])).toContain("读取完成");
    finish(selection);
    await Promise.all([first, second]);
    expect(store.ready).toBe(true);
  });
});
