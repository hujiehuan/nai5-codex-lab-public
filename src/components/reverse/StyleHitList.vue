<script setup lang="ts">
import VCost from "../../ui/VCost.vue";
import VButton from "../../ui/VButton.vue";
import { computed } from "vue";
import { imgSrc } from "../../api";
import { useReverse } from "../../stores/reverse";
import { useToast } from "../../stores/toast";
import { copyText } from "../../lib/clipboard";
import { DECISION_LABEL } from "../../lib/decision";
import { codexLabel } from "../../lib/pool";
import {
  NO_ARTIST_HINT,
  NO_RUN_HINT,
  RERENDER_BUSY_HINT,
  RERENDER_NO_TAGS_HINT,
  fmtCosine,
  hitArtist,
  hitLabel,
  hitMuted,
  hitPath,
  scorePercent,
} from "../../lib/style";
import type { StyleHit } from "../../types";

defineProps<{ hits: StyleHit[] }>();
const rev = useReverse();
const toast = useToast();
// ---- 与重画打通（§12.5）：目标是 store 里的"当前 run"（最近在单题面板打开的那条，面板关掉仍记住）----
const target = computed(() => rev.currentRun);
const rerendering = computed(() => rev.rerenderingRunId !== null);
/** 不能重画的原因（按钮 title）；null = 可以。 */
function rerenderBlock(hit: StyleHit): string | null {
  if (!hitArtist(hit)) return NO_ARTIST_HINT;
  if (!target.value) return NO_RUN_HINT;
  if (!target.value.cleaned_tags.length) return RERENDER_NO_TAGS_HINT;
  if (rerendering.value) return RERENDER_BUSY_HINT;
  return null;
}
function rerenderTitle(hit: StyleHit): string {
  const n = rev.renderForm.count;
  return (
    rerenderBlock(hit) ??
    `按 run #${target.value?.run_id} 的反推标签 + 这条的完整画风配方（指定画风）重画${n ? `${n} 张取最高分` : "（张数按插件默认）"}并评分（覆盖旧结果，消耗点数）`
  );
}
/** §12.6：指定画师 = artist_mode="given" + 命中串；张数沿用实验配置里当前选的。 */
async function useForRerender(hit: StyleHit) {
  const artist = hitArtist(hit);
  const run = target.value;
  if (!artist || !run || rerenderBlock(hit)) return;
  await rev.rerender(run.run_id, artist, "given");
}
async function copyArtist(hit: StyleHit) {
  const a = hitArtist(hit);
  if (a && (await copyText(a))) toast.success("已复制完整画风配方");
}
</script>

<template>
  <div v-if="hits.length" class="cards">
    <div
      v-for="h in hits"
      :key="h.entry_id"
      class="hit"
      :class="{ muted: hitMuted(h) }"
    >
      <img
        v-if="imgSrc(h.thumb_path)"
        :src="imgSrc(h.thumb_path)!"
        loading="lazy"
        decoding="async"
        alt=""
      />
      <div v-else class="noimg">无图</div>
      <div class="body">
        <div class="t" :title="hitLabel(h)">{{ h.title }}</div>
        <div class="dim small" :title="hitPath(h)">
          {{ codexLabel(h.codex_id)
          }}<template v-if="hitPath(h)"> · {{ hitPath(h) }}</template>
        </div>
        <div class="badges">
          <span class="badge" :class="h.decision">{{
            DECISION_LABEL[h.decision]
          }}</span>
          <span
            v-if="h.prose"
            class="badge prose"
            title="散文条目：完整画风配方是整段原文"
            >散文</span
          >
        </div>
        <div
          class="sim"
          :title="`余弦相似度 ${fmtCosine(h.score)}（条宽按 0..1 截断）`"
        >
          <span class="bar"
            ><span class="fill" :style="{ width: scorePercent(h.score) + '%' }"
          /></span>
          <span class="mono num">{{ fmtCosine(h.score) }}</span>
        </div>
        <pre
          v-if="hitArtist(h)"
          class="artist mono"
          :title="hitArtist(h) ?? ''"
          >{{ hitArtist(h) }}</pre
        >
        <div v-else class="dim small">{{ NO_ARTIST_HINT }}</div>
        <div class="row">
          <VButton class="mini" :disabled="!hitArtist(h)" @click="copyArtist(h)"
            >复制完整画风配方</VButton
          >
          <VButton
            class="mini"
            :disabled="rerenderBlock(h) !== null"
            :title="rerenderTitle(h)"
            @click="useForRerender(h)"
            >{{ rerendering ? "重画中…" : "用它重画" }}</VButton
          ><VCost kind="free" label="点数" /><VCost
            kind="paid"
            label="可能消耗 Anlas"
          /><VCost kind="model" label="评审模型调用" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr));
  gap: 10px;
}
.hit {
  display: flex;
  gap: 8px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 8px;
  background: var(--panel);
  min-width: 0;
}
.hit.muted {
  opacity: 0.55;
  filter: grayscale(0.6);
}
.hit img,
.noimg {
  width: 96px;
  height: 96px;
  object-fit: cover;
  border-radius: 4px;
  background: var(--bg);
  flex: 0 0 96px;
}
.noimg {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: var(--muted);
}
.body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.t {
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.badges {
  display: flex;
  gap: 4px;
}
.badge {
  display: inline-block;
  padding: 0 5px;
  font-size: 10px;
  line-height: 15px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--line);
  color: var(--muted);
}
.badge.include {
  color: var(--green);
  border-color: var(--green);
}
.badge.exclude {
  color: var(--muted);
  border-color: var(--muted);
}
.badge.prose {
  color: var(--paid);
  border-color: var(--paid);
}
.sim {
  display: flex;
  align-items: center;
  gap: 6px;
}
.bar {
  flex: 1;
  height: 6px;
  border-radius: 3px;
  background: var(--line);
  overflow: hidden;
}
.fill {
  display: block;
  height: 100%;
  background: var(--accent);
  border-radius: 3px;
}
.num {
  font-size: 11px;
  width: 48px;
  text-align: right;
}
.artist {
  margin: 0;
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-all;
  background: var(--bg);
  padding: 4px 6px;
  border-radius: 4px;
  max-height: 4.5em;
  overflow: auto;
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
.small {
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.err {
  color: var(--danger);
}
</style>
