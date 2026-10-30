<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { NSelect } from "naive-ui";
import { api, errMessage, imgSrc } from "../../api";
import { studioApi } from "../../api-studio";
import { usePluginModels } from "../../stores/plugin-models";
import { pairMetrics, percent } from "../../lib/studio";
import type { StudioCompareResult, StyleCompareResult } from "../../types-studio";
import type { StyleModelStatus } from "../../types";
import VCard from "../../ui/VCard.vue";
import VButton from "../../ui/VButton.vue";
import VCost from "../../ui/VCost.vue";

const pluginModels = usePluginModels();
const props = defineProps<{ providers: string[]; initial?: unknown }>();
const emit = defineEmits<{ state: [unknown] }>();
type Image = { id: string; path: string; group: string };
type Visual = { provider_id: string; result: StudioCompareResult | null; error: string | null; loading: boolean; note?: string | null };
type ReviewRecord = { id: string; created_at: string; images: Image[]; providers: string[]; models: string[]; prompt: string; thresholds: Record<string, number>; local: StyleCompareResult | null; local_error: string | null; visual: Visual[] };
const LEGACY_VISUAL_PROMPT = '只比较画风，忽略人物身份、内容与构图。观察线条、上色、明暗、色调、笔触、材质与媒介感。逐对返回 JSON {"pairs":[{"left_id":"图片ID","right_id":"图片ID","same_style":true,"reason":"理由"}]}；无法判断时 same_style 为 null。';
const DEFAULT_VISUAL_PROMPT = "重点比较线条、上色、光影、色调、笔触、材质和媒介感。请为前三名分别写出具体依据；相似度分数只用于本批候选的相对排序。";
const images = ref<Image[]>([]), providers = ref<string[]>([]), models = ref<string[]>([]);
const modelStatus = ref<StyleModelStatus[] | null>(null), modelError = ref<string | null>(null);
const prompt = ref(DEFAULT_VISUAL_PROMPT);
const thresholds = reactive<Record<string, number>>({ "clip-vit-base-patch32": .8, "csd-vit-large-patch14": .8, "megastyle-siglip-so400m": .8 });
const local = ref<StyleCompareResult | null>(null), localError = ref<string | null>(null), visual = ref<Visual[]>([]);
const busy = ref(false), stopRequested = ref(false), actionError = ref<string | null>(null), picking = ref(false);
const frozenImages = ref<Image[]>([]);
const localRunning = ref(false);
const history = ref<ReviewRecord[]>([]), selectedHistory = ref<string | null>(null);
const frozenPrompt = ref("");
const providerIssue = computed(() => providers.value.length ? pluginModels.issue(providers.value) : null);
const unlabelled = computed(() => images.value.filter((i) => !i.group.trim()).length);
function state() { return { images: images.value, providers: providers.value, models: models.value, prompt: prompt.value, thresholds: { ...thresholds }, history: history.value,
  current: { images: frozenImages.value, prompt: frozenPrompt.value, local: local.value, local_error: localError.value, visual: visual.value }, selected_history: selectedHistory.value }; }
watch([images, providers, models, prompt, thresholds, local, localError, visual, history, selectedHistory], () => emit("state", state()), { deep: true });
watch(() => props.initial, (value) => {
  if (!value || typeof value !== "object" || busy.value) return;
  const v = value as Record<string, unknown>;
  if (Array.isArray(v.images)) images.value = v.images.filter((x) => x && typeof x.id === "string" && typeof x.path === "string" && typeof x.group === "string");
  if (Array.isArray(v.providers)) providers.value = v.providers.filter((x) => typeof x === "string");
  if (Array.isArray(v.models)) models.value = v.models.filter((x) => typeof x === "string");
  if (typeof v.prompt === "string") prompt.value = v.prompt === LEGACY_VISUAL_PROMPT ? DEFAULT_VISUAL_PROMPT : v.prompt;
  if (v.thresholds && typeof v.thresholds === "object") for (const [key, n] of Object.entries(v.thresholds)) if (typeof n === "number" && Number.isFinite(n) && n >= -1 && n <= 1) thresholds[key] = n;
  if (Array.isArray(v.history)) history.value = v.history.filter((x) => x && typeof x.id === "string" && Array.isArray(x.images) && Array.isArray(x.visual)).slice(0, 10);
  if (typeof v.selected_history === "string") selectedHistory.value = v.selected_history;
  if (v.current && typeof v.current === "object") {
    const current = v.current as Partial<ReviewRecord>;
    if (Array.isArray(current.images)) frozenImages.value = current.images;
    if (typeof current.prompt === "string") frozenPrompt.value = current.prompt;
    if (current.local && Array.isArray(current.local.results)) local.value = current.local;
    if (typeof current.local_error === "string") localError.value = current.local_error;
    if (Array.isArray(current.visual)) visual.value = current.visual.map((r) => ({ ...r, loading: false, error: r.loading ? "上次请求未确认完成，未自动重试。" : r.error }));
  }
}, { immediate: true });
function viewHistory(id: string | null) {
  const value = history.value.find((r) => r.id === id); if (!value || busy.value) return;
  selectedHistory.value = id;
  images.value = value.images.map((i) => ({ ...i })); frozenImages.value = value.images.map((i) => ({ ...i }));
  providers.value = [...value.providers]; models.value = [...value.models]; prompt.value = value.prompt; frozenPrompt.value = value.prompt;
  Object.assign(thresholds, value.thresholds); local.value = value.local; localError.value = value.local_error; visual.value = value.visual.map((v) => ({ ...v }));
}
async function loadModels() {
  modelError.value = null;
  try { const v = await api.styleModelsList(); if (!Array.isArray(v)) throw new Error("模型列表结构错误"); modelStatus.value = v; }
  catch (e) { modelError.value = errMessage(e); }
}
async function addImage() {
  picking.value = true; actionError.value = null;
  try {
    const path = await api.reversePickLocalImage(); if (!path) return;
    const copied = await api.reverseImportLocalImage(path);
    const localPath = copied.startsWith("local:") ? copied.slice(6) : copied;
    if (images.value.some((i) => i.path === localPath)) throw new Error("这张图片已经加入");
    images.value.push({ id: crypto.randomUUID(), path: localPath, group: "" });
  } catch (e) { actionError.value = errMessage(e); } finally { picking.value = false; }
}
function imageName(id: string) { const at = frozenImages.value.findIndex((i) => i.id === id); return at < 0 ? id : `图 ${at + 1}`; }
function imagePath(id: string) { return frozenImages.value.find((i) => i.id === id)?.path; }
function comparisonImageSrc(id: string) { const path = imagePath(id); return path ? imgSrc(path) ?? undefined : undefined; }
function labels() { return frozenImages.value.map((i) => ({ ...i, group: images.value.find((v) => v.id === i.id)?.group ?? i.group })); }
function validThreshold(model: string) { const value = thresholds[model] ?? .8; return Number.isFinite(value) && value >= -1 && value <= 1; }
function localMetrics(row: StyleCompareResult["results"][number]) { return pairMetrics(labels(), validThreshold(row.model) ? row.pairs.map((p) => ({ ...p, same: p.cosine >= (thresholds[row.model] ?? .8) })) : []); }
function readVisual(v: StudioCompareResult, row: Visual) {
  if (!v || typeof v.text !== "string" || !Number.isFinite(v.latency_ms)) throw new Error("视觉比较响应结构错误");
  const ordered = frozenImages.value.map((x) => x.id), reference = ordered[0], ids = new Set(ordered.slice(1));
  if (Array.isArray(v.ranking)) {
    const expected = Math.min(3, ids.size), seen = new Set<string>();
    const ranking = v.ranking.filter((item, index) => {
      if (!item || item.rank !== index + 1 || !ids.has(item.image_id) || seen.has(item.image_id)
        || typeof item.similarity !== "number" || !Number.isFinite(item.similarity) || item.similarity < 0 || item.similarity > 100
        || typeof item.reason !== "string") return false;
      seen.add(item.image_id); return true;
    });
    row.note = v.reference_id !== reference || ranking.length !== v.ranking.length || (ranking.length !== 0 && ranking.length !== expected)
      ? "模型没有返回完整、有效的前三名；完整模型原文仍保留。" : ranking.length ? null : "模型结果未能解析为排名；完整模型原文仍保留。";
    row.result = { ...v, reference_id: reference, ranking };
    return;
  }
  const pairs = Array.isArray(v.pairs) ? v.pairs : [];
  row.note = "这是旧版插件返回的两两判断。更新插件后重试此模型，即可得到以图 1 为基准的前三名。";
  row.result = { ...v, pairs };
}
async function start() {
  actionError.value = null;
  if (images.value.length < 2 || images.value.length > 32 || (!models.value.length && !providers.value.length)) { actionError.value = "请加入 2–32 张图片，并选择至少一个比较模型。"; return; }
  if (providers.value.length && images.value.length > 8) { actionError.value = "视觉模型一次支持 2–8 张图片；超过 8 张时请只选本地编码器，或减少样本。"; return; }
  busy.value = true; stopRequested.value = false;
  if (providers.value.length) { await pluginModels.refresh(); const issue = pluginModels.issue(providers.value); if (issue || stopRequested.value) { actionError.value = issue; busy.value = false; return; } }
  selectedHistory.value = null; frozenPrompt.value = prompt.value;
  frozenImages.value = structuredClone(images.value.map((i) => ({ ...i })));
  const input = frozenImages.value.map(({ id, path }) => ({ id, path }));
  const chosenModels = [...models.value], chosenPrompt = prompt.value;
  visual.value = providers.value.map((provider_id) => ({ provider_id, result: null, error: null, loading: false }));
  local.value = null; localError.value = null;
  try {
    if (chosenModels.length) {
      localRunning.value = true;
      try {
        const value = await studioApi.compareLocal({ images: input, models: chosenModels });
        if (!value || value.centered !== false || !Array.isArray(value.results) || !Array.isArray(value.images)) throw new Error("本地比较返回结构错误");
        if (value.results.some((r) => !Array.isArray(r.pairs) || r.pairs.some((p) => !Number.isFinite(p.cosine)))) throw new Error("本地比较图对结构错误");
        local.value = value;
      } catch (e) { localError.value = errMessage(e); } finally { localRunning.value = false; }
    }
    for (const row of visual.value) {
      if (stopRequested.value) break;
      row.loading = true;
      try { await pluginModels.refresh(); if (stopRequested.value) break; const issue = pluginModels.issue([row.provider_id]); if (issue) throw new Error(issue); const v = await studioApi.compare({ provider_id: row.provider_id, prompt: chosenPrompt, images: input }); readVisual(v, row); }
      catch (e) { row.error = errMessage(e); } finally { row.loading = false; }
    }
  } finally {
    history.value = [{ id: crypto.randomUUID(), created_at: new Date().toISOString(), images: frozenImages.value.map((i) => ({ ...i })), providers: visual.value.map((v) => v.provider_id), models: chosenModels, prompt: chosenPrompt, thresholds: { ...thresholds }, local: local.value, local_error: localError.value, visual: visual.value.map((v) => ({ ...v })) }, ...history.value].slice(0, 10);
    busy.value = false;
  }
}
async function retryVisual(row: Visual) {
  if (busy.value || !frozenImages.value.length) return;
  busy.value = true; stopRequested.value = false; row.loading = true; row.error = null;
  try { await pluginModels.refresh(); if (stopRequested.value) return; const issue = pluginModels.issue([row.provider_id]); if (issue) throw new Error(issue); const value = await studioApi.compare({ provider_id: row.provider_id, prompt: frozenPrompt.value, images: frozenImages.value.map(({ id, path }) => ({ id, path })) }); readVisual(value, row); }
  catch (e) { row.error = errMessage(e); } finally { row.loading = false; busy.value = false; }
}
async function stop() { stopRequested.value = true; if (localRunning.value) try { await studioApi.cancelLocalCompare(); } catch (e) { actionError.value = errMessage(e); } }
onMounted(() => void loadModels());
onBeforeUnmount(() => { stopRequested.value = true; });
</script>

<template>
  <div class="review-workspace">
    <div class="review-config">
      <VCard title="比较样本" subtitle="图 1 是视觉模型的基准图；其余图片是排名候选">
        <div class="inline"><VButton :disabled="busy || picking || images.length >= 32" @click="addImage">＋ 添加本地图片</VButton><span>{{ images.length }} / 32 张</span></div>
        <p v-if="!images.length" class="empty">至少加入两张图，可以混合相同与不同画风。</p>
        <div class="sample-grid"><article v-for="(image, index) in images" :key="image.id" class="sample"><img :src="imgSrc(image.path) ?? undefined" :alt="`比较原图 ${index + 1}`" /><strong>图 {{ index + 1 }} · {{ index === 0 ? '基准图' : '候选图' }}</strong><label>人工同风格组<input v-model="image.group" :disabled="busy" :aria-label="`图 ${index + 1} 人工同风格组`" placeholder="如 A / B，留空未标注" /></label><VButton :disabled="busy" @click="images = images.filter((i) => i.id !== image.id)">移除</VButton></article></div>
        <p class="hint">视觉模型会一次查看全部图片，并从候选图中找出最接近图 1 的前三名。人工组标签只供本地编码器计算准确率；当前有 {{ unlabelled }} 张未标注。</p>
      </VCard>
      <VCard title="模型与判定">
        <label>视觉模型，可多选<NSelect v-model:value="providers" multiple filterable :disabled="busy || !pluginModels.ready" :options="props.providers.map((p) => ({ value: p, label: p }))" placeholder="选择视觉模型" aria-label="比较视觉模型" /></label>
        <p v-if="providerIssue" class="error" role="alert">{{ providerIssue }}</p><label>本地编码器<NSelect v-model:value="models" multiple :disabled="busy" :options="(modelStatus ?? []).map((m) => ({ value: m.model, label: `${m.label}${m.present ? '' : ' · 未下载'}` }))" placeholder="选择 CLIP / CSD / MegaStyle" aria-label="本地编码器" /></label>
        <p v-if="modelError" class="error">{{ modelError }} <VButton @click="loadModels">重试模型列表</VButton></p>
        <p v-else-if="modelStatus === null" class="hint">正在读取本地模型…</p>
        <p class="hint">本地编码支持 2–32 张并保留全图对计算；视觉模型支持 2–8 张，以图 1 为基准返回最多三名。模型给出的 0–100 分只用于本轮候选排序，不能跨模型横向比较。</p>
        <details><summary>视觉模型排序补充要求</summary><textarea v-model="prompt" :disabled="busy" rows="6" aria-label="视觉模型排序补充要求" /></details>
        <div class="inline"><VCost kind="local" label="本地编码不调用 API" /><VCost kind="model" label="视觉比较产生模型费用" /></div>
        <div class="inline"><VButton class="primary" :disabled="busy || !!providerIssue || images.length < 2 || (!providers.length && !models.length)" @click="start">开始比较</VButton><VButton v-if="busy" :disabled="stopRequested" @click="stop">{{ stopRequested ? '停止中…' : '停止后续比较' }}</VButton></div>
        <p v-if="stopRequested" class="hint">已停止后续提交；正在处理的请求可能仍会完成。</p>
        <p v-if="actionError" class="error" role="alert">{{ actionError }}</p>
      </VCard>
    </div>
    <VCard title="比较结果" subtitle="视觉模型分别给出最接近图 1 的前三名；本地编码器仍按人工标签统计">
      <label v-if="history.length">最近 10 次比较记录<NSelect :value="selectedHistory" :disabled="busy" :options="history.map((r) => ({ value: r.id, label: `${r.created_at} · ${r.images.length} 图 · ${r.models.length + r.providers.length} 模型` }))" placeholder="查看历史比较" aria-label="比较历史" @update:value="viewHistory" /></label>
      <p v-if="selectedHistory" class="hint">已恢复历史的图片、模型、阈值与结果。修改人工标签或阈值会在当前界面重新计算指标，历史快照保留。</p>
      <p v-if="!local && !localError && !visual.length && !busy" class="empty">配置同一组图片后开始比较，结果会保留在这里。</p>
      <p v-if="localRunning" class="hint" role="status">正在本机编码图片…</p><p v-if="localError" class="error" role="alert">{{ localError }}</p>
      <article v-for="result in local?.results ?? []" :key="result.model" class="comparison-result"><h3>{{ result.label }}</h3><p class="hint">{{ result.total_ms }} ms · {{ result.device ?? '未运行' }} · {{ result.preprocessing }}</p><p class="hint">版本 {{ result.model_version }}<template v-if="result.fallback_reason"> · {{ result.fallback_reason }}</template></p><p v-if="result.error" class="error">{{ result.error.message }}</p><template v-else><label class="threshold">同风格判定阈值<input v-model.number="thresholds[result.model]" type="number" min="-1" max="1" step="0.01" :aria-label="`${result.label} 阈值`" /></label><p class="hint">阈值未经校准。这里仅用它评估这组人工标注，不修改检索设置。</p><p v-if="!validThreshold(result.model)" class="error">阈值须为 -1 到 1 之间的数值，当前不计算准确率。</p><p class="metrics">准确率 {{ percent(localMetrics(result).accuracy) }} · 精确率 {{ percent(localMetrics(result).precision) }} · 召回率 {{ percent(localMetrics(result).recall) }} · 有标签 {{ localMetrics(result).evaluated }} 对</p><div class="table-scroll"><table><thead><tr><th>图对</th><th>余弦相似度</th><th>按阈值判定</th></tr></thead><tbody><tr v-for="p in result.pairs" :key="p.left_id + p.right_id"><td>{{ imageName(p.left_id) }} / {{ imageName(p.right_id) }}</td><td>{{ p.cosine.toFixed(4) }}</td><td>{{ !validThreshold(result.model) ? '阈值无效' : p.cosine >= (thresholds[result.model] ?? .8) ? '同风格' : '不同风格' }}</td></tr></tbody></table></div></template></article>
      <article v-for="row in visual" :key="row.provider_id" class="comparison-result"><h3>{{ row.provider_id }}</h3><p v-if="row.loading" role="status">视觉模型正在查看全部图片并排序…</p><p v-else-if="row.error" class="error" role="alert">{{ row.error }} <VButton :disabled="busy" @click="retryVisual(row)">重试此模型（API）</VButton></p><template v-else-if="row.result"><p class="hint">{{ row.result.latency_ms }} ms · 基准 {{ imageName(row.result.reference_id ?? frozenImages[0]?.id ?? '') }} · {{ row.result.ranking?.length ?? 0 }} 个有效名次</p><p v-if="row.note" class="hint">{{ row.note }}</p><div v-if="row.result.ranking?.length" class="ranking-list"><article v-for="item in row.result.ranking" :key="item.image_id" class="ranking-item"><div class="rank-number">第 {{ item.rank }} 名</div><img v-if="comparisonImageSrc(item.image_id)" :src="comparisonImageSrc(item.image_id)" :alt="`${imageName(item.image_id)} 排名预览`" /><div><strong>{{ imageName(item.image_id) }}</strong><p class="metrics">模型相似度 {{ item.similarity.toFixed(1) }} / 100 · 仅限本轮相对排序</p><p>{{ item.reason }}</p></div></article></div><template v-else-if="row.result.pairs?.length"><p class="hint">旧版两两判断仅供历史查看，请重试生成新排名。</p><div class="table-scroll"><table><thead><tr><th>图对</th><th>旧版判断</th><th>理由</th></tr></thead><tbody><tr v-for="p in row.result.pairs" :key="p.left_id + p.right_id"><td>{{ imageName(p.left_id) }} / {{ imageName(p.right_id) }}</td><td>{{ p.same_style === null ? '无法判断' : p.same_style ? '同风格' : '不同风格' }}</td><td>{{ p.reason }}</td></tr></tbody></table></div></template><p v-else class="hint">未返回可用的结构化排名，保留模型原文。</p><details><summary>模型原文</summary><pre>{{ row.result.text }}</pre></details></template><p v-else class="hint">{{ stopRequested ? '已停止，未提交' : '等待比较' }}</p></article>
    </VCard>
  </div>
</template>

<style scoped>
.review-workspace{display:grid;grid-template-columns:minmax(320px,.95fr) minmax(0,1.2fr);gap:16px;align-items:start}.review-config{display:flex;flex-direction:column;gap:16px;min-width:0}.inline{display:flex;flex-wrap:wrap;gap:10px;align-items:center}.sample-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(130px,1fr));gap:12px;margin:14px 0}.sample{border:1px solid var(--line);border-radius:var(--radius);padding:10px;display:flex;flex-direction:column;gap:8px;min-width:0}.sample img{width:100%;height:130px;object-fit:contain;background:var(--field)}label{display:flex;flex-direction:column;gap:6px;margin-bottom:12px;min-width:0}input,textarea{width:100%;min-width:0}.hint{color:var(--muted);font-size:12px;margin:8px 0;overflow-wrap:anywhere}.error{color:var(--danger);overflow-wrap:anywhere}.empty{color:var(--muted);padding:24px 0}.comparison-result{padding:16px 0;border-bottom:1px solid var(--line)}h3{font-size:14px;overflow-wrap:anywhere;margin:0 0 6px}.threshold{flex-direction:row;align-items:center;margin-top:12px}.threshold input{width:100px}.metrics{color:var(--secondary);font-size:12px;margin:10px 0}.ranking-list{display:grid;gap:10px;margin:12px 0}.ranking-item{display:grid;grid-template-columns:auto 84px minmax(0,1fr);gap:12px;align-items:start;border:1px solid var(--line);border-radius:var(--radius);padding:10px}.ranking-item img{width:84px;height:84px;object-fit:contain;background:var(--field);border-radius:8px}.ranking-item p{margin:6px 0;overflow-wrap:anywhere}.rank-number{color:var(--secondary);font-weight:700;white-space:nowrap}.table-scroll{overflow-x:auto;max-width:100%}table{border-collapse:collapse;width:100%;min-width:300px;font-size:12px}td,th{padding:8px;text-align:left;border-bottom:1px solid var(--line);vertical-align:top}td:last-child{overflow-wrap:anywhere;min-width:100px}summary{cursor:pointer;padding:10px 0}pre{white-space:pre-wrap;overflow:auto;max-height:350px;font-size:12px;overflow-wrap:anywhere}@media(max-width:1100px){.review-workspace{grid-template-columns:minmax(0,1fr)}}@media(max-width:620px){.ranking-item{grid-template-columns:auto minmax(0,1fr)}.ranking-item img{grid-row:2;grid-column:1 / -1;width:100%;height:160px}.ranking-item>div:last-child{grid-column:2}}
</style>
