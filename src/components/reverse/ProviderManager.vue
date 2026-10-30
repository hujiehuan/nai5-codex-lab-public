<script setup lang="ts">
import VButton from "../../ui/VButton.vue";
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { usePluginModels } from "../../stores/plugin-models";
import { useReverse } from "../../stores/reverse";
import {
  PROVIDER_KIND_LABEL,
  isPluginProvider,
  pluginProviderInput,
  pluginProviderRows,
  providerInputFrom,
} from "../../lib/provider";
import type { ReverseProvider } from "../../types";

const emit = defineEmits<{ close: [] }>();
const rev = useReverse();

// ---- 上半栏：插件里的模型（CONTRACT §10.1）----
// 打开对话框时取一次 plugin_providers()；不可达时栏内灰字给原因，不弹 toast。
const pluginModels = usePluginModels();
const pluginIds = computed(() => pluginModels.ready ? pluginModels.ids : null);
const pluginError = computed(() => pluginModels.error);
const pluginLoading = computed(() => pluginModels.loading);
const busyId = ref<string | null>(null); // 正在保存的 provider id，按钮 / 勾选框置灰防连点
const pluginRows = computed(() =>
  pluginProviderRows(pluginIds.value, rev.providers),
);
const localProviders = computed(() =>
  rev.providers.filter((p) => !isPluginProvider(p)),
);

async function loadPluginProviders() { await pluginModels.refresh(); }
function onFocus() { void loadPluginProviders(); }
onBeforeUnmount(() => window.removeEventListener("focus", onFocus));
/** 一键"启用为反推模型"：走现有 saveProvider（不设 key）。 */
async function enablePlugin(id: string) {
  await pluginModels.refresh();
  if (pluginModels.issue([id])) return;
  busyId.value = id;
  await rev.saveProvider(pluginProviderInput(id), "");
  busyId.value = null;
}
/** 已加入的插件模型：启用开关直接改 enabled 整条重存；失败时把勾选框拨回去（列表没刷新，DOM 会停在用户点的状态）。 */
async function togglePluginEnabled(p: ReverseProvider, ev: Event) {
  if (!p.enabled) { await pluginModels.refresh(); if (pluginModels.issue([p.plugin_provider_id ?? ""])) { if (ev.target instanceof HTMLInputElement) ev.target.checked = p.enabled; return; } }
  busyId.value = p.plugin_provider_id ?? String(p.provider_id);
  const saved = await rev.saveProvider(
    providerInputFrom(p, { enabled: !p.enabled }),
    "",
  );
  busyId.value = null;
  if (!saved && ev.target instanceof HTMLInputElement)
    ev.target.checked = p.enabled;
}

// ---- 下半栏：本地 OpenAI 兼容模型（现有表单，kind 恒 openai）----
const form = reactive({
  provider_id: undefined as number | undefined,
  name: "",
  base_url: "",
  model: "",
  max_tokens: 1024,
  enabled: true,
});
const apiKey = ref("");
const editingHasKey = ref(false);
const saving = ref(false);
const modal = ref<HTMLElement | null>(null);
onMounted(() => {
  modal.value?.focus();
  window.addEventListener("focus", onFocus);
  void loadPluginProviders();
});

function reset() {
  form.provider_id = undefined;
  form.name = "";
  form.base_url = "";
  form.model = "";
  form.max_tokens = 1024;
  form.enabled = true;
  apiKey.value = "";
  editingHasKey.value = false;
}
function edit(p: ReverseProvider) {
  form.provider_id = p.provider_id;
  form.name = p.name;
  form.base_url = p.base_url;
  form.model = p.model;
  form.max_tokens = p.max_tokens;
  form.enabled = p.enabled;
  apiKey.value = "";
  editingHasKey.value = p.has_key;
}
async function save() {
  if (!form.name.trim() || !form.base_url.trim() || !form.model.trim()) return;
  saving.value = true;
  const saved = await rev.saveProvider(
    {
      provider_id: form.provider_id,
      name: form.name.trim(),
      base_url: form.base_url.trim().replace(/\/+$/, ""),
      model: form.model.trim(),
      max_tokens: Math.max(1, Math.round(form.max_tokens) || 1024),
      enabled: form.enabled,
      kind: "openai",
      plugin_provider_id: null,
    },
    apiKey.value,
  );
  saving.value = false;
  if (saved) reset();
}
/** 两栏共用：删除本地记录（插件模型删的只是"启用为反推模型"这条记录，AstrBot 里的 provider 不动）。 */
async function remove(p: ReverseProvider) {
  if (!window.confirm(`删除模型 ${p.name}？历史 run 仍保留。`)) return;
  if (
    (await rev.deleteProvider(p.provider_id)) &&
    form.provider_id === p.provider_id
  )
    reset();
}
</script>

<template>
  <div class="modal-wrap" @keydown.esc="emit('close')">
    <div class="backdrop" @click="emit('close')" />
    <div ref="modal" class="modal" tabindex="-1">
      <header>
        <span>模型管理</span
        ><VButton class="close" @click="emit('close')">×</VButton>
      </header>
      <div class="body">
        <section class="sec">
          <div class="sec-head">
            插件里的模型
            <span class="badge plugin">{{ PROVIDER_KIND_LABEL.plugin }}</span>
            <VButton
              class="mini"
              :disabled="pluginLoading"
              @click="loadPluginProviders"
              >{{ pluginLoading ? "读取中…" : "刷新" }}</VButton
            >
          </div>
          <div class="dim small">
            AstrBot 里配置好的 provider，反推时由插件代为调用，不用在这里填
            base_url / key。
          </div>
          <div v-if="pluginError" class="dim small reason">
            插件不可达：{{ pluginError }}
          </div>
          <ul class="plist">
            <li
              v-for="row in pluginRows"
              :key="
                row.provider ? `p${row.provider.provider_id}` : `id:${row.id}`
              "
              :class="{ off: row.provider && !row.provider.enabled }"
            >
              <div class="pname">
                {{ row.provider?.name ?? row.id }}
                <span v-if="row.provider && !row.provider.enabled" class="dim"
                  >（停用）</span
                >
              </div>
              <div class="dim small">
                provider id {{ row.id || "—"
                }}<span v-if="row.missing"> · 已不在插件可选名单</span
                ><span v-if="row.provider">
                  · max_tokens {{ row.provider.max_tokens }}</span
                >
              </div>
              <div class="acts">
                <template v-if="row.provider">
                  <label class="inline"
                    ><input
                      type="checkbox"
                      :checked="row.provider.enabled"
                      :disabled="busyId === row.id || (!row.provider.enabled && (!pluginModels.ready || row.missing))"
                      @change="togglePluginEnabled(row.provider, $event)"
                    />
                    启用</label
                  >
                  <VButton class="mini danger" @click="remove(row.provider)"
                    >删除</VButton
                  >
                </template>
                <VButton
                  v-else
                  class="mini primary"
                  :disabled="busyId === row.id || !pluginModels.ready || row.missing"
                  @click="enablePlugin(row.id)"
                  >启用为反推模型</VButton
                >
              </div>
            </li>
            <li v-if="!pluginRows.length" class="dim empty">
              {{
                pluginLoading
                  ? "读取中…"
                  : pluginError
                    ? "插件不可达，本地也还没有启用过插件模型"
                    : "插件里没有配置 provider"
              }}
            </li>
          </ul>
        </section>

        <section class="sec">
          <div class="sec-head">
            本地 OpenAI 兼容模型
            <span class="badge">{{ PROVIDER_KIND_LABEL.openai }}</span>
          </div>
          <div class="cols">
            <ul class="plist">
              <li
                v-for="p in localProviders"
                :key="p.provider_id"
                :class="{
                  on: form.provider_id === p.provider_id,
                  off: !p.enabled,
                }"
                class="clickable"
                @click="edit(p)"
              >
                <div class="pname">
                  {{ p.name }}
                  <span v-if="!p.enabled" class="dim">（停用）</span>
                </div>
                <div class="dim small">{{ p.model }} · {{ p.base_url }}</div>
                <div class="dim small">
                  key {{ p.has_key ? "已设置" : "未设置" }} · max_tokens
                  {{ p.max_tokens }}
                </div>
                <VButton class="mini danger corner" @click.stop="remove(p)"
                  >删除</VButton
                >
              </li>
              <li v-if="!localProviders.length" class="dim empty">
                还没有本地模型
              </li>
            </ul>
            <form class="pform" @submit.prevent="save">
              <div class="ftitle">
                {{
                  form.provider_id === undefined
                    ? "新建模型"
                    : `编辑 #${form.provider_id}`
                }}
                <VButton
                  v-if="form.provider_id !== undefined"
                  type="button"
                  class="mini"
                  @click="reset"
                  >改为新建</VButton
                >
              </div>
              <label>名称 <input v-model="form.name" required /></label>
              <label
                >base_url
                <input
                  v-model="form.base_url"
                  placeholder="https://api.example.com/v1"
                  required
              /></label>
              <label>model <input v-model="form.model" required /></label>
              <label
                >max_tokens
                <input v-model.number="form.max_tokens" type="number" min="1"
              /></label>
              <label class="inline"
                ><input v-model="form.enabled" type="checkbox" /> 启用</label
              >
              <label>
                API key
                <input
                  v-model="apiKey"
                  type="password"
                  autocomplete="off"
                  :placeholder="
                    editingHasKey ? '已设置，留空则不变' : '留空则不设置'
                  "
                />
              </label>
              <div class="dim small">
                key 只写入钥匙串 `provider:&lt;id&gt;`，不回显。
              </div>
              <div>
                <VButton type="submit" class="primary" :disabled="saving"
                  >保存</VButton
                >
              </div>
            </form>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.modal-wrap {
  position: fixed;
  inset: 0;
  z-index: 60;
}
.backdrop {
  position: absolute;
  inset: 0;
  background: var(--backdrop);
}
.modal {
  position: absolute;
  top: 6vh;
  left: 50%;
  transform: translateX(-50%);
  width: 820px;
  max-width: 94vw;
  max-height: 88vh;
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  display: flex;
  flex-direction: column;
  box-shadow: 0 10px 40px var(--backdrop);
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--line);
  font-weight: 600;
}
.close {
  padding: 0 10px;
  font-size: 18px;
}
.body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 12px 14px;
  overflow: auto;
}
.sec {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sec-head {
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 8px;
}
.reason {
  color: var(--muted);
}
.cols {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}
.plist {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.plist li {
  position: relative;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
}
.plist li.clickable {
  cursor: pointer;
}
.plist li.on {
  border-color: var(--accent);
}
.plist li.off {
  opacity: 0.6;
}
.plist li.empty {
  border: none;
  padding: 4px 0;
}
.plist .corner {
  position: absolute;
  top: 6px;
  right: 6px;
}
.acts {
  position: absolute;
  top: 8px;
  right: 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.acts label.inline {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
}
.pname {
  font-weight: 600;
  padding-right: 150px;
}
.pform {
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 13px;
}
.pform label {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.pform label.inline {
  flex-direction: row;
  align-items: center;
  gap: 6px;
}
.ftitle {
  font-weight: 600;
  display: flex;
  gap: 8px;
  align-items: center;
}
.badge {
  display: inline-block;
  padding: 0 6px;
  font-size: 10px;
  line-height: 16px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--line);
  color: var(--muted);
}
.badge.plugin {
  color: var(--accent);
  border-color: var(--accent);
}
.mini {
  padding: 1px 8px;
  font-size: 11px;
}
.dim {
  color: var(--muted);
}
.small {
  font-size: 11px;
}
</style>
