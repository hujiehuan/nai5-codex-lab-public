<script setup lang="ts">
import VButton from "../../ui/VButton.vue";
import { computed, onMounted, watch } from "vue";
import { RouterLink } from "vue-router";
import { imgSrc } from "../../api";
import { useReverse } from "../../stores/reverse";
import { useStyle } from "../../stores/style";
import { useSync } from "../../stores/sync";
import { useToast } from "../../stores/toast";
import { copyText } from "../../lib/clipboard";
import { DECISION_LABEL } from "../../lib/decision";
import { ALL_DECISIONS } from "../../lib/query";
import {
  NO_RUN_HINT,
  NO_SOURCE_HINT,
  RERENDER_NO_TAGS_HINT,
  SETUP_LINK_LABEL,
  STYLE_TOP_K_MAX,
  STYLE_TOP_K_MIN,
  buildStyleRequest,
  classifierArtistTag,
  defaultSourceId,
  fmtCosine,
  styleFormError,
  styleSourceOptions,
} from "../../lib/style";
import type { Decision } from "../../types";
import StyleHitList from "./StyleHitList.vue";

const rev = useReverse();
const style = useStyle();
const sync = useSync();
const toast = useToast();
const options = computed(() =>
  styleSourceOptions(rev.selectedQuestionIds, rev.questions, rev.currentRun),
);
const sourceId = computed({
  get: () =>
    options.value.some((o) => o.id === style.sourceId)
      ? style.sourceId
      : defaultSourceId(options.value),
  set: (v: string | null) => {
    style.sourceId = v;
  },
});
const current = computed(
  () => options.value.find((o) => o.id === sourceId.value) ?? null,
);
const currentThumb = computed(() => imgSrc(current.value?.thumb_path ?? null));
const formError = computed(() => styleFormError(style.form));
const locked = computed(
  () => style.busy || style.searching || sync.running || sync.starting,
);
const canSearch = computed(
  () => current.value !== null && !locked.value && formError.value === null,
);
const readyCount = computed(
  () => style.readyModels.length + style.readyClassifiers.length,
);
const selectedModel = computed({
  get: () => style.form.model ?? style.status?.model ?? "",
  set: (v: string) => {
    style.form.model = v || null;
  },
});
watch(
  [
    selectedModel,
    () =>
      style.models.find((m) => m.model === selectedModel.value)?.default_center,
  ],
  ([id]) => {
    const model = style.models.find((m) => m.model === id);
    if (model) style.form.center = model.default_center;
  },
);
onMounted(() => {
  if (!style.statusLoading) void style.refreshStatus();
});
function toggleDecision(d: Decision) {
  const values = new Set(style.form.decisions);
  if (values.has(d)) values.delete(d);
  else values.add(d);
  style.form.decisions = ALL_DECISIONS.filter((x) => values.has(x));
}
async function search(compare = false) {
  if (!current.value || !canSearch.value) return;
  const req = buildStyleRequest(current.value.source, {
    ...style.form,
    model: selectedModel.value || null,
  });
  if (compare) await style.compare(req);
  else await style.search(req);
}
async function copyTag(artist: string) {
  if (await copyText(classifierArtistTag(artist)))
    toast.success("已复制 artist: 画师标签");
}
const target = computed(() => rev.currentRun);
const phases = {
  queued: "等待比较",
  searching: "计算中…",
  done: "完成",
  error: "失败",
};
</script>

<template>
  <div class="style">
    <div class="ctrl">
      <label class="fld src"
        >原图
        <span class="row">
          <img
            v-if="currentThumb"
            :src="currentThumb"
            class="src-thumb"
            alt="待比较的原图"
          />
          <select
            v-model="sourceId"
            :disabled="!options.length || locked"
            :title="current?.label ?? NO_SOURCE_HINT"
          >
            <option v-if="!options.length" :value="null">
              {{ NO_SOURCE_HINT }}
            </option>
            <option v-for="o in options" :key="o.id" :value="o.id">
              {{ o.label }}
            </option>
          </select>
        </span>
      </label>
      <label class="fld"
        >画风检索模型
        <select
          v-model="selectedModel"
          :disabled="locked || !style.models.length"
        >
          <option v-if="!style.models.length" value="">
            {{ style.statusLoading ? "读取模型…" : "尚未读取模型" }}
          </option>
          <option v-for="m in style.models" :key="m.model" :value="m.model">
            {{ m.label }} ·
            {{
              !m.present
                ? "未下载"
                : m.indexed
                  ? `已索引 ${m.indexed}`
                  : "未建索引"
            }}
          </option>
        </select>
      </label>
      <label class="fld"
        >候选数
        <input
          v-model.number="style.form.top_k"
          :disabled="locked"
          type="number"
          :min="STYLE_TOP_K_MIN"
          :max="STYLE_TOP_K_MAX"
      /></label>
      <span class="fld"
        >画风词典决策
        <span class="group">
          <label v-for="d in ALL_DECISIONS" :key="d"
            ><input
              type="checkbox"
              :disabled="locked"
              :checked="style.form.decisions.includes(d)"
              @change="toggleDecision(d)"
            />
            {{ DECISION_LABEL[d] }}</label
          >
        </span>
      </span>
      <label
        class="chk"
        title="查询向量与画风词典向量减去相同的库均值；此开关用于向量检索，不影响画师分类器。"
        ><input
          v-model="style.form.center"
          :disabled="locked"
          type="checkbox"
        />
        减库均值</label
      >
      <VButton :disabled="!canSearch || !selectedModel" @click="search(false)"
        >检索所选画风模型</VButton
      >
      <VButton
        class="primary"
        :disabled="!canSearch || !readyCount"
        @click="search(true)"
        >{{
          style.searching ? "计算中…" : `同图对比（${readyCount} 个已就绪模型）`
        }}</VButton
      >
      <span v-if="formError" class="err">{{ formError }}</span>
    </div>
    <div class="dim">
      画风检索比较法典与 NAX V5 画师词典。对比依次运行已就绪模型，各组分数仅在本模型内比较。
    </div>
    <div class="row wrap dim">
      <RouterLink to="/settings" class="link">{{
        SETUP_LINK_LABEL
      }}</RouterLink>
      <span
        >向量模型 {{ style.readyModels.length }} /
        {{ style.models.length }} 就绪 · 画师识别
        {{ style.readyClassifiers.length }} /
        {{ style.classifiers.length }} 就绪</span
      >
      <VButton
        class="mini"
        :disabled="style.statusLoading || locked"
        @click="style.refreshStatus()"
        >刷新模型</VButton
      >
      <span v-if="style.statusError" class="err">{{ style.statusError }}</span>
    </div>

    <div class="target dim">
      <template v-if="target">
        重画目标：run #{{ target.run_id }} · {{ target.provider }} ·
        <span class="mono">{{ target.question_id ?? "本地图片" }}</span>
        <VButton class="mini" @click="rev.openRun(target.run_id)">打开</VButton>
        <span v-if="!target.cleaned_tags.length" class="err">{{
          RERENDER_NO_TAGS_HINT
        }}</span>
      </template>
      <template v-else
        >{{ NO_RUN_HINT }}，即可将检索命中的完整画风配方用于重画</template
      >
    </div>
    <div v-if="style.searchError" class="errbox">
      <span>{{ style.searchError }}</span>
      <RouterLink v-if="style.searchNeedsSetup" to="/settings" class="link">{{
        SETUP_LINK_LABEL
      }}</RouterLink>
    </div>
    <template v-if="style.result">
      <div class="group-heading">
        画风词典检索 ·
        {{
          style.models.find((m) => m.model === style.result?.model)?.label ??
          style.result.model
        }}
      </div>
      <div class="dim">
        已索引 {{ style.result.indexed }} · {{ style.result.query_ms }} ms ·
        {{ style.result.hits.length }} 条候选
      </div>
      <StyleHitList :hits="style.result.hits" />
    </template>

    <section v-if="style.comparisons.length" class="comparison-section">
      <div class="group-heading">画风词典检索对比</div>
      <div class="dim">
        展示相似画风的完整配方，可复制或用它重画；相似度不会直接作为 NovelAI
        权重。
      </div>
      <article
        v-for="group in style.comparisons"
        :key="group.model"
        class="comparison"
      >
        <div class="group-heading">
          {{ group.label }}
          <span class="dim"
            >{{ phases[group.phase]
            }}<template v-if="group.elapsed_ms !== null">
              · {{ group.elapsed_ms }} ms</template
            ></span
          >
        </div>
        <div v-if="group.error" class="errbox">{{ group.error }}</div>
        <template v-if="group.result">
          <div class="dim">
            已索引 {{ group.result.indexed }} ·
            {{ group.result.hits.length }} 条候选<template
              v-if="!group.result.hits.length"
              >（当前范围没有匹配项）</template
            >
          </div>
          <StyleHitList :hits="group.result.hits" />
        </template>
      </article>
    </section>

    <section
      v-if="style.classifierComparisons.length"
      class="comparison-section"
    >
      <div class="group-heading">画师标签识别对照</div>
      <div class="dim">
        分类器直接读取同一原图，无需画风索引。下列分数是各模型的标签预测分，不代表原作者，也不是
        NovelAI 权重；不与向量相似度混排。
      </div>
      <article
        v-for="group in style.classifierComparisons"
        :key="group.model"
        class="comparison"
      >
        <div class="group-heading">
          {{ group.label }}
          <span class="dim"
            >{{ phases[group.phase]
            }}<template v-if="group.elapsed_ms !== null">
              · {{ group.elapsed_ms }} ms</template
            ></span
          >
        </div>
        <div v-if="group.error" class="errbox">{{ group.error }}</div>
        <div class="table-scroll" v-if="group.result?.hits.length">
          <table class="classifier-table">
            <thead>
              <tr>
                <th>画师标签</th>
                <th>预测分</th>
                <th><span class="dim">复制标签</span></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="hit in group.result.hits" :key="hit.artist">
                <td class="mono">{{ hit.artist }}</td>
                <td class="mono">{{ fmtCosine(hit.score) }}</td>
                <td>
                  <VButton
                    class="mini"
                    title="复制 artist: 画师标签，不包含预测分"
                    @click="copyTag(hit.artist)"
                    >复制</VButton
                  >
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-else-if="group.result" class="dim">没有返回画师标签</div>
      </article>
    </section>
  </div>
</template>

<style scoped>
.style,
.comparison-section,
.comparison {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.style {
  padding: 8px 12px 12px;
  font-size: 12px;
}
.ctrl {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  flex-wrap: wrap;
}
.fld {
  display: flex;
  flex-direction: column;
  gap: 3px;
  color: var(--muted);
}
.fld > .row,
.fld > .group {
  color: var(--ink);
}
.fld.src {
  min-width: 230px;
  flex: 1;
}
.fld.src select {
  flex: 1;
  min-width: 0;
}
.fld input[type="number"] {
  width: 64px;
}
.row,
.group,
.target {
  display: flex;
  align-items: center;
  gap: 8px;
}
.wrap,
.target {
  flex-wrap: wrap;
}
.group label,
.chk {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  white-space: nowrap;
  cursor: pointer;
}
.src-thumb {
  width: 32px;
  height: 32px;
  object-fit: cover;
  border-radius: 4px;
  background: var(--bg);
}
.errbox {
  padding: 8px 10px;
  border: 1px solid var(--danger);
  border-radius: var(--radius);
  color: var(--danger);
}
.link {
  color: var(--accent);
  font-weight: 600;
  text-decoration: none;
}
.group-heading {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 600;
}
.group-heading .dim {
  font-weight: 400;
}
.comparison {
  border: 1px solid var(--line);
  padding: 10px;
  border-radius: var(--radius);
}
.classifier-table {
  border-collapse: collapse;
  max-width: 720px;
  text-align: left;
}
.classifier-table th,
.classifier-table td {
  padding: 5px 10px;
  border-bottom: 1px solid var(--line);
}
.classifier-table td:first-child {
  overflow-wrap: anywhere;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.mini {
  padding: 1px 8px;
  font-size: 11px;
}
.dim {
  color: var(--muted);
}
.err {
  color: var(--danger);
}
@media (max-width: 650px) {
  .fld.src {
    min-width: 100%;
  }
  .group-heading {
    flex-wrap: wrap;
  }
}
</style>
