<script setup lang="ts">
import VButton from "../../ui/VButton.vue";
import { computed, onMounted, reactive, ref } from "vue";
import { useReverse } from "../../stores/reverse";
import { directProviders } from "../../lib/provider";
import type { ReverseProvider } from "../../types";

const emit = defineEmits<{ close: [] }>();
const rev = useReverse();

// 评测界面只管理 OpenAI 兼容直连端点（kind 恒 openai）；后端仍认的 kind=plugin 旧记录不在这里列出。
const localProviders = computed(() => directProviders(rev.providers));
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
onMounted(() => modal.value?.focus());

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
/** 删除本地模型记录；历史 run 仍保留。 */
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
          <div class="sec-head">OpenAI 兼容端点</div>
          <div class="dim small">
            每个视觉模型一条记录：填 base_url、model 与 API key，反推时直接调用该端点的
            /chat/completions。
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
                还没有模型
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
.pname {
  font-weight: 600;
  padding-right: 60px;
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
