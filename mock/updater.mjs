// Synthetic updater only: never executes Git, toolchains, backups or installation.
import { readFileSync } from "node:fs";
export const appVersion = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const states = new Map();
export const updaterCalls = [];
const target = { version: "9.9.9", revision: "b".repeat(40), tag: "v9.9.9" };
const source = "/Users/offline/Projects/My Source/nai5-codex-lab";
function initial(scenario) {
  const value = { supported: true, phase: "idle", current_version: appVersion, current_revision: "a".repeat(40), source_dir: null,
    candidate: null, stage: null, message: null, log_tail: [], started_at: null, can_install: false, polls: 0 };
  if (["update-ready", "update-install-error", "update-read-error", "update-unwritable"].includes(scenario)) Object.assign(value, { source_dir: source, phase: "ready", candidate: { ...target }, can_install: true });
  if (scenario === "update-unwritable") Object.assign(value, { can_install: false, message: "当前应用位于只读位置，请移至可写的 Applications 目录。" });
  if (scenario === "update-unsupported") Object.assign(value, { supported: false, message: "内置软件更新仅支持 macOS 已安装的应用。" });
  if (scenario === "update-interrupted") Object.assign(value, { phase: "error", source_dir: source, candidate: { ...target }, message: "上次准备更新已中断，请重新准备。" });
  return value;
}
function wire(s) { const { polls, ...value } = s; return structuredClone(value); }
export function resetUpdater() { states.clear(); updaterCalls.splice(0); }
export function updaterSnapshot(scenario = "") { return wire(states.get(scenario) ?? initial(scenario)); }
export async function answerUpdater(command, args = {}, scenario = "") {
  updaterCalls.push({ command, args });
  const state = states.get(scenario) ?? initial(scenario); states.set(scenario, state);
  if (command === "app_update_status") {
    if (scenario === "update-read-error") throw { code: "io", message: "模拟：更新状态读取失败" };
    if (scenario === "update-malformed") return { phase: "ready", log_tail: {} };
    if (state.phase === "checking" && ++state.polls >= 2) Object.assign(state, scenario === "update-auth-error"
      ? { phase: "error", message: "Git 鉴权失败，请先恢复此源码仓库已有的登录。" }
      : scenario === "update-latest" ? { phase: "up_to_date", candidate: null, message: null }
        : { phase: "available", candidate: { ...target }, message: null });
    if (state.phase === "preparing") {
      const steps = ["获取源码", "安装依赖", "编译应用", "校验应用"];
      state.stage = steps[Math.min(++state.polls, steps.length - 1)];
      state.log_tail.push(`${state.stage} · 合成离线执行`);
      if (state.polls >= 4) Object.assign(state, scenario === "update-build-error"
        ? { phase: "error", message: "编译失败：模拟缺少 Rust / Cargo 工具链。", can_install: false }
        : { phase: "ready", message: "应用已校验，等待安装。", can_install: true });
    }
    return wire(state);
  }
  if (command === "app_update_choose_source") {
    if (scenario !== "update-picker-cancel") Object.assign(state, { source_dir: source, phase: "idle", candidate: null, message: null, can_install: false });
  } else if (command === "app_update_check") {
    Object.assign(state, { phase: "checking", stage: "检查稳定版本", started_at: new Date().toISOString(), polls: 0, message: null, candidate: null, can_install: false });
  } else if (command === "app_update_prepare") {
    Object.assign(state, { phase: "preparing", stage: "获取源码", started_at: new Date().toISOString(), polls: 0, message: null, log_tail: [], can_install: false });
  } else if (command === "app_update_cancel") {
    Object.assign(state, { phase: "cancelled", message: "已停止本次更新，原应用保留。", can_install: false });
  } else if (command === "app_update_install") {
    if (scenario === "update-install-error") throw { code: "backup_failed", message: "升级备份失败，原应用继续使用。" };
    Object.assign(state, { phase: "installing", stage: "等待应用退出", message: "离线模拟不会退出或替换应用。", can_install: false });
  } else throw { code: "not_found", message: `未登记更新命令 ${command}` };
  return wire(state);
}
