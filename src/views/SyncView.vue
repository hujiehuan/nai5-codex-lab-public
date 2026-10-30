<script setup lang="ts">
import AsyncState from "../ui/AsyncState.vue";
import VCard from "../ui/VCard.vue";
import VCost from "../ui/VCost.vue";
import VButton from "../ui/VButton.vue";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { PHASE_LABEL, useSync } from "../stores/sync";
import ProgressBar from "../components/ProgressBar.vue";
import IndexTiming from "../components/IndexTiming.vue";
import { fmtDateTime } from "../lib/format";
import { embedLabel, embedRatio } from "../lib/style";
import { NAX_ARTIST_ID } from "../lib/artist-sources";
import type { SyncLog } from "../types";

const sync = useSync();
const expanded = ref<Set<number>>(new Set());
let timer: number | null = null;

onMounted(async () => {
  await sync.refresh();
  // 只要在跑就轮询：本会话没有 Channel（同步是上次会话或别处发起的）时靠它更新进度；
  // 有 Channel 时也轮询，终止事件丢了能靠后端状态把按钮恢复过来（见 stores/sync.ts refresh）
  timer = window.setInterval(() => {
    if (sync.running) void sync.refresh();
  }, 2000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

const phaseText = computed(() => {
  const p = sync.channelActive ? sync.progress?.phase : sync.status?.phase;
  if (p === "done" && live.value?.message.includes("同步失败"))
    return "部分完成";
  return PHASE_LABEL[p ?? "idle"];
});
const live = computed(() => (sync.channelActive ? sync.progress : sync.status));
const imageProgress = computed(() => {
  const s = live.value;
  if (!s || !s.images_total) return null;
  return s.images_done / s.images_total;
});
const showProgress = computed(
  () =>
    sync.running || (sync.progress !== null && sync.progress.phase !== "done"),
);
const partialDone = computed(
  () => !!sync.lastDone?.message.includes("同步失败"),
);
const sourceFailure = computed(
  () => !!live.value?.message.includes("同步失败"),
);
// ---- v0.3.0（§12.2）embedding 阶段：进度看 embed_done / embed_total（只有 Channel 事件带；轮询到的 SyncStatus 没有 → 跑马灯）----
const embedding = computed(() => live.value?.phase === "embedding");
const embedEvent = computed(() => (sync.channelActive ? sync.progress : null));
const barValue = computed<number | null>(() => {
  if (!sync.running) return live.value?.phase === "error" ? 0 : 1;
  return embedding.value ? embedRatio(embedEvent.value) : imageProgress.value;
});
const barLabel = computed(() => {
  const s = live.value;
  if (embedding.value) return embedLabel(embedEvent.value);
  if (s?.images_total)
    return `图片 ${s.images_done} / ${s.images_total}${s.images_failed ? `，失败 ${s.images_failed}` : ""}`;
  return sync.running ? "准备中…" : "";
});

const KIND_LABEL: Record<SyncLog["kind"], string> = {
  full: "全量",
  resume: "续传",
  noop: "无变化",
  partial: "部分失败",
  failed: "失败",
};

function toggle(id: number) {
  const s = new Set(expanded.value);
  if (s.has(id)) s.delete(id);
  else s.add(id);
  expanded.value = s;
}
</script>

<template>
  <div class="page workspace-page">
    <header class="page-heading">
      <div>
        <p class="eyebrow">NAI5 · WORKSPACE</p>
        <h1>同步图库</h1>
        <p>
          同步法典画师、精选图包与 NAX V5
          画师测试库的条目和图片，并补齐当前模型的画风索引。
        </p>
      </div>
      <VCost kind="local" />
    </header>
    <AsyncState
      :loading="sync.loading && !sync.status"
      :error="sync.readError"
      @retry="sync.refresh"
      ><header class="top">
        <div class="releases">
          <div>
            <span class="k">上游 release</span>
            <span class="mono">{{ sync.status?.release_remote ?? "—" }}</span>
          </div>
          <div>
            <span class="k">本地 release</span>
            <span class="mono">{{
              sync.status?.release_local ?? "未同步"
            }}</span>
          </div>
        </div>
        <div class="actions">
          <VButton
            class="primary"
            :disabled="sync.running || sync.starting"
            @click="sync.start(false)"
            >同步</VButton
          >
          <VButton
            :disabled="sync.running || sync.starting"
            title="忽略本地缓存，重新拉取清单与全部条目；已落地的决策不受影响"
            @click="sync.start(true)"
            >强制重拉</VButton
          >
          <VButton v-if="sync.running" @click="sync.cancel()">取消</VButton>
          <VButton :disabled="sync.running" @click="sync.refresh()"
            >刷新</VButton
          >
        </div>
      </header>

      <VCard
        flush
        v-if="showProgress || (sourceFailure && !sync.lastDone)"
        class="card progress"
      >
        <div class="row">
          <span
            class="phase"
            :class="{ danger: live?.phase === 'error' || sourceFailure }"
            >{{ phaseText }}</span
          >
          <span class="msg">{{ live?.message }}</span>
        </div>
        <ProgressBar
          :value="barValue"
          :danger="live?.phase === 'error'"
          :label="barLabel"
        />
        <IndexTiming :timing="sync.indexTiming" />
        <div
          v-if="sync.progress && (sync.progress.added || sync.progress.changed)"
          class="dim"
        >
          新增 {{ sync.progress.added }} · 变更 {{ sync.progress.changed }}
        </div>
      </VCard>

      <VCard
        flush
        v-if="sync.lastDone"
        class="card banner"
        :class="{ partial: partialDone }"
      >
        <div>
          <span v-if="partialDone" class="danger">同步部分完成 · </span>
          本次新增 <b>{{ sync.lastDone.added }}</b> 条、变更
          <b>{{ sync.lastDone.changed }}</b> 条
          <template v-if="sync.lastDone.release">
            · release
            <span class="mono">{{ sync.lastDone.release }}</span></template
          >
          <template v-if="sync.lastDone.images_failed">
            · 图片失败 {{ sync.lastDone.images_failed }}</template
          >
          <template v-if="sync.lastDone.embed_total">
            · {{ embedLabel(sync.lastDone) }}</template
          >
          <p
            v-if="sync.lastDone.message"
            class="done-message"
            :class="{ danger: partialDone }"
          >
            {{ sync.lastDone.message }}
          </p>
        </div>
        <div class="banner-links">
          <RouterLink
            :to="{
              path: '/codex/artist_nai5_personal',
              query: sync.lastDone.release
                ? { first_seen: sync.lastDone.release }
                : {},
            }"
            >去筛选画师词典 →</RouterLink
          >
          <RouterLink :to="`/codex/${NAX_ARTIST_ID}`"
            >去筛选 NAX 画师 →</RouterLink
          >
          <RouterLink
            :to="{
              path: '/codex/nai5_community_pack',
              query: sync.lastDone.release
                ? { first_seen: sync.lastDone.release }
                : {},
            }"
            >去筛选精选图包 →</RouterLink
          >
          <RouterLink
            :to="{
              path: '/codex/suozhang_nai5',
              query: sync.lastDone.release
                ? { first_seen: sync.lastDone.release }
                : {},
            }"
            >去看所长法典 →</RouterLink
          >
          <VButton class="mini" @click="sync.dismissBanner()">关闭</VButton>
        </div>
      </VCard>

      <VCard flush class="card">
        <div class="sec-head">最近 {{ sync.logs.length }} 次同步</div>
        <div class="table-scroll" v-if="sync.logs.length">
          <table class="tbl">
            <thead>
              <tr>
                <th>开始</th>
                <th>结束</th>
                <th>类型</th>
                <th>release</th>
                <th>新增</th>
                <th>变更</th>
                <th>图片 OK</th>
                <th>图片失败</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <template v-for="l in sync.logs" :key="l.id">
                <tr
                  :class="{ bad: l.kind === 'failed' || l.kind === 'partial' }"
                >
                  <td>{{ fmtDateTime(l.started_at) }}</td>
                  <td>
                    {{ l.finished_at ? fmtDateTime(l.finished_at) : "进行中" }}
                  </td>
                  <td>{{ KIND_LABEL[l.kind] }}</td>
                  <td class="mono">{{ l.release_id ?? "—" }}</td>
                  <td>{{ l.added }}</td>
                  <td>{{ l.changed }}</td>
                  <td>{{ l.images_ok }}</td>
                  <td>{{ l.images_failed }}</td>
                  <td>
                    <VButton
                      v-if="l.failures.length"
                      class="mini"
                      @click="toggle(l.id)"
                      >{{
                        expanded.has(l.id)
                          ? "收起"
                          : `失败清单 (${l.failures.length})`
                      }}</VButton
                    >
                  </td>
                </tr>
                <tr v-if="expanded.has(l.id)" class="detail">
                  <td colspan="9">
                    <ul class="failures">
                      <li v-for="(f, i) in l.failures" :key="i">
                        <span class="mono">{{ f.url }}</span> —
                        <span v-if="f.code" class="code mono">{{ f.code }}</span
                        >{{ f.error }}
                      </li>
                    </ul>
                  </td>
                </tr>
              </template>
            </tbody>
          </table>
        </div>
        <div v-else class="dim">还没有同步记录</div>
      </VCard></AsyncState
    >
  </div>
</template>

<style scoped>
.page {
  max-width: 1400px;
}
.top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}
.releases {
  display: flex;
  gap: 24px;
  font-size: 13px;
}
.k {
  color: var(--muted);
  margin-right: 6px;
}
.actions {
  display: flex;
  gap: 8px;
}
.card {
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 18px;
  box-shadow: var(--shadow);
}
.progress .row {
  display: flex;
  gap: 10px;
  align-items: baseline;
  margin-bottom: 8px;
}
.phase {
  font-weight: 600;
}
.danger {
  color: var(--danger);
}
.msg {
  color: var(--muted);
  font-size: 13px;
}
.banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-color: var(--green);
  flex-wrap: wrap;
}
.banner.partial {
  border-color: var(--danger);
}
.done-message {
  margin: 6px 0 0;
  font-size: 12px;
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.banner-links {
  display: flex;
  gap: 14px;
  align-items: center;
}
.banner-links a {
  color: var(--accent);
  font-weight: 600;
  text-decoration: none;
}
.sec-head {
  font-weight: 600;
  margin-bottom: 8px;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 5px 6px;
  border-bottom: 1px solid var(--line);
  white-space: nowrap;
}
.tbl th {
  color: var(--muted);
  font-weight: 500;
}
.tbl tr.bad td {
  color: var(--danger);
}
.tbl tr.detail td {
  white-space: normal;
  background: var(--bg);
}
.failures {
  margin: 0;
  padding-left: 18px;
}
.failures .code {
  margin-right: 6px;
  padding: 0 5px;
  border: 1px solid var(--line);
  border-radius: 4px;
  font-size: 11px;
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
  font-size: 12px;
}
</style>

<style scoped>
.actions,
.releases,
.banner-links,
.secret,
.inline {
  flex-wrap: wrap;
}
.card {
  min-width: 0;
}
.sec-head {
  font-size: 15px;
}
.tbl td {
  max-width: 600px;
  overflow-wrap: anywhere;
}
.pool {
  min-width: 0;
}
.form input {
  max-width: 100%;
}
.releases {
  overflow-wrap: anywhere;
}
.pool-row {
  min-width: 0;
}
</style>
