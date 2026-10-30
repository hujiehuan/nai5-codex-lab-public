import { describe, it, expect, vi } from "vitest";
import { reactive } from "vue";
import { Channel } from "@tauri-apps/api/core";
import { snapshot } from "./ipc";
describe("IPC snapshot", () => {
  it("clones nested Vue lists and ranked fields without changing their order", () => {
    const draft = reactive({
      updates: { models: ["B", "A"], nested: [{ values: ["x"] }] },
    });
    const plain = snapshot(draft);
    expect(structuredClone(plain)).toEqual(draft);
    draft.updates.models.reverse();
    expect(plain.updates.models).toEqual(["B", "A"]);
  });
  it("retains the Tauri channel wire representation rather than copying implementation fields", () => {
    vi.stubGlobal("window", {
      __TAURI_INTERNALS__: { transformCallback: () => 4 },
    });
    try {
      expect(
        snapshot({
          on_event: new Channel(),
          req: reactive({ list: ["one"] }),
        }),
      ).toEqual({ on_event: "__CHANNEL__:4", req: { list: ["one"] } });
    } finally {
      vi.unstubAllGlobals();
    }
  });
  it("rejects cyclic data before transport", () => {
    const x: Record<string, unknown> = {};
    x.self = x;
    expect(() => snapshot(x)).toThrow();
  });
});
