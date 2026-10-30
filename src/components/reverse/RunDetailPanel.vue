<script setup lang="ts">
import VCost from "../../ui/VCost.vue";
import VButton from "../../ui/VButton.vue";
import { computed, nextTick, ref, watch } from "vue";
import { api, imgSrc, errMessage } from "../../api";
import { useReverse } from "../../stores/reverse";
import { buildTagClouds } from "../../lib/tagcloud";
import { fmtDateTime, fmtMs, fmtPct } from "../../lib/format";
import { fmtScore } from "../../lib/lab";
import {
  NO_PLUGIN_DROPPED_NOTE,
  WOULD_REJECT_NOTE,
  artistSourceLabel,
  bestCandidateIndex,
  candidateLabel,
  fmtRenderSize,
  hasAlignInfo,
  leakCounts,
  renderState,
  styleHitInfo,
  truthKindBadge,
  truthKindNote,
} from "../../lib/reverse-summary";
import TagCloud from "./TagCloud.vue";

const rev = useReverse();
const detail = computed(() => rev.runDetail);
/** 最近一次手动重画的尝试参数；失败也保留，与批次开场的 request.render 分开显示。 */
const lastRenderStyleModel = computed(() => {
  const request = detail.value?.request;
  if (
    !request ||
    typeof request !== "object" ||
    !("last_render_request" in request)
  )
    return null;
  const last = request.last_render_request;
  if (
    !last ||
    typeof last !== "object" ||
    !("artist_mode" in last) ||
    last.artist_mode !== "style" ||
    !("style_model" in last)
  )
    return null;
  return typeof last.style_model === "string" && last.style_model
    ? last.style_model
    : null;
});
const clouds = computed(() =>
  detail.value ? buildTagClouds(detail.value) : null,
);
const sourceImage = ref<Awaited<
  ReturnType<typeof api.reverseSourceImage>
> | null>(null);
const sourceError = ref("");
const sourceLoading = ref(false);
watch(
  () => detail.value?.run.run_id,
  async (id, _, onCleanup) => {
    let active = true;
    onCleanup(() => {
      active = false;
    });
    sourceImage.value = null;
    sourceError.value = "";
    sourceLoading.value = id != null;
    if (id == null) return;
    try {
      const result = await api.reverseSourceImage(id);
      if (active) sourceImage.value = result;
    } catch (e) {
      if (active) sourceError.value = errMessage(e);
    } finally {
      if (active) sourceLoading.value = false;
    }
  },
  { immediate: true },
);
const imageSrc = computed(() => imgSrc(sourceImage.value?.path));
// ---- 重画对照（CONTRACT §11.4）：原图与重画图并排、两个分数与理由、"重画"按钮 ----
const renderSrc = computed(() => imgSrc(detail.value?.run.render_path ?? null));
const rState = computed(() =>
  detail.value ? renderState(detail.value.run) : "none",
);
// ---- 与 插件 /反推 对齐的信息（§12.6）：尺寸与来源、画师来源、线上还会剔掉的词、全部候选 ----
const sizeLine = computed(() =>
  detail.value ? fmtRenderSize(detail.value.run) : null,
);
const artistSrc = computed(() =>
  artistSourceLabel(detail.value?.run.render_artist_source),
);
const aligned = computed(
  () => detail.value !== null && hasAlignInfo(detail.value.run),
);
const dropped = computed(() => detail.value?.run.plugin_dropped ?? []);
const wouldReject = computed(
  () => detail.value?.run.plugin_would_reject === true,
);
const candidates = computed(() => detail.value?.run.render_candidates ?? []);
const bestIdx = computed(() =>
  detail.value ? bestCandidateIndex(candidates.value, detail.value.run) : null,
);
/** 点了哪张候选在上面看大图；null = 顶层的重画图（最优那张）。换 run 时复位。 */
const selectedCand = ref<number | null>(null);
const shownCand = computed(() =>
  selectedCand.value !== null
    ? (candidates.value[selectedCand.value] ?? null)
    : null,
);
const shownRenderSrc = computed(() =>
  shownCand.value?.local_path
    ? imgSrc(shownCand.value.local_path)
    : renderSrc.value,
);
const shownSeed = computed(() =>
  shownCand.value
    ? shownCand.value.seed
    : (detail.value?.run.render_seed ?? null),
);
function candTitle(c: { file: string; quality_notes: string[] }): string {
  return [c.file, ...(c.quality_notes ?? [])].join("\n");
}
// ---- 命中画风（§12.7 精准反推）：命中条目的标题 / 相似度 / 决策 / 兜底说明；缩略图 RunRow 里没有，经 get_entry 取展示图的 thumb_path（取不到就不放图）----
const styleHit = computed(() =>
  detail.value ? styleHitInfo(detail.value.run) : null,
);
const styleThumbCache = new Map<string, string | null>();
const styleThumb = ref<string | null>(null);
const styleThumbSrc = computed(() => imgSrc(styleThumb.value));
watch(
  () => styleHit.value?.entry_id ?? null,
  async (id) => {
    styleThumb.value = null;
    if (!id) return;
    if (styleThumbCache.has(id)) {
      styleThumb.value = styleThumbCache.get(id) ?? null;
      return;
    }
    let thumb: string | null = null;
    try {
      const d = await api.getEntry(id);
      const img = d.images.find((i) => i.ordinal === 0) ?? d.images[0];
      thumb = img?.thumb_path ?? img?.local_path ?? null;
    } catch {
      // 条目已删 / 读不到：只是不放图
    }
    styleThumbCache.set(id, thumb);
    if ((styleHit.value?.entry_id ?? null) === id) styleThumb.value = thumb;
  },
  { immediate: true },
);
const kindNote = computed(() =>
  detail.value ? truthKindNote(detail.value.run) : null,
);
const kindBadge = computed(() =>
  detail.value ? truthKindBadge(detail.value.run) : null,
);
const leaks = computed(() =>
  detail.value ? leakCounts(detail.value.run) : null,
);
const rerendering = computed(
  () =>
    detail.value !== null && rev.rerenderingRunId === detail.value.run.run_id,
);
const canRerender = computed(
  () =>
    detail.value !== null &&
    rev.rerenderingRunId === null &&
    detail.value.run.cleaned_tags.length > 0,
);
const rawOpen = ref(false);
const panel = ref<HTMLElement | null>(null);

const score = ref<number | null>(null);
const comment = ref("");
watch(
  detail,
  (d) => {
    score.value = d?.judgment?.score ?? null;
    comment.value = d?.judgment?.comment ?? "";
    rawOpen.value = false;
    selectedCand.value = null;
    if (d) void nextTick(() => panel.value?.focus());
  },
  { immediate: true },
);

async function submitJudge() {
  if (!detail.value || score.value === null) return;
  await rev.judge(detail.value.run.run_id, score.value, comment.value);
}
watch(
  () => rev.runDetailLoading || !!rev.runDetailError,
  (open) => {
    if (open) void nextTick(() => panel.value?.focus());
  },
);
function pretty(v: unknown) {
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}
</script>

<template>
  <div
    v-if="rev.runDetailLoading || rev.runDetailError"
    class="panel-wrap"
    @keydown.esc="rev.closeRun"
  >
    <div class="backdrop" @click="rev.closeRun" />
    <aside ref="panel" class="panel" tabindex="-1">
      <header>
        <span>运行详情</span><VButton @click="rev.closeRun">关闭</VButton>
      </header>
      <div v-if="rev.runDetailError" class="read-error" role="alert">
        {{ rev.runDetailError }}
      </div>
      <div v-else class="body" aria-busy="true">正在读取运行详情…</div>
    </aside>
  </div>
  <div v-else-if="detail" class="panel-wrap" @keydown.esc="rev.closeRun()">
    <div class="backdrop" @click="rev.closeRun()" />
    <aside ref="panel" class="panel" tabindex="-1">
      <header>
        <span
          >run #{{ detail.run.run_id }} · {{ detail.run.provider }} ·
          {{ detail.run.status }} · {{ fmtMs(detail.run.latency_ms)
          }}<span
            v-if="kindBadge"
            class="badge"
            :class="detail.run.truth_kind"
            >{{ kindBadge }}</span
          ></span
        >
        <VButton class="close" @click="rev.closeRun()">×</VButton>
      </header>
      <div class="body">
        <div class="pair">
          <div class="img">
            <div class="k">
              原图
              <span v-if="sourceImage?.kind === 'thumbnail'" class="dim small"
                >仅有缩略图，清晰度受限</span
              ><span
                v-else-if="sourceImage?.kind === 'display'"
                class="dim small"
                >已下载的展示图</span
              >
            </div>
            <img
              v-if="imageSrc"
              :src="imageSrc"
              alt="反推来源图片"
              @error="sourceError = '图片文件无法显示'"
            />
            <div v-if="sourceError" class="err" role="alert">
              {{ sourceError }}
            </div>
            <div v-else-if="!imageSrc" class="dim ph">
              {{ sourceLoading ? "正在读取原图…" : "本地来源图片不存在" }}
            </div>
            <div class="dim small mono">
              {{ detail.run.question_id ?? "本地图片" }}
            </div>
          </div>
          <div class="img">
            <div class="k">
              重画图<span v-if="shownSeed !== null" class="dim">
                · seed {{ shownSeed }}</span
              ><span v-if="shownCand && selectedCand !== bestIdx" class="dim">
                · 候选 #{{ (selectedCand ?? 0) + 1 }}</span
              >
            </div>
            <img v-if="shownRenderSrc" :src="shownRenderSrc" alt="" />
            <div v-else-if="rState === 'error'" class="ph err small">
              {{ detail.run.render_error }}
            </div>
            <div v-else class="dim ph">
              {{ rState === "ok" ? "重画图未取到" : "未重画" }}
            </div>
            <div class="scores">
              <div
                v-if="sizeLine"
                title="尺寸与来源：按原图比例 = 与 插件 /反推 同一函数按原图宽高比选；按标签 = 缺原图宽高时退到 size_for_tags（§12.6）"
              >
                <span class="k">尺寸</span
                ><span class="mono">{{ sizeLine }}</span>
              </div>
              <div
                v-if="artistSrc || detail.run.render_artist"
                class="artist-line"
                :title="detail.run.render_artist ?? ''"
              >
                <span class="k">画师</span
                ><span
                  v-if="artistSrc"
                  class="badge src"
                  :class="detail.run.render_artist_source ?? ''"
                  >{{ artistSrc }}</span
                ><span v-if="detail.run.render_artist" class="mono">{{
                  detail.run.render_artist
                }}</span>
              </div>
              <div>
                <span class="k">重画质量</span
                ><b>{{ fmtScore(detail.run.quality_score) }}</b
                ><span v-if="candidates.length > 1" class="dim small">
                  · {{ candidates.length }} 张里最高</span
                >
              </div>
              <div v-if="detail.run.quality_notes?.length" class="dim small">
                {{ detail.run.quality_notes.join("；") }}
              </div>
              <div>
                <span class="k">像原图</span
                ><b>{{ fmtScore(detail.run.similarity_score) }}</b>
              </div>
              <div v-if="detail.run.similarity_reason" class="dim small">
                {{ detail.run.similarity_reason }}
              </div>
              <div v-if="rState === 'error' && renderSrc" class="err small">
                {{ detail.run.render_error }}
              </div>
            </div>
            <div class="row">
              <VButton
                class="mini"
                :disabled="!canRerender"
                :title="
                  canRerender
                    ? '按实验配置里当前的画师来源 / 张数再画并评分（覆盖旧结果；消耗点数，可能消耗 Anlas 和模型调用费用）'
                    : '没有清洗后的标签，无法重画'
                "
                @click="rev.rerender(detail.run.run_id)"
              >
                {{
                  rerendering
                    ? "重画中…"
                    : rState === "none"
                      ? "重画"
                      : "重新画"
                }}
              </VButton>
              <VCost kind="free" label="点数" /><VCost
                kind="paid"
                label="可能消耗 Anlas"
              /><VCost kind="model" label="评审模型调用" /><span
                class="dim small"
                >评委、画师来源与张数用实验配置里当前选的；要指定画师重画去底部「画风反推」</span
              >
            </div>
          </div>
        </div>
        <div v-if="lastRenderStyleModel" class="dim small">
          最近重画尝试的画风模型：<span class="mono">{{
            lastRenderStyleModel
          }}</span
          >（包括未成功的尝试；批次初始配置见原始请求）
        </div>
        <div v-if="styleHit" class="stylehit">
          <div class="k">
            命中画风<span class="dim"
              >——精准反推（§12.7）自动检索到与原图最像的条目，重画的画师串取自它</span
            >
          </div>
          <div
            class="hit"
            :class="{
              muted: styleHit.decision === 'exclude' || styleHit.fallback,
            }"
          >
            <img
              v-if="styleThumbSrc"
              :src="styleThumbSrc"
              loading="lazy"
              decoding="async"
              alt=""
            />
            <div v-else class="noimg">无图</div>
            <div class="hit-body">
              <div class="t" :title="styleHit.entry_id ?? ''">
                {{ styleHit.title }}
              </div>
              <div class="row">
                <span class="k">相似度</span
                ><b class="mono">{{ styleHit.score }}</b>
                <span
                  v-if="styleHit.decision_label"
                  class="badge src"
                  :class="styleHit.decision ?? ''"
                  >{{ styleHit.decision_label }}</span
                >
                <span v-if="styleHit.fallback" class="badge src fallback"
                  >已兜底</span
                >
              </div>
              <div v-if="styleHit.note" class="note small">
                {{ styleHit.note }}
              </div>
              <div v-if="styleHit.entry_id" class="dim small mono">
                {{ styleHit.entry_id }}
              </div>
            </div>
          </div>
        </div>
        <div v-if="candidates.length" class="cands">
          <div class="k">
            候选 {{ candidates.length }} 张 · 绿框 =
            最高分那张（就是上面的重画图）；点缩略图在上面看大图，再点回到最优
          </div>
          <div class="strip">
            <div
              v-for="(c, i) in candidates"
              :key="c.file"
              class="cand"
              :class="{ best: i === bestIdx, on: i === selectedCand }"
              :title="candTitle(c)"
              @click="selectedCand = selectedCand === i ? null : i"
            >
              <img
                v-if="imgSrc(c.local_path)"
                :src="imgSrc(c.local_path)!"
                loading="lazy"
                decoding="async"
                alt=""
              />
              <div v-else class="noimg">未取到</div>
              <div class="small mono lbl">{{ candidateLabel(c) }}</div>
            </div>
          </div>
        </div>
        <div v-if="dropped.length || wouldReject" class="online">
          <div class="k">
            线上 /反推 还会剔掉（{{ dropped.length }}）<span class="dim"
              >——这些词进了实验室的提示词，线上 _clean_tags
              口径会多剔（§12.6）</span
            >
          </div>
          <div v-if="dropped.length" class="chips">
            <span
              v-for="(t, i) in dropped"
              :key="`${i}:${t}`"
              class="chip drop"
              >{{ t }}</span
            >
          </div>
          <div v-if="wouldReject" class="err small reject">
            {{ WOULD_REJECT_NOTE }}
          </div>
        </div>
        <div v-else-if="rState === 'ok' && aligned" class="dim small">
          {{ NO_PLUGIN_DROPPED_NOTE }}
        </div>
        <div v-if="kindNote" class="note">{{ kindNote }}</div>

        <div class="top">
          <div class="metrics" v-if="detail.run.metric">
            <div>
              <span class="k">P</span>{{ fmtPct(detail.run.metric.precision) }}
            </div>
            <div>
              <span class="k">R</span>{{ fmtPct(detail.run.metric.recall) }}
            </div>
            <div>
              <span class="k">F1</span><b>{{ fmtPct(detail.run.metric.f1) }}</b>
            </div>
            <div><span class="k">命中</span>{{ detail.run.metric.hit }}</div>
            <div><span class="k">漏</span>{{ detail.run.metric.missed }}</div>
            <div><span class="k">多余</span>{{ detail.run.metric.extra }}</div>
            <div>
              <span class="k">违规命中</span
              >{{ detail.run.metric.expected_hit }}
            </div>
            <div>
              <span class="k">外貌泄漏</span
              >{{ detail.run.metric.appearance_leak }}
            </div>
            <div>
              <span class="k">禁词泄漏</span>{{ detail.run.metric.banned_leak }}
            </div>
            <div>
              <span class="k">规则剔除</span
              >{{ detail.run.metric.ruleset_drop }}
            </div>
            <div>
              <span class="k">矛盾</span>{{ detail.run.metric.contradiction }}
            </div>
          </div>
          <div v-else class="metrics">
            <div><span class="k">P</span>—</div>
            <div><span class="k">R</span>—</div>
            <div><span class="k">F1</span>—</div>
            <div>
              <span class="k">外貌泄漏</span
              >{{ leaks ? leaks.appearance : "—" }}
            </div>
            <div>
              <span class="k">禁词泄漏</span>{{ leaks ? leaks.banned : "—" }}
            </div>
            <div class="dim">
              {{ detail.run.error ?? (kindNote ? "" : "没有指标（失败）") }}
            </div>
          </div>
        </div>

        <div class="legend">
          <span class="lg hit">命中</span><span class="lg miss">漏</span
          ><span class="lg extra">多余</span
          ><span class="lg violation">违规命中</span
          ><span class="lg expected">不计分</span
          ><span class="lg contra">矛盾</span>
        </div>
        <TagCloud v-if="clouds" title="标准答案" :tags="clouds.truth" />
        <TagCloud
          v-if="clouds"
          title="模型输出（清洗后）"
          :tags="clouds.pred"
        />

        <details v-if="detail.pred_raw.length">
          <summary>清洗前输出（{{ detail.pred_raw.length }}）</summary>
          <div class="chips">
            <span v-for="t in detail.pred_raw" :key="t" class="chip">{{
              t
            }}</span>
          </div>
        </details>
        <details v-if="detail.expected_miss.length">
          <summary>
            不计分词（角色 / 外貌 / 人数 / 禁词，{{
              detail.expected_miss.length
            }}）
          </summary>
          <div class="chips">
            <span v-for="t in detail.expected_miss" :key="t" class="chip">{{
              t
            }}</span>
          </div>
        </details>
        <details :open="rawOpen">
          <summary @click.prevent="rawOpen = !rawOpen">
            raw_response 全文
          </summary>
          <pre class="mono">{{ detail.raw_response ?? "（空）" }}</pre>
        </details>
        <details v-if="detail.parsed">
          <summary>解析 JSON</summary>
          <pre class="mono">{{ pretty(detail.parsed) }}</pre>
        </details>
        <details v-if="detail.run.render_prompt">
          <summary>重画 prompt</summary>
          <pre class="mono">{{ detail.run.render_prompt }}</pre>
        </details>
        <details>
          <summary>请求参数</summary>
          <pre class="mono">{{ pretty(detail.request) }}</pre>
        </details>

        <section class="judge">
          <div class="k">人工打分</div>
          <div class="stars">
            <label v-for="n in 5" :key="n"
              ><input v-model.number="score" type="radio" :value="n" />
              {{ n }}</label
            >
          </div>
          <textarea v-model="comment" rows="2" placeholder="评论" />
          <div class="row">
            <VButton
              class="primary"
              :disabled="score === null"
              @click="submitJudge"
              >保存打分</VButton
            >
            <VCost kind="local" label="人工打分免费" /><span
              v-if="detail.judgment"
              class="dim small"
              >上次：{{ detail.judgment.score ?? "—" }} 分 ·
              {{ fmtDateTime(detail.judgment.at) }}</span
            >
          </div>
        </section>
      </div>
    </aside>
  </div>
</template>

<style scoped>
.panel-wrap {
  position: fixed;
  inset: 0;
  z-index: 55;
}
.backdrop {
  position: absolute;
  inset: 0;
  background: var(--backdrop);
}
.panel {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 640px;
  max-width: 92vw;
  background: var(--panel);
  border-left: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  outline: none;
  box-shadow: -8px 0 30px var(--backdrop);
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--line);
  font-weight: 600;
  font-size: 13px;
}
.badge {
  display: inline-block;
  padding: 0 5px;
  margin-left: 6px;
  font-size: 10px;
  line-height: 15px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--line);
  color: var(--muted);
  vertical-align: 1px;
}
.badge.prose {
  color: var(--paid);
  border-color: var(--paid);
}
.pair {
  display: flex;
  gap: 14px;
}
.pair .img {
  flex: 1 1 0;
  min-width: 0;
}
.pair .img img {
  width: 100%;
  max-height: 300px;
  object-fit: contain;
}
.ph {
  min-height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg);
  border-radius: var(--radius);
  padding: 8px;
  text-align: center;
}
.scores {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12px;
}
.artist-line {
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 3.6em;
  overflow: auto;
}
.badge.src {
  margin-left: 0;
  margin-right: 6px;
}
.badge.src.pool {
  color: var(--accent);
  border-color: var(--accent);
}
.badge.src.given {
  color: var(--green);
  border-color: var(--green);
}
.badge.src.style {
  color: var(--accent);
  border-color: var(--accent);
}
.badge.src.include {
  color: var(--green);
  border-color: var(--green);
}
.badge.src.exclude {
  color: var(--muted);
  border-color: var(--muted);
}
.badge.src.fallback {
  color: var(--paid);
  border-color: var(--paid);
}
.stylehit {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.stylehit .hit {
  display: flex;
  gap: 10px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 8px;
  background: var(--bg);
}
.stylehit .hit.muted {
  opacity: 0.7;
}
.stylehit .hit img,
.stylehit .noimg {
  width: 72px;
  height: 72px;
  object-fit: cover;
  border-radius: 4px;
  background: var(--panel);
  flex: 0 0 72px;
}
.stylehit .noimg {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: var(--muted);
}
.stylehit .hit-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 12px;
}
.stylehit .t {
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.stylehit .row {
  gap: 6px;
}
.stylehit .note {
  padding: 4px 8px;
}
.cands {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.strip {
  display: flex;
  gap: 8px;
  overflow-x: auto;
  padding: 2px;
}
.cand {
  flex: 0 0 auto;
  width: 96px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
  border: 2px solid transparent;
  border-radius: var(--radius);
  padding: 2px;
}
.cand.best {
  border-color: var(--green);
}
.cand.on {
  outline: 2px solid var(--accent);
}
.cand img,
.cand .noimg {
  width: 92px;
  height: 92px;
  object-fit: cover;
  border-radius: 4px;
  background: var(--bg);
}
.cand .noimg {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: var(--muted);
}
.cand .lbl {
  text-align: center;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.online {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.online .chips {
  margin-top: 0;
}
.chip.drop {
  background: var(--danger-soft);
  color: var(--danger);
}
.reject {
  background: var(--danger-soft);
  padding: 6px 10px;
  border-radius: var(--radius);
}
.note {
  font-size: 12px;
  color: var(--paid);
  background: var(--paid-soft);
  padding: 6px 10px;
  border-radius: var(--radius);
}
.err {
  color: var(--danger);
}
.close {
  padding: 0 10px;
  font-size: 18px;
}
.body {
  flex: 1;
  overflow: auto;
  padding: 12px 14px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  font-size: 13px;
}
.top {
  display: flex;
  gap: 14px;
}
.img {
  flex: 0 0 220px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.img img {
  width: 220px;
  max-height: 260px;
  object-fit: contain;
  background: var(--bg);
  border-radius: var(--radius);
}
.metrics {
  display: grid;
  grid-template-columns: repeat(3, auto);
  gap: 4px 16px;
  align-content: start;
  font-size: 12px;
}
.k {
  color: var(--muted);
  font-size: 11px;
  margin-right: 6px;
}
.legend {
  display: flex;
  gap: 6px;
  font-size: 11px;
}
.lg {
  padding: 0 8px;
  border-radius: 10px;
  border: 1px solid var(--line);
}
.lg.hit {
  background: var(--green-soft);
  color: var(--green);
}
.lg.miss {
  color: var(--muted);
}
.lg.extra {
  background: var(--paid-soft);
  color: var(--paid);
}
.lg.violation {
  background: var(--danger-soft);
  color: var(--danger);
}
.lg.expected {
  border-style: dashed;
  color: var(--muted);
}
.lg.contra {
  outline: 2px solid var(--danger);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}
.chip {
  padding: 1px 8px;
  border-radius: 10px;
  font-size: 12px;
  background: var(--bg);
}
details summary {
  cursor: pointer;
  font-size: 12px;
  color: var(--muted);
}
pre.mono {
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 12px;
  background: var(--bg);
  padding: 8px;
  border-radius: var(--radius);
  max-height: 320px;
  overflow: auto;
  margin: 6px 0 0;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.judge {
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-top: 1px solid var(--line);
  padding-top: 10px;
}
.stars {
  display: flex;
  gap: 12px;
}
.stars label {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  cursor: pointer;
}
.row {
  display: flex;
  align-items: center;
  gap: 10px;
}
textarea {
  resize: vertical;
}
.dim {
  color: var(--muted);
}
.small {
  font-size: 11px;
}
</style>
