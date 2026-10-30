import { computed, nextTick, onScopeDispose, ref } from "vue";
import { defineStore } from "pinia";
import { api, errMessage } from "../api";
import { flushStudioWorkspace } from "../api-studio";
import type { AppUpdatePhase, AppUpdateStatus } from "../types";

const phases: AppUpdatePhase[] = ["idle", "checking", "up_to_date", "available", "preparing", "ready", "installing", "error", "cancelled"];
const active = (s: AppUpdateStatus | null) => !!s && ["checking", "preparing", "installing"].includes(s.phase);
const nullableString = (x: unknown) => x === null || typeof x === "string";
export function readUpdateStatus(value: unknown): AppUpdateStatus {
  const s = value as AppUpdateStatus | null;
  if (!s || typeof s !== "object" || Array.isArray(s) || !phases.includes(s.phase) ||
      typeof s.supported !== "boolean" || typeof s.current_version !== "string" || !s.current_version ||
      !nullableString(s.current_revision) || !nullableString(s.source_dir) || !nullableString(s.stage) ||
      !nullableString(s.message) || !nullableString(s.started_at) || typeof s.can_install !== "boolean" ||
      !Array.isArray(s.log_tail) || !s.log_tail.every((v) => typeof v === "string") ||
      !(s.candidate === null || (typeof s.candidate === "object" && !Array.isArray(s.candidate) &&
        [s.candidate.version, s.candidate.revision, s.candidate.tag].every((v) => typeof v === "string" && !!v)))) {
    throw new Error("软件更新状态格式无效，请重试读取。");
  }
  if (["available", "ready"].includes(s.phase) && !s.candidate)
    throw new Error("软件更新缺少目标版本，请重新检查。");
  return s;
}

export const updatePhaseLabels: Record<AppUpdatePhase, string> = {
  idle: "尚未检查更新", checking: "正在检查更新…", up_to_date: "当前已是最新稳定版",
  available: "发现新版本", preparing: "正在准备更新…", ready: "更新已准备好",
  installing: "正在安装并重启…", error: "更新未完成", cancelled: "已取消更新",
};

export const useUpdater = defineStore("updater", () => {
  const status = ref<AppUpdateStatus | null>(null);
  const readError = ref<string | null>(null), actionError = ref<string | null>(null);
  const loading = ref(false), pending = ref<"choose" | "check" | "prepare" | "cancel" | "install" | null>(null);
  const installHold = ref(false);
  let generation = 0, disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let refreshPromise: Promise<boolean> | null = null;
  const installing = computed(() => installHold.value || status.value?.phase === "installing");
  const busy = computed(() => !!pending.value || active(status.value) || installing.value);
  const usable = computed(() => !!status.value?.supported && !readError.value && !busy.value);
  const canCheck = computed(() => usable.value && !!status.value?.source_dir);
  const canPrepare = computed(() => usable.value && !!status.value?.candidate && ["available", "error", "cancelled"].includes(status.value.phase));
  const canInstall = computed(() => usable.value && status.value?.phase === "ready" && status.value.can_install);
  const canCancel = computed(() => !pending.value && ["checking", "preparing"].includes(status.value?.phase ?? ""));
  const label = computed(() => readError.value ? "更新状态读取失败" : pending.value === "check" ? "正在检查更新…" :
    pending.value === "choose" ? "请选择源码目录…" : pending.value === "prepare" ? "正在启动更新准备…" :
      pending.value === "cancel" ? "正在取消…" : pending.value === "install" ? "正在保存工作区并准备安装…" :
        status.value ? updatePhaseLabels[status.value.phase] : "正在读取更新状态…");

  function schedule() {
    clearTimeout(timer);
    if (!disposed && (active(status.value) || installHold.value)) timer = setTimeout(() => void refresh(), 1000);
  }
  function refresh(): Promise<boolean> {
    if (pending.value || disposed) return Promise.resolve(false);
    if (refreshPromise) return refreshPromise;
    const epoch = ++generation;
    loading.value = true;
    const task = (async () => {
      try {
        const value = readUpdateStatus(await api.appUpdateStatus());
        if (epoch !== generation || disposed) return false;
        status.value = value; readError.value = null;
        return true;
      } catch (e) {
        if (epoch === generation && !disposed) readError.value = errMessage(e);
        return false;
      } finally {
        if (epoch === generation && !disposed) { loading.value = false; schedule(); }
      }
    })();
    refreshPromise = task;
    void task.finally(() => { if (refreshPromise === task) refreshPromise = null; });
    return task;
  }
  async function action(kind: "choose" | "check" | "prepare" | "cancel", call: () => Promise<AppUpdateStatus>) {
    if (disposed || pending.value || installing.value || (kind === "cancel" ? !canCancel.value : busy.value)) return false;
    ++generation; refreshPromise = null; clearTimeout(timer); loading.value = false;
    pending.value = kind; actionError.value = null;
    try {
      status.value = readUpdateStatus(await call()); readError.value = null;
      return true;
    } catch (e) { actionError.value = errMessage(e); return false; }
    finally { pending.value = null; schedule(); }
  }
  const chooseSource = () => status.value?.supported && !readError.value ? action("choose", api.appUpdateChooseSource) : Promise.resolve(false);
  const check = () => canCheck.value ? action("check", api.appUpdateCheck) : Promise.resolve(false);
  const prepare = () => canPrepare.value ? action("prepare", api.appUpdatePrepare) : Promise.resolve(false);
  const cancel = () => action("cancel", api.appUpdateCancel);
  async function install() {
    if (!canInstall.value || disposed) return false;
    ++generation; refreshPromise = null; clearTimeout(timer); loading.value = false;
    pending.value = "install"; installHold.value = true; actionError.value = null;
    try {
      // Paint the global interaction lock before draining the last Studio save.
      await nextTick();
      await flushStudioWorkspace();
      status.value = readUpdateStatus(await api.appUpdateInstall());
      readError.value = null;
      return true;
    } catch (e) { actionError.value = errMessage(e); return false; }
    finally { pending.value = null; installHold.value = false; schedule(); }
  }
  onScopeDispose(() => { disposed = true; ++generation; clearTimeout(timer); });
  return { status, loading, pending, readError, actionError, installing, busy, label,
    canCheck, canPrepare, canInstall, canCancel, refresh, chooseSource, check, prepare, cancel, install };
});
