<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { api, errMessage, imgSrc } from "../../api";
import { creativeApi } from "../../api-creative";
import type { BatchSummary, BatchResults, LabRunSummary, LabRun } from "../../types";
import type { CreativeRun } from "../../types-creative";
import VButton from "../../ui/VButton.vue";
const mode = ref<"reverse" | "creative" | "legacy">("creative"), loading = ref(false), error = ref<string | null>(null);
const list = ref<{ id: string; label: string }[]>([]), selected = ref<string | null>(null);
const details = ref<BatchResults | LabRun | CreativeRun | null>(null), urls = ref<{ idx: number; path: string }[]>([]), imageError = ref<string | null>(null);
let sequence = 0;
async function load() {
  const seq = ++sequence; loading.value = true; error.value = null; selected.value = null; details.value = null; urls.value = [];
  try {
    const values = mode.value === "reverse" ? await api.reverseBatchesList() : mode.value === "creative" ? await creativeApi.list(100) : await api.labRunList(100);
    if (!Array.isArray(values)) throw new Error("历史列表结构错误");
    if (seq !== sequence) return;
    list.value = values.map((value) => {
      if ("batch_id" in value) { const v = value as BatchSummary; return { id: v.batch_id, label: `${v.created_at} · ${v.template} · ${v.status}` }; }
      const v = value as LabRunSummary | CreativeRun; return { id: v.run_id, label: `${v.created_at} · ${v.status} · ${v.run_id.slice(0, 10)}` };
    });
  } catch (e) { if (seq === sequence) error.value = errMessage(e); } finally { if (seq === sequence) loading.value = false; }
}
async function view(id: string) {
  const seq = ++sequence; selected.value = id; loading.value = true; error.value = null; imageError.value = null; urls.value = []; details.value = null;
  try {
    const value = mode.value === "reverse" ? await api.reverseBatchResults(id) : mode.value === "creative" ? await creativeApi.status(id) : await api.labRunStatus(id);
    if (seq !== sequence) return; details.value = value;
    if ("images" in value && Array.isArray(value.images)) {
      for (const image of value.images.filter((i) => i.status === "done")) {
        try {
          let path = mode.value === "creative" ? await creativeApi.image(id, image.idx, true) : await api.labRunImage(id, image.idx, true);
          if (!path.startsWith("/") && !path.startsWith("data:")) path = `${(await api.appInfo()).app_data_dir}/${path}`;
          if (seq === sequence) urls.value.push({ idx: image.idx, path });
        } catch (e) { if (seq === sequence) imageError.value = errMessage(e); }
      }
    }
  } catch (e) { if (seq === sequence) error.value = errMessage(e); } finally { if (seq === sequence) loading.value = false; }
}
watch(mode, () => void load()); onMounted(() => void load());
</script>
<template>
  <div class="legacy-history"><div class="inline"><label>旧记录类型<select v-model="mode"><option value="creative">旧画图创作记录</option><option value="reverse">旧反推批次</option><option value="legacy">旧画图实验室记录</option></select></label><VButton :disabled="loading" @click="load">刷新旧记录</VButton></div><p class="hint">只读查看，保留原实验参数和评分口径。新实验请使用上面的工作区。</p><p v-if="loading" role="status">正在读取记录…</p><p v-if="error" class="error" role="alert">{{ error }}</p><p v-else-if="!loading && !list.length" class="hint">这类旧记录为空。</p><div class="legacy-list"><VButton v-for="row in list" :key="row.id" :class="{ primary: selected === row.id }" @click="view(row.id)">{{ row.label }}</VButton></div><p v-if="imageError" class="error">图片读取失败：{{ imageError }} <VButton v-if="selected" @click="view(selected)">重试读取</VButton></p><div class="legacy-images"><img v-for="image in urls" :key="image.idx" :src="imgSrc(image.path) ?? undefined" :alt="`旧记录结果 ${image.idx + 1}`" /></div><details v-if="details" open><summary>原始实验记录</summary><pre>{{ JSON.stringify(details, null, 2) }}</pre></details></div>
</template>
<style scoped>
.inline{display:flex;gap:12px;flex-wrap:wrap;align-items:end}label{display:flex;flex-direction:column;gap:6px;min-width:0}.hint{font-size:12px;color:var(--muted);margin:10px 0}.error{color:var(--danger);overflow-wrap:anywhere}.legacy-list{display:flex;flex-direction:column;gap:6px;max-height:240px;overflow:auto;margin:12px 0}.legacy-list :deep(.n-button__content){white-space:normal;overflow-wrap:anywhere}.legacy-list :deep(.n-button){height:auto;min-height:34px;justify-content:start;padding:8px}.legacy-images{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:10px}.legacy-images img{width:100%;height:220px;object-fit:contain;background:var(--field)}pre{background:var(--field);padding:12px;white-space:pre-wrap;overflow:auto;max-height:450px;font-size:12px;overflow-wrap:anywhere}summary{padding:10px 0;cursor:pointer}
</style>
