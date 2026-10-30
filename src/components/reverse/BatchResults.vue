<script setup lang="ts">
import VButton from "../../ui/VButton.vue";
import { computed } from "vue";
import { imgSrc } from "../../api";
import { useReverse } from "../../stores/reverse";
import { fmtMs, fmtPct, fmtTime } from "../../lib/format";
import {
  STYLE_RUN_BADGE,
  fmtLeaks,
  fmtRenderCell,
  isStyleRun,
  renderState,
  styleRunTitle,
  summaryRenderCells,
  summaryStyleCells,
  truthKindBadge,
} from "../../lib/reverse-summary";
import type { RunRow } from "../../types";

const rev = useReverse();

const batchId = computed({
  get: () => rev.currentBatchId ?? "",
  set: (v: string) => {
    void rev.selectBatch(v || null);
  },
});

const STATUS_LABEL: Record<RunRow["status"], string> = {
  ok: "OK",
  refused: "拒答",
  error: "错误",
};

function questionLabel(r: RunRow): string {
  return r.question_id ?? "本地图片";
}
/** 重画一格的 title：失败给原因，成功给评委的一句理由与评语。 */
function renderTitle(r: RunRow): string {
  if (r.render_error) return r.render_error;
  const parts = [r.similarity_reason, ...(r.quality_notes ?? [])].filter(
    Boolean,
  );
  return parts.join("\n");
}
</script>

<template>
  <div class="results">
    <div v-if="rev.batchesError" class="read-error" role="alert">
      批次读取失败：{{ rev.batchesError }}
    </div>
    <div v-else-if="rev.batchesLoading" class="dim">读取批次…</div>
    <div class="head">
      <select v-model="batchId">
        <option value="">选择批次…</option>
        <option v-for="b in rev.batches" :key="b.batch_id" :value="b.batch_id">
          {{ fmtTime(b.created_at) }} · {{ b.template }} / {{ b.ruleset }} ·
          {{ b.providers.join("+") }} · {{ b.question_count }} 题 · {{ b.status
          }}{{ b.note ? " · " + b.note : "" }}
        </option>
      </select>
      <VButton class="mini" @click="rev.loadBatches()">刷新</VButton>
    </div>

    <div v-if="rev.resultsError" class="read-error" role="alert">
      {{ rev.resultsError
      }}<VButton @click="rev.selectBatch(rev.currentBatchId)">重试结果</VButton>
    </div>
    <div v-else-if="rev.resultsLoading" class="dim">读取结果…</div>
    <template v-else-if="rev.results">
      <div class="dim small">
        batch {{ rev.results.batch.batch_id }} ·
        {{ rev.results.batch.status }} · 建于
        {{ fmtTime(rev.results.batch.created_at)
        }}<template v-if="rev.results.batch.finished_at">
          · 完成 {{ fmtTime(rev.results.batch.finished_at) }}</template
        >
      </div>

      <div class="table-scroll">
        <table class="tbl summary">
          <thead>
            <tr>
              <th>模型</th>
              <th>runs</th>
              <th>ok</th>
              <th>拒答</th>
              <th>错误</th>
              <th
                title="参与 P/R/F1 的题数：truth_kind=tags 且有指标的 run（散文题、本地图片题不算）"
              >
                题数
              </th>
              <th>P</th>
              <th>R</th>
              <th>F1</th>
              <th>外貌泄漏</th>
              <th>禁词泄漏</th>
              <th>矛盾</th>
              <th title="取到重画图的 run 数">重画</th>
              <th title="重画图的质量分均值（0–10，插件三评委）">重画质量</th>
              <th title="成对评委 像原图 均值（0–10，忽略角色外貌差异）">
                像原图
              </th>
              <th
                title="精准反推（§12.7）：用到检索画风的张数（render_artist_source = style）"
              >
                检索命中
              </th>
              <th
                title="这些 run 的检索相似度（余弦，-1..1）均值；与 pool 模式的 像原图 之差 = 画风检索的贡献"
              >
                检索相似度均值
              </th>
              <th>平均延迟</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="s in rev.results.summary" :key="s.provider_id">
              <td>{{ s.provider }}</td>
              <td>{{ s.runs }}</td>
              <td>{{ s.ok }}</td>
              <td>{{ s.refused }}</td>
              <td>{{ s.errors }}</td>
              <td>{{ summaryRenderCells(s).tags_n }}</td>
              <td>{{ fmtPct(s.precision) }}</td>
              <td>{{ fmtPct(s.recall) }}</td>
              <td>
                <b>{{ fmtPct(s.f1) }}</b>
              </td>
              <td>{{ s.appearance_leak }}</td>
              <td>{{ s.banned_leak }}</td>
              <td>{{ s.contradiction }}</td>
              <td>{{ summaryRenderCells(s).render_n }}</td>
              <td>{{ summaryRenderCells(s).quality_avg }}</td>
              <td>{{ summaryRenderCells(s).similarity_avg }}</td>
              <td>{{ summaryStyleCells(s).style_hit_n }}</td>
              <td class="mono">{{ summaryStyleCells(s).style_score_avg }}</td>
              <td>{{ fmtMs(s.avg_latency_ms) }}</td>
            </tr>
          </tbody>
        </table>
      </div>

      <div class="table-scroll">
        <table class="tbl runs">
          <thead>
            <tr>
              <th></th>
              <th>question</th>
              <th>模型</th>
              <th>状态</th>
              <th>F1</th>
              <th>P</th>
              <th>R</th>
              <th
                title="外貌泄漏 / 禁词泄漏（按清洗前输出算，散文题与本地图片题也有）"
              >
                泄漏
              </th>
              <th title="重画质量 · 像原图（0–10）">重画</th>
              <th>延迟</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="r in rev.results.runs"
              :key="r.run_id"
              :class="[
                r.status,
                { on: rev.runDetail?.run.run_id === r.run_id },
              ]"
              @click="rev.openRun(r.run_id)"
            >
              <td class="thumb-cell">
                <img
                  v-if="r.thumb_path"
                  :src="imgSrc(r.thumb_path) ?? ''"
                  loading="lazy"
                  alt=""
                />
              </td>
              <td class="mono q" :title="questionLabel(r)">
                <span
                  v-if="truthKindBadge(r)"
                  class="badge"
                  :class="r.truth_kind"
                  >{{ truthKindBadge(r) }}</span
                >{{ questionLabel(r) }}
              </td>
              <td>{{ r.provider }}</td>
              <td>
                {{ STATUS_LABEL[r.status]
                }}<span v-if="r.error" class="dim" :title="r.error"> ⓘ</span>
              </td>
              <td>{{ fmtPct(r.metric?.f1) }}</td>
              <td>{{ fmtPct(r.metric?.precision) }}</td>
              <td>{{ fmtPct(r.metric?.recall) }}</td>
              <td>{{ fmtLeaks(r) }}</td>
              <td
                class="render"
                :class="renderState(r)"
                :title="renderTitle(r)"
              >
                <span
                  v-if="isStyleRun(r)"
                  class="badge style"
                  :title="styleRunTitle(r)"
                  >{{ STYLE_RUN_BADGE }}</span
                >{{ fmtRenderCell(r) }}
              </td>
              <td>{{ fmtMs(r.latency_ms) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>
    <div v-else class="dim">选一个批次查看结果</div>
  </div>
</template>

<style scoped>
.results {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px;
  overflow: auto;
  height: 100%;
  font-size: 13px;
}
.head {
  display: flex;
  gap: 8px;
  align-items: center;
}
.head select {
  flex: 1;
  min-width: 0;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 4px 6px;
  border-bottom: 1px solid var(--line);
  white-space: nowrap;
}
.tbl th {
  color: var(--muted);
  font-weight: 500;
}
.runs tr {
  cursor: pointer;
}
.runs tr:hover td {
  background: var(--bg);
}
.runs tr.on td {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
}
.runs tr.refused td,
.runs tr.error td {
  color: var(--danger);
}
.thumb-cell {
  width: 36px;
}
.thumb-cell img {
  width: 32px;
  height: 32px;
  object-fit: cover;
  border-radius: 4px;
  display: block;
}
.q {
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.badge {
  display: inline-block;
  padding: 0 5px;
  margin-right: 4px;
  font-size: 10px;
  line-height: 15px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--line);
  color: var(--muted);
  font-family: system-ui, sans-serif;
}
.badge.prose {
  color: var(--paid);
  border-color: var(--paid);
}
.badge.style {
  color: var(--accent);
  border-color: var(--accent);
}
.render.error {
  color: var(--danger);
}
.render.none {
  color: var(--muted);
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
}
</style>
