<script setup lang="ts">
import { computed, ref } from "vue";
import { NSelect } from "naive-ui";
import { studioApi } from "../../api-studio";
import { errMessage } from "../../api";
import { bytesLabel, validSkill } from "../../lib/studio";
import { PLUGIN_SKILL_ID } from "../../lib/studio-skills";
import type { PluginSkillState } from "../../stores/plugin-skills";
import type { StudioSkill, StudioSkillGroup, StudioSkillKind } from "../../types-studio";
import VButton from "../../ui/VButton.vue";

const props = defineProps<{ skills: StudioSkill[]; groups: StudioSkillGroup[]; pluginState: PluginSkillState; kind: StudioSkillKind; disabled?: boolean }>();
const emit = defineEmits<{ "update:groups": [StudioSkillGroup[]]; refresh: []; sync: [] }>();
const preview = ref<StudioSkill | null>(null), busy = ref(false), error = ref<string | null>(null);
const options = computed(() => [{ label: "跟随插件所选 skill（自动同步）", value: PLUGIN_SKILL_ID },
  ...props.skills.map((s) => ({ label: `${s.name} · ${s.version.slice(0, 10)} · 固定版`, value: s.id }))]);
function skillName(id: string) {
  if (id !== PLUGIN_SKILL_ID) return props.skills.find((s) => s.id === id)?.name ?? `缺失资料：${id}`;
  const state = props.pluginState;
  return state.loading ? "跟随插件 · 同步中…" : state.error ? "跟随插件 · 同步失败" : !state.value ? "跟随插件 · 尚未同步" :
    state.value.skill ? `跟随插件 · ${state.value.skill.name} · ${state.value.skill.version.slice(0, 12)}` : "跟随插件 · 不附加反推 skill";
}
const importPath = ref("");
function addGroup() { emit("update:groups", [...props.groups, { id: crypto.randomUUID(), name: `组合 ${props.groups.length + 1}`, skill_ids: [] }]); }
function updateGroup(id: string, field: "name" | "skill_ids", value: string | string[]) {
  emit("update:groups", props.groups.map((g) => g.id === id ? { ...g, [field]: value } : g));
}
function removeGroup(id: string) { emit("update:groups", props.groups.filter((g) => g.id !== id)); }
function moveSkill(group: StudioSkillGroup, index: number, dir: number) {
  const ids = [...group.skill_ids]; [ids[index], ids[index + dir]] = [ids[index + dir], ids[index]];
  updateGroup(group.id, "skill_ids", ids);
}
async function importSkill(remote = false) {
  busy.value = true; error.value = null;
  try {
    const value = remote ? await studioApi.fetchSkill("npe") : await studioApi.importSkill({ path: importPath.value.trim() });
    if (!validSkill(value)) throw new Error("Skill 返回结构不完整");
    preview.value = value; emit("refresh");
  } catch (e) { error.value = errMessage(e); } finally { busy.value = false; }
}
async function chooseFile() {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ title: "选择 Markdown / SKILL.md", multiple: false, filters: [{ name: "Markdown", extensions: ["md"] }] });
    if (typeof path === "string") importPath.value = path;
  } catch (e) { error.value = errMessage(e); }
}
async function removeSkill(skill: StudioSkill) {
  busy.value = true; error.value = null;
  try { await studioApi.deleteSkill(skill.id); emit("refresh"); preview.value = null; }
  catch (e) { error.value = errMessage(e); } finally { busy.value = false; }
}
</script>

<template>
  <div class="skill-editor">
    <div class="plugin-skill" data-testid="studio-plugin-skill" :aria-busy="pluginState.loading">
      <div class="inline"><strong>插件当前 skill · {{ kind === 'reverse' ? '反推' : '/画' }}</strong><VButton :disabled="disabled || pluginState.loading" @click="emit('sync')">同步插件 skill</VButton></div>
      <p v-if="pluginState.loading" role="status">正在同步插件所选资料…</p>
      <template v-else-if="pluginState.error"><p class="error" role="alert">同步失败：{{ pluginState.error }}</p><p class="hint">跟随组合暂不可新建，未使用旧缓存。固定资料对照仍可使用。</p></template>
      <template v-else-if="pluginState.value?.skill"><strong>{{ pluginState.value.skill.name }}</strong><p class="hint mono">版本 {{ pluginState.value.skill.version }} · SHA256 {{ pluginState.value.skill.sha256.slice(0, 12) }} · {{ bytesLabel(pluginState.value.skill.bytes) }}</p><VButton @click="preview = pluginState.value!.skill">查看已同步全文</VButton></template>
      <p v-else-if="pluginState.value">插件未启用反推 skill；跟随此选择时不附加资料。</p>
      <p v-else>尚未读取插件选择。</p>
      <p class="hint">选择“跟随插件”后，新实验自动使用插件选中的版本；已建实验保留原快照。仅下载、未启用的版本不会生效。</p>
    </div>
    <p class="hint">每个组合按顺序提供资料；空组合是“无 skill”对照。每个选中的模型都会运行所有组合。</p>
    <div v-for="(group, index) in groups" :key="group.id" class="skill-group">
      <div class="inline"><input :value="group.name" :aria-label="`组合 ${index + 1} 名称`" :disabled="disabled" @input="updateGroup(group.id, 'name', ($event.target as HTMLInputElement).value)" /><VButton :disabled="disabled || groups.length < 2" @click="removeGroup(group.id)">删除组合</VButton></div>
      <NSelect :value="group.skill_ids" multiple :disabled="disabled" :options="options" placeholder="无 skill 对照" :aria-label="`${group.name} 的 skill`" @update:value="updateGroup(group.id, 'skill_ids', $event)" />
      <ol v-if="group.skill_ids.length" class="skill-order"><li v-for="(id, at) in group.skill_ids" :key="id"><span>{{ skillName(id) }}</span><VButton :disabled="disabled || at === 0" :aria-label="`上移 ${id}`" @click="moveSkill(group, at, -1)">↑</VButton><VButton :disabled="disabled || at === group.skill_ids.length - 1" :aria-label="`下移 ${id}`" @click="moveSkill(group, at, 1)">↓</VButton></li></ol>
    </div>
    <VButton :disabled="disabled || groups.length >= 8" @click="addGroup">＋ 添加 skill 组合</VButton>
    <details class="skill-library"><summary>资料库 · {{ skills.length }} 份</summary>
      <div v-for="skill in skills" :key="skill.id" class="skill-item"><span>{{ skill.name }}<small>{{ bytesLabel(skill.bytes) }} · {{ skill.builtin ? '内置固定版' : '本地资料' }}</small></span><VButton @click="preview = skill">查看全文</VButton></div>
      <div class="inline"><input v-model="importPath" aria-label="Skill 文件或目录路径" placeholder="Markdown 文件或 SKILL 目录" /><VButton :disabled="busy" @click="chooseFile">选择文件</VButton></div>
      <div class="inline"><VButton :disabled="busy || !importPath.trim()" @click="importSkill()">导入并预览</VButton><VButton :disabled="busy" @click="importSkill(true)">导入 NPE 固定版</VButton></div>
      <p class="hint">NPE 固定 revision 58f98ac；仅下载为实验参考资料，不执行其中指令。导入后查看全文，再加入组合。</p>
    </details>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <div v-if="preview" class="skill-preview" role="region" aria-label="Skill 内容预览"><div class="inline"><strong>{{ preview.name }}</strong><VButton @click="preview = null">关闭预览</VButton><VButton v-if="!preview.builtin && !preview.id.startsWith('plugin-skill-')" :disabled="busy" @click="removeSkill(preview)">删除本地资料</VButton></div><p class="hint">版本 {{ preview.version }} · {{ preview.source }} · {{ bytesLabel(preview.bytes) }}<br />SHA256 {{ preview.sha256 }}</p><pre>{{ preview.text }}</pre></div>
  </div>
</template>

<style scoped>
.plugin-skill{display:flex;flex-direction:column;align-items:flex-start;gap:8px;min-width:0;padding:12px;border:1px solid var(--line);border-radius:var(--radius);background:var(--panel)}.plugin-skill p{margin:0;overflow-wrap:anywhere;max-width:100%}.plugin-skill strong{overflow-wrap:anywhere}.plugin-skill .mono{font-variant-numeric:tabular-nums}
.skill-editor,.skill-group,.skill-library,.skill-preview{display:flex;flex-direction:column;gap:10px;min-width:0}.skill-group,.skill-preview{padding:12px;background:var(--field);border:1px solid var(--line);border-radius:var(--radius)}.inline,.skill-item{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.inline input{flex:1;min-width:120px}.skill-item{padding:10px 0;border-bottom:1px solid var(--line);justify-content:space-between}.skill-item span{overflow-wrap:anywhere}.skill-item small{display:block;color:var(--muted)}.hint{color:var(--muted);font-size:12px;overflow-wrap:anywhere}.error{color:var(--danger);overflow-wrap:anywhere}.skill-order{margin:0;padding-left:20px}.skill-order li{display:flex;gap:6px;align-items:center;margin:5px 0}.skill-order span{flex:1;overflow-wrap:anywhere}.skill-library summary{cursor:pointer;padding:8px 0}.skill-preview pre{white-space:pre-wrap;overflow:auto;max-height:340px;margin:0;font-size:12px;overflow-wrap:anywhere}.skill-preview{margin-top:6px}
</style>
