import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { AppUpdateStatus } from "../types";

vi.mock("../api", () => ({ api: {
  appUpdateStatus: vi.fn(), appUpdateChooseSource: vi.fn(), appUpdateCheck: vi.fn(),
  appUpdatePrepare: vi.fn(), appUpdateCancel: vi.fn(), appUpdateInstall: vi.fn(),
}, errMessage: (e: unknown) => e instanceof Error ? e.message : String(e) }));
vi.mock("../api-studio", () => ({ flushStudioWorkspace: vi.fn() }));
import { api } from "../api";
import { flushStudioWorkspace } from "../api-studio";
import { readUpdateStatus, useUpdater } from "./updater";

const candidate = { version: "0.6.0", revision: "a".repeat(40), tag: "v0.6.0" };
function status(patch: Partial<AppUpdateStatus> = {}): AppUpdateStatus {
  return { supported: true, phase: "idle", current_version: "0.5.9", current_revision: null,
    source_dir: "/Users/test/Project With Spaces/nai5-codex-lab", candidate: null, stage: null,
    message: null, log_tail: [], started_at: null, can_install: false, ...patch };
}
function deferred<T>() { let resolve!: (v: T) => void, reject!: (e: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
let store: ReturnType<typeof useUpdater>;
beforeEach(() => {
  vi.useFakeTimers(); vi.resetAllMocks(); setActivePinia(createPinia()); store = useUpdater();
  vi.mocked(api.appUpdateStatus).mockResolvedValue(status());
  vi.mocked(flushStudioWorkspace).mockResolvedValue();
});
afterEach(() => { store.$dispose(); vi.useRealTimers(); });

describe("software update state", () => {
  it("reads local status without checking, building or installing automatically", async () => {
    await store.refresh(); await vi.advanceTimersByTimeAsync(10_000);
    expect(api.appUpdateStatus).toHaveBeenCalledTimes(1);
    expect(api.appUpdateCheck).not.toHaveBeenCalled(); expect(api.appUpdatePrepare).not.toHaveBeenCalled();
    expect(api.appUpdateInstall).not.toHaveBeenCalled(); expect(store.status?.current_revision).toBeNull();
  });
  it("rejects malformed arrays, flags, phases and missing target metadata", () => {
    for (const bad of [{}, status({ log_tail: {} as string[] }), status({ phase: "ready" }),
      status({ phase: "unknown" as AppUpdateStatus["phase"] }), status({ supported: "yes" as unknown as boolean })])
      expect(() => readUpdateStatus(bad)).toThrow();
    expect(readUpdateStatus(status({ phase: "ready", candidate, can_install: true })).candidate).toEqual(candidate);
  });
  it("never turns a failed or malformed status read into an up-to-date claim", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "up_to_date" })).mockRejectedValueOnce(Error("read unavailable"));
    await store.refresh(); await store.refresh();
    expect(store.label).toBe("更新状态读取失败"); expect(store.canCheck).toBe(false);
    expect(store.readError).toBe("read unavailable"); await store.refresh(); expect(store.readError).toBeNull();
  });
  it("coalesces status reads and suppresses an old poll arriving after a new check", async () => {
    await store.refresh(); const old = deferred<AppUpdateStatus>(); vi.mocked(api.appUpdateStatus).mockReturnValueOnce(old.promise);
    const poll = store.refresh(); const second = store.refresh(); expect(api.appUpdateStatus).toHaveBeenCalledTimes(2);
    vi.mocked(api.appUpdateCheck).mockResolvedValueOnce(status({ phase: "checking" })); await store.check();
    old.resolve(status({ phase: "up_to_date" })); await Promise.all([poll, second]);
    expect(store.status?.phase).toBe("checking"); expect(store.readError).toBeNull();
  });
  it("deduplicates clicks and continues polling without a mounted settings view", async () => {
    await store.refresh(); const check = deferred<AppUpdateStatus>(); vi.mocked(api.appUpdateCheck).mockReturnValueOnce(check.promise);
    const first = store.check(); expect(await store.check()).toBe(false); expect(store.pending).toBe("check");
    check.resolve(status({ phase: "checking" })); await first;
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "available", candidate }));
    await vi.advanceTimersByTimeAsync(1000); expect(store.canPrepare).toBe(true);
    await vi.advanceTimersByTimeAsync(5000); expect(api.appUpdateStatus).toHaveBeenCalledTimes(2);
    expect(api.appUpdateCheck).toHaveBeenCalledTimes(1);
  });
  it("preserves the source when the native picker is cancelled", async () => {
    await store.refresh(); vi.mocked(api.appUpdateChooseSource).mockResolvedValueOnce(status());
    expect(await store.chooseSource()).toBe(true); expect(store.status?.source_dir).toContain("Project With Spaces");
    expect(api.appUpdateCheck).not.toHaveBeenCalled();
  });
  it("keeps polling after a transient read failure but disallows another operation", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "preparing", candidate })).mockRejectedValueOnce(Error("temporary"))
      .mockResolvedValueOnce(status({ phase: "ready", candidate, can_install: true }));
    await store.refresh(); await vi.advanceTimersByTimeAsync(1000);
    expect(store.readError).toBe("temporary"); expect(await store.check()).toBe(false);
    await vi.advanceTimersByTimeAsync(1000); expect(store.canInstall).toBe(true); expect(store.readError).toBeNull();
  });
  it("cancels only checking or preparation and never installs a failed build", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "preparing", candidate })); await store.refresh();
    vi.mocked(api.appUpdateCancel).mockResolvedValueOnce(status({ phase: "cancelled", candidate }));
    expect(await store.cancel()).toBe(true); expect(store.canPrepare).toBe(true); expect(await store.cancel()).toBe(false);
    vi.mocked(api.appUpdatePrepare).mockResolvedValueOnce(status({ phase: "error", candidate, message: "Cargo unavailable" })); await store.prepare();
    expect(await store.install()).toBe(false); expect(api.appUpdateInstall).not.toHaveBeenCalled();
  });
  it("locks immediately, waits for the final workspace save and installs only once", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "ready", candidate, can_install: true })); await store.refresh();
    const save = deferred<void>(); vi.mocked(flushStudioWorkspace).mockReturnValueOnce(save.promise);
    vi.mocked(api.appUpdateInstall).mockResolvedValueOnce(status({ phase: "installing", candidate }));
    const install = store.install(); expect(store.installing).toBe(true); expect(await store.install()).toBe(false);
    await Promise.resolve(); expect(api.appUpdateInstall).not.toHaveBeenCalled();
    save.resolve(); expect(await install).toBe(true); expect(api.appUpdateInstall).toHaveBeenCalledTimes(1);
    expect(store.installing).toBe(true); expect(await store.check()).toBe(false); expect(await store.cancel()).toBe(false);
  });
  it("a save failure prevents native install, unlocks and permits an explicit retry", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "ready", candidate, can_install: true })); await store.refresh();
    vi.mocked(flushStudioWorkspace).mockRejectedValueOnce(Error("workspace save failed"));
    expect(await store.install()).toBe(false); expect(store.installing).toBe(false); expect(store.actionError).toBe("workspace save failed");
    expect(api.appUpdateInstall).not.toHaveBeenCalled();
    vi.mocked(api.appUpdateInstall).mockResolvedValueOnce(status({ phase: "installing", candidate }));
    expect(await store.install()).toBe(true);
  });
  it("native backup failure leaves the ready build and unlocks the existing app", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "ready", candidate, can_install: true })); await store.refresh();
    vi.mocked(api.appUpdateInstall).mockRejectedValueOnce(Error("backup failed"));
    expect(await store.install()).toBe(false); expect(store.installing).toBe(false); expect(store.canInstall).toBe(true);
    expect(store.actionError).toBe("backup failed");
  });
  it("honours unsupported platforms and an unwritable install location", async () => {
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ supported: false })); await store.refresh();
    expect(await store.check()).toBe(false); expect(await store.chooseSource()).toBe(false);
    vi.mocked(api.appUpdateStatus).mockResolvedValueOnce(status({ phase: "ready", candidate, can_install: false })); await store.refresh();
    expect(await store.install()).toBe(false); expect(api.appUpdateInstall).not.toHaveBeenCalled();
  });
});
