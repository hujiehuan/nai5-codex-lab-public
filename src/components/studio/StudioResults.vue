<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { studioApi } from "../../api-studio";
import { errMessage, imgSrc } from "../../api";
import { bytesLabel, studioTerminal } from "../../lib/studio";
import { createCreativeImageLoader } from "../../lib/creative-image-loader";
import type { StudioPlan, StudioRun, StudioImageResult } from "../../types-studio";
import VButton from "../../ui/VButton.vue";
import VCost from "../../ui/VCost.vue";
import StudioImagePreview from "./StudioImagePreview.vue";

const props = defineProps<{ plan: StudioPlan | null; run: StudioRun | null; active?: boolean; jpeg?: boolean; quality?: number }>();
type ImageState = { loading: boolean; value: StudioImageResult | null; error: string | null; pending: boolean };
const states = reactive<Record<string, ImageState>>({});
const opened = ref<string | null>(null);
const effectivePlan = computed(() => props.plan ?? props.run?.plan ?? null);
const referenceImageCount = computed(() => {
  const value = effectivePlan.value?.settings.reference_image_count;
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
});
const suffix = () => `${props.jpeg ? "jpeg" : "png"}:${props.quality ?? 88}`;
const key = (idx: number) => `${props.run?.run_id}:${idx}:${suffix()}`;
let previousRun: { id: string; images: number[] } | null = null;
function makeLoader() {
  const format = props.jpeg ? "jpeg" : "png", quality = props.quality ?? 88;
  const extra = `${format}:${quality}`;
  return createCreativeImageLoader(async (runId, idx) => {
    const id = `${runId}:${idx}:${extra}`;
    try {
      const result = await studioApi.image(runId, idx, format, quality);
      if (!result || typeof result.path !== "string" || typeof result.original_path !== "string" || !Number.isFinite(result.original_bytes) || !Number.isFinite(result.encoded_bytes)) throw new Error("取图返回结构错误");
      states[id] ??= { loading: true, value: null, error: null, pending: false };
      states[id].value = result;
      return result.path;
    } catch (error) {
      if ((error as { code?: string })?.code === "studio_image_pending") throw { code: "creative_image_pending", message: "图片准备中" };
      throw error;
    }
  }, (base, value) => {
    const id = `${base}:${extra}`;
    const current = states[id] ??= { loading: false, value: null, error: null, pending: false };
    current.loading = value.status === "loading";
    current.pending = value.status === "pending";
    current.error = value.status === "error" ? value.error : null;
  }, errMessage);
}
let loader = makeLoader();
function loadCurrent() {
  const run = props.run;
  if (previousRun && previousRun.id !== run?.run_id) for (const idx of previousRun.images) loader.ensure(previousRun.id, idx, false);
  if (run) {
    const indices = run.images.filter((i) => i.status === "done").map((i) => i.idx);
    for (const idx of indices) loader.ensure(run.run_id, idx, !!props.active && !studioTerminal(run.status));
    previousRun = { id: run.run_id, images: indices };
  }
}
watch(() => [props.jpeg, props.quality], () => { loader.dispose(); loader = makeLoader(); previousRun = null; loadCurrent(); });
watch(() => [props.run, props.active], loadCurrent, { immediate: true, deep: true });
function loadImage(runId: string, idx: number, _manual = true) { loader.retry(runId, idx); }
onBeforeUnmount(() => loader.dispose());
function field(record: unknown, name: string): string { return record && typeof record === "object" && name in record ? String((record as Record<string, unknown>)[name] ?? "") : ""; }
</script>

<template>
  <div class="studio-results">
    <template v-if="effectivePlan">
      <div class="plan-meta"><strong>{{ effectivePlan.mode === 'reverse' ? '反推方案' : `${effectivePlan.plans.length} 个独立构思` }}</strong><span class="mono">{{ effectivePlan.plan_id.slice(0, 12) }}</span></div>
      <div v-if="effectivePlan.warnings.length" class="notice"><p v-for="w in effectivePlan.warnings" :key="w">{{ w }}</p></div>
      <p class="hint">{{ field(effectivePlan.settings, 'provider_id') }} · 角色 {{ field(effectivePlan.settings, 'characters') || field(effectivePlan.settings, 'character') || '见各方案' }}</p>
      <p v-if="effectivePlan.mode !== 'reverse'" class="hint" data-testid="studio-reference-summary"><template v-if="effectivePlan.settings.reference_content === 'text'">已提供 {{ effectivePlan.plans[0]?.reference_ids?.length ?? '—' }} 条完整文字，未附参考图（0 张）。</template><template v-else-if="referenceImageCount !== null">已提供 {{ effectivePlan.plans[0]?.reference_ids?.length ?? '—' }} 条完整文字＋{{ referenceImageCount }} 张主参考图。所有图片与提示词在同一次构思中提供。</template><template v-else>旧方案：仅文字参考，未提供图包图片。</template></p>
      <details v-for="(plan, index) in effectivePlan.plans" :key="plan.plan_id || index" :open="index === 0" class="plan-detail"><summary>方案 {{ index + 1 }} · {{ plan.concept || '查看冻结提示词' }}</summary><p v-if="plan.concept">{{ plan.concept }}</p><p class="hint">{{ plan.size }} · {{ plan.steps }} 步 · {{ plan.model }}<template v-if="Array.isArray(plan.reference_ids)"> · 已提供 {{ plan.reference_ids.length }} 条参考</template><template v-if="Array.isArray(plan.used_reference_ids)"> · 模型声明借鉴 {{ plan.used_reference_ids.length }} 条</template></p><p v-if="plan.requirements" class="requirements">原始要求：{{ plan.requirements }}</p><details v-if="plan.requirement_checks?.length"><summary>模型要求自检（成图仍需人工确认）</summary><p v-for="(check, at) in plan.requirement_checks" :key="at">{{ check.requirement }}：{{ check.evidence }}</p></details><label>完整提示词<pre>{{ plan.prompt }}</pre></label><details><summary>负面词与角色提示词</summary><pre>{{ plan.negative }}</pre><pre v-for="(character, at) in plan.character_prompts" :key="at">{{ character }}</pre></details><details><summary>模型原文</summary><pre>{{ plan.llm_raw }}</pre></details></details>
    </template>
    <template v-if="run">
      <div class="run-meta"><strong>运行 {{ run.run_id.slice(0, 10) }}</strong><VCost kind="free" /><span>实际 Anlas {{ run.anlas_spent === null ? '未核实' : run.anlas_spent }}</span></div>
      <p v-if="run.error" class="error" role="alert">{{ run.error }}</p><div v-if="run.warnings.length" class="notice"><p v-for="w in run.warnings" :key="w">{{ w }}</p></div>
      <div class="image-grid"><article v-for="image in run.images" :key="image.idx" class="result-image">
        <button v-if="states[key(image.idx)]?.value" class="image-open" @click="opened = states[key(image.idx)].value!.path"><img :src="imgSrc(states[key(image.idx)].value!.path) ?? undefined" :alt="`实验结果 ${image.idx + 1}`" /></button>
        <div v-else class="image-placeholder" :aria-busy="image.status === 'running' || states[key(image.idx)]?.loading"><p v-if="image.error" class="error">{{ image.error }}</p><template v-else-if="states[key(image.idx)]?.error"><p class="error">{{ states[key(image.idx)].error }}</p><VButton @click="loadImage(run.run_id, image.idx, true)">重试取图</VButton></template><p v-else>{{ image.status === 'done' ? '图片准备中…' : image.status === 'running' ? '生成中…' : image.status === 'cancelled' ? '已停止' : image.status === 'error' ? '生成失败' : '等待生成' }}</p></div>
        <div class="image-info"><strong>#{{ image.idx + 1 }}</strong><span class="mono">seed {{ image.seed }}</span><p v-if="image.score !== null">综合评分 {{ image.score }}</p><details v-if="image.judges?.length"><summary>各评委评分</summary><p v-for="judge in image.judges" :key="judge.provider_id" class="hint">{{ judge.provider_id }} · {{ judge.latency_ms }} ms<br /><span :class="{ error: !!judge.error }">{{ judge.error || (judge.score === null ? '无有效评分' : `${judge.score} 分`) }}</span></p></details><p v-for="note in image.notes ?? []" :key="note" class="hint">{{ note }}</p><p v-if="states[key(image.idx)]?.value" class="hint">PNG 原图 {{ bytesLabel(states[key(image.idx)].value!.original_bytes) }}<template v-if="states[key(image.idx)].value!.format === 'jpeg'"> · JPEG {{ bytesLabel(states[key(image.idx)].value!.encoded_bytes) }} / 质量 {{ states[key(image.idx)].value!.quality ?? quality }}</template></p><VButton v-if="states[key(image.idx)]?.value?.format === 'jpeg'" @click="opened = states[key(image.idx)].value!.original_path">查看 PNG 原图</VButton></div>
      </article></div>
    </template>
    <p v-if="!effectivePlan && !run" class="empty">选择矩阵中的一行，查看它的冻结方案和结果。</p>
    <StudioImagePreview :path="opened" @close="opened = null" />
  </div>
</template>

<style scoped>
.studio-results{min-width:0}.plan-meta,.run-meta{display:flex;align-items:center;gap:10px;flex-wrap:wrap}.plan-meta span,.hint{color:var(--muted);font-size:12px;overflow-wrap:anywhere}.hint{margin:8px 0}.plan-detail{border-top:1px solid var(--line);padding:8px 0}.plan-detail summary{cursor:pointer;padding:8px 0;overflow-wrap:anywhere}.plan-detail>p{margin-bottom:10px;overflow-wrap:anywhere}.plan-detail label{display:block;margin:10px 0}.plan-detail pre{background:var(--field);border-radius:var(--radius);padding:12px;margin:6px 0;white-space:pre-wrap;overflow:auto;max-height:300px;overflow-wrap:anywhere;font-size:12px}.requirements{padding:10px;background:var(--accent-soft);border-radius:var(--radius)}.run-meta{margin:20px 0 14px}.notice{padding:10px;color:var(--paid);background:var(--paid-soft);border-radius:var(--radius);font-size:12px;margin:10px 0}.notice p+ p{margin-top:6px}.error{color:var(--danger);overflow-wrap:anywhere}.empty{padding:28px 0;color:var(--muted)}.image-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:12px}.result-image{min-width:0;background:var(--field);border:1px solid var(--line);border-radius:var(--radius);overflow:hidden}.image-open{display:block;padding:0;border:0;width:100%;background:var(--field);cursor:zoom-in}.image-open img{width:100%;height:280px;object-fit:contain;display:block}.image-placeholder{min-height:220px;display:flex;flex-direction:column;justify-content:center;align-items:center;text-align:center;padding:16px;gap:12px;color:var(--muted)}.image-info{padding:12px}.image-info>span{font-size:11px;color:var(--muted);margin-left:8px}
</style>
