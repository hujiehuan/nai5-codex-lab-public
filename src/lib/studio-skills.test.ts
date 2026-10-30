import { describe, expect, it } from "vitest";
import { PLUGIN_SKILL_ID, freezeSkillGroups, migrateSkillGroups, skillKind, validPluginSkill } from "./studio-skills";
import type { StudioPluginSkill, StudioSkillGroup } from "../types-studio";

const selection: StudioPluginSkill = { version: 1, kind: "draw", enabled: true, selected_id: "upstream@new",
  skill: { id: `plugin-skill-${"b".repeat(64)}`, name: "构图", version: "new", sha256: "a".repeat(64), text: "构图正文",
    source: "https://example.invalid/skill", builtin: false, bytes: 12 } };
const group: StudioSkillGroup = { id: "default", name: "默认资料", skill_ids: [PLUGIN_SKILL_ID] };

describe("plugin-selected skill snapshots", () => {
  it("maps drawing modes to the plugin drawing selection", () => {
    expect([skillKind("draw"), skillKind("reverse")]).toEqual(["draw", "reverse"]);
  });
  it("accepts a complete immutable snapshot and explicit reverse disable", () => {
    expect(validPluginSkill(selection, "draw")).toBe(true);
    expect(validPluginSkill({ version: 1, kind: "reverse", enabled: false, selected_id: "", skill: null }, "reverse")).toBe(true);
  });
  it.each([
    {}, { ...selection, kind: "reverse" }, { ...selection, version: 2 }, { ...selection, enabled: false },
    { ...selection, skill: null }, { ...selection, skill: { ...selection.skill, bytes: 4 } },
    { ...selection, skill: { ...selection.skill, id: "nai5-prompting" } },
    { ...selection, skill: { ...selection.skill, sha256: "unverified" } },
  ])("rejects incomplete, cross-mode or unfrozen result %j", (value) => {
    expect(validPluginSkill(value, "draw")).toBe(false);
    expect(() => freezeSkillGroups([group], value as StudioPluginSkill, "draw")).toThrow("未提交构思");
  });
  it("freezes the same version in every combination without mutating the follow choice", () => {
    const groups = [group, { ...group, id: "second", skill_ids: ["fixed", PLUGIN_SKILL_ID] }];
    const frozen = freezeSkillGroups(groups, selection, "draw");
    const updated = { ...selection, skill: { ...selection.skill!, id: `plugin-skill-${"c".repeat(64)}`, version: "later" } };
    expect(freezeSkillGroups(groups, updated, "draw")[0].skill_ids).toEqual([updated.skill.id]);
    expect(frozen.map((g) => g.skill_ids)).toEqual([[selection.skill!.id], ["fixed", selection.skill!.id]]);
    expect(groups[0].skill_ids).toEqual([PLUGIN_SKILL_ID]);
  });
  it("reverse disable means zero bundles, while fixed controls work without the new endpoint", () => {
    expect(freezeSkillGroups([group], { version: 1, kind: "reverse", enabled: false, selected_id: "", skill: null }, "reverse")[0].skill_ids).toEqual([]);
    expect(freezeSkillGroups([{ ...group, skill_ids: ["fixed"] }, { ...group, id: "empty", skill_ids: [] }], null, "draw").map((g) => g.skill_ids)).toEqual([["fixed"], []]);
    expect(freezeSkillGroups([{ ...group, skill_ids: [PLUGIN_SKILL_ID, PLUGIN_SKILL_ID] }], selection, "draw")[0].skill_ids).toEqual([selection.skill!.id]);
  });
  it("migrates only the untouched old default, once, preserving custom and no-skill controls", () => {
    const old = { ...group, skill_ids: ["nai5-prompting"] };
    const custom = [{ ...old, name: "固定对照" }, { ...old, id: "mine" }, { ...group, skill_ids: [] }, { ...old, skill_ids: ["nai5-prompting", "another"] }];
    expect(migrateSkillGroups([old, ...custom], undefined)).toEqual([group, ...custom]);
    expect(old.skill_ids).toEqual(["nai5-prompting"]);
    expect(migrateSkillGroups([old], 1)).toEqual([old]);
  });
});
