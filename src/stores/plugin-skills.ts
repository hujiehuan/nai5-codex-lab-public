import { reactive } from "vue";
import { defineStore } from "pinia";
import { studioApi } from "../api-studio";
import { errMessage } from "../api";
import { validPluginSkill } from "../lib/studio-skills";
import type { StudioPluginSkill, StudioSkillKind } from "../types-studio";

export interface PluginSkillState { value: StudioPluginSkill | null; loading: boolean; error: string | null }
export const usePluginSkills = defineStore("plugin-skills", () => {
  const states = reactive<Record<StudioSkillKind, PluginSkillState>>({
    draw: { value: null, loading: false, error: null }, reverse: { value: null, loading: false, error: null },
  });
  const generations = { draw: 0, reverse: 0 };
  // Never reuse a pending read for a new experiment: each round needs a fresh selection.
  async function refresh(kind: StudioSkillKind): Promise<StudioPluginSkill> {
    const generation = ++generations[kind], state = states[kind];
    state.loading = true; state.error = null; state.value = null;
    try {
      const value = await studioApi.syncSkills(kind);
      if (!validPluginSkill(value, kind)) throw new Error("插件 skill 响应不完整或与当前实验不符，请更新并重载插件后重试。");
      if (generation === generations[kind]) state.value = value;
      return value;
    } catch (e) {
      if (generation === generations[kind]) state.error = errMessage(e);
      throw e;
    } finally { if (generation === generations[kind]) state.loading = false; }
  }
  return { states, refresh };
});
