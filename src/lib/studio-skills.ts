import { validSkill } from "./studio";
import type { StudioMode, StudioPluginSkill, StudioSkillGroup, StudioSkillKind } from "../types-studio";

export const PLUGIN_SKILL_ID = "plugin-active";
export function skillKind(mode: StudioMode): StudioSkillKind { return mode === "reverse" ? "reverse" : "draw"; }
export function followsPlugin(groups: StudioSkillGroup[]): boolean { return groups.some((g) => g.skill_ids.includes(PLUGIN_SKILL_ID)); }

export function validPluginSkill(value: unknown, kind: StudioSkillKind): value is StudioPluginSkill {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const x = value as Record<string, unknown>;
  if (x.version !== 1 || x.kind !== kind || typeof x.enabled !== "boolean" || typeof x.selected_id !== "string") return false;
  if (!x.enabled) return kind === "reverse" && x.selected_id === "" && x.skill === null;
  return !!x.selected_id.trim() && validSkill(x.skill) && x.skill.id.startsWith("plugin-skill-") &&
    !!x.skill.version.trim() && !!x.skill.text.trim() && /^[a-f0-9]{64}$/.test(x.skill.sha256) &&
    x.skill.bytes === new TextEncoder().encode(x.skill.text).byteLength;
}

// This only migrates the untouched legacy default; an explicitly empty group is a control.
export function migrateSkillGroups(groups: StudioSkillGroup[], version: unknown): StudioSkillGroup[] {
  return groups.map((g) => ({ ...g, skill_ids: version !== 1 && g.id === "default" && g.name === "默认资料" &&
    g.skill_ids.length === 1 && g.skill_ids[0] === "nai5-prompting" ? [PLUGIN_SKILL_ID] : [...g.skill_ids] }));
}

export function freezeSkillGroups(groups: StudioSkillGroup[], selection: StudioPluginSkill | null, kind: StudioSkillKind): StudioSkillGroup[] {
  if (followsPlugin(groups) && !validPluginSkill(selection, kind)) throw new Error("插件 skill 尚未同步，本轮未提交构思。请同步后重试。");
  return groups.map((g) => ({ ...g, skill_ids: [...new Set(g.skill_ids.flatMap((id) => id === PLUGIN_SKILL_ID ?
    (selection?.skill ? [selection.skill.id] : []) : [id]))] }));
}
