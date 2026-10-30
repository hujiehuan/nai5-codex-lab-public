import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("./ipc", () => ({ invoke, snapshot: (value: unknown) => JSON.parse(JSON.stringify(value)) }));
beforeEach(() => { invoke.mockReset(); vi.resetModules(); });

it("waits for every queued workspace write and freezes the snapshot at enqueue", async () => {
  const { studioApi, flushStudioWorkspace } = await import("./api-studio");
  let finish!: () => void;
  invoke.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
  const value = { selected_id: "first" };
  const a = studioApi.saveWorkspace(value);
  const b = studioApi.saveWorkspace({ selected_id: "last" });
  value.selected_id = "changed";
  let flushed = false;
  const flushing = flushStudioWorkspace().then(() => { flushed = true; });
  await Promise.resolve(); await Promise.resolve();
  expect(invoke).toHaveBeenCalledWith("studio_workspace_save", { state: { selected_id: "first" } });
  expect(flushed).toBe(false);
  finish(); await Promise.all([a, b, flushing]);
  expect(invoke).toHaveBeenLastCalledWith("studio_workspace_save", { state: { selected_id: "last" } });
  expect(flushed).toBe(true);
});

it("preserves a failed save for installation, and permits recovery by a successful save", async () => {
  const { studioApi, flushStudioWorkspace } = await import("./api-studio");
  invoke.mockRejectedValueOnce(new Error("disk full")).mockResolvedValue(undefined);
  await expect(studioApi.saveWorkspace({ rows: [1] })).rejects.toThrow("disk full");
  await expect(flushStudioWorkspace()).rejects.toThrow("disk full");
  await studioApi.saveWorkspace({ rows: [1, 2] });
  await expect(flushStudioWorkspace()).resolves.toBeUndefined();
});
