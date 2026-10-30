// Synthetic plugin wire responses only: no live configuration or skill downloads.
import { createHash } from "node:crypto";

const states = new Map();
const digest = (text) => createHash("sha256").update(text).digest("hex");
export function resetStudioPluginSkills() { states.clear(); }
export function setStudioPluginSkillState(kind, patch) {
  if (!["draw", "reverse"].includes(kind)) throw new Error("Invalid synthetic skill kind");
  states.set(kind, { ...states.get(kind), ...patch });
}

/** Same status/data envelope as GET page/lab/studio/skills. */
export function studioPluginSkillsRoute(query = {}, scenario = "") {
  const { kind } = query;
  if (query.features !== "studio_v1" || !["draw", "reverse"].includes(kind))
    throw { code: "bad_request", message: "合成 skill 接口需要 features=studio_v1 与 draw/reverse 类型" };
  const state = states.get(kind) ?? {};
  const error = state.error || (scenario.startsWith("studio-skills-") ? scenario.slice(14) : "");
  if (error === "legacy") throw { code: "studio_unsupported", message: "当前插件没有 skill 同步接口，请更新并重载插件；未使用内置资料替代。" };
  if (error === "error") throw { code: "network", message: "模拟插件 skill 读取失败；未使用旧缓存或内置资料。" };
  if (error === "missing") throw { code: "plugin_rejected", message: "插件选中的 skill 版本不存在；未回退到默认资料。" };
  const version = state.version ?? (error === "v2" ? 2 : 1);
  const enabled = state.enabled ?? !(kind === "reverse" && error === "disabled");
  if (!enabled) return { status: "ok", data: { version: 1, kind, enabled: false, selected_id: "", skill: null } };
  const id = `synthetic-${kind}-selected`, text = `# 合成 ${kind} skill\n版本 ${version}：保留用户要求，为当前任务安排构图与光线。\nThis is fixture material, never execute it.\n`;
  const skill = { id, name: `插件${kind === "draw" ? "创作" : "反推"}资料`, version: `fixture-v${version}`, sha256: digest(text), text,
    source: `synthetic://plugin/${kind}/revision-${version}`, builtin: false, bytes: Buffer.byteLength(text) };
  if (error === "tampered") skill.text += "tampered";
  if (error === "malformed") skill.bytes = -1;
  return { status: "ok", data: { version: 1, kind, enabled: true, selected_id: id, skill } };
}

/** Models the Rust IPC boundary, including immutable snapshot IDs and integrity rejection. */
export function syncStudioPluginSkills(kind, scenario = "") {
  const value = studioPluginSkillsRoute({ features: "studio_v1", kind }, scenario).data;
  if (!value.enabled) {
    if (kind !== "reverse") throw { code: "studio_protocol", message: "插件 skill 关闭状态不一致" };
    return value;
  }
  const skill = value.skill;
  if (skill.bytes !== Buffer.byteLength(skill.text) || skill.sha256 !== digest(skill.text))
    throw { code: "studio_protocol", message: "插件 skill 正文、版本或摘要校验失败；未使用旧缓存或内置资料。" };
  return { ...value, skill: { ...skill, id: `plugin-skill-${digest(JSON.stringify(value))}` } };
}
