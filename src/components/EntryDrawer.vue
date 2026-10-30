<script setup lang="ts">
import VButton from "../ui/VButton.vue";
import { computed, nextTick, ref, watch } from "vue";
import { api, Channel, imgSrc } from "../api";
import type { Decision, EntryDetail, SyncEvent } from "../types";
import { useCodex } from "../stores/codex";
import { useToast } from "../stores/toast";
import { formatArtistPoolLine } from "../lib/artist-recipe";
import { copyText } from "../lib/clipboard";
import { DECISION_LABEL, VIA_LABEL } from "../lib/decision";
import { fmtBytes, fmtDateTime, padOrder } from "../lib/format";
import ProgressBar from "./ProgressBar.vue";

const props = defineProps<{ entryId: string | null }>();
const emit = defineEmits<{ close: []; navigate: [entryId: string] }>();

const toast = useToast();
const codex = useCodex();

const root = ref<HTMLElement | null>(null);
const detail = ref<EntryDetail | null>(null);
const loading = ref(false);
const index = ref(0);
const note = ref("");
const downloading = ref<{
  kind: "display_all" | "originals";
  ev: SyncEvent | null;
} | null>(null);
let seq = 0;

async function load() {
  const id = props.entryId;
  if (!id) {
    detail.value = null;
    return;
  }
  seq += 1;
  const my = seq;
  loading.value = true;
  try {
    const d = await api.getEntry(id);
    if (my !== seq) return;
    detail.value = d;
    index.value = 0;
    note.value = d.membership?.member_note ?? "";
    await nextTick();
    root.value?.focus();
  } catch (e) {
    if (my === seq) toast.error(e, "读取条目失败");
  } finally {
    if (my === seq) loading.value = false;
  }
}
watch(() => props.entryId, load, { immediate: true });

const images = computed(() => detail.value?.images ?? []);
const current = computed(() => images.value[index.value] ?? null);
const currentSrc = computed(() =>
  imgSrc(current.value?.local_path ?? current.value?.thumb_path ?? null),
);
const decision = computed<Decision>(
  () => detail.value?.membership?.decision ?? "undecided",
);
const artistLine = computed(() =>
  detail.value?.parse ? formatArtistPoolLine(detail.value.parse.artists) : "",
);
const missingDisplay = computed(
  () => images.value.filter((i) => !i.local_path).length,
);
const missingOriginal = computed(
  () => images.value.filter((i) => !i.original_local_path).length,
);
const totalBytes = computed(() =>
  images.value.reduce((s, i) => s + (i.bytes ?? 0), 0),
);
const downloadProgress = computed(() => {
  const ev = downloading.value?.ev;
  if (!ev || !ev.images_total) return null;
  return ev.images_done / ev.images_total;
});
const characterPrompts = computed(
  () => detail.value?.entry.character_prompts ?? [],
);

function prev() {
  if (images.value.length)
    index.value = (index.value - 1 + images.value.length) % images.value.length;
}
function next() {
  if (images.value.length)
    index.value = (index.value + 1) % images.value.length;
}

async function decide(d: Decision) {
  const id = props.entryId;
  if (!id || d === decision.value) return;
  const ok = await codex.setDecision([id], d, "click");
  if (ok) await load();
}

async function copy(text: string, what: string) {
  if (await copyText(text)) toast.success(`已复制${what}`);
  else toast.push("复制失败", "error");
}

async function saveNote() {
  const id = props.entryId;
  if (!id || !detail.value) return;
  const before = detail.value.membership?.member_note ?? "";
  if (note.value === before) return;
  try {
    await api.setMemberNote(id, note.value);
    if (detail.value.membership)
      detail.value.membership.member_note = note.value;
    toast.success("备注已保存");
  } catch (e) {
    toast.error(e, "保存备注失败");
  }
}

async function download(kind: "display_all" | "originals") {
  const id = props.entryId;
  if (!id || downloading.value) return;
  const ch = new Channel<SyncEvent>();
  downloading.value = { kind, ev: null };
  ch.onmessage = (e) => {
    downloading.value = { kind, ev: e };
    if (e.phase === "done" || e.phase === "error") {
      if (e.phase === "error") toast.error(e.message, "下载失败");
      else
        toast.success(
          `下载完成：${e.images_done}/${e.images_total}${e.images_failed ? `，失败 ${e.images_failed}` : ""}`,
        );
      downloading.value = null;
      if (props.entryId === id) void load();
    }
  };
  try {
    await api.downloadImages(id, kind, ch);
  } catch (e) {
    downloading.value = null;
    toast.error(e, "下载失败");
  }
}

function onKeydown(ev: KeyboardEvent) {
  if (
    ev.target instanceof HTMLTextAreaElement ||
    ev.target instanceof HTMLInputElement
  )
    return;
  if (ev.key === "Escape") {
    ev.preventDefault();
    emit("close");
    return;
  }
  if (ev.key === "ArrowLeft") {
    ev.preventDefault();
    prev();
    return;
  }
  if (ev.key === "ArrowRight") {
    ev.preventDefault();
    next();
    return;
  }
  if (ev.key === " ") {
    ev.preventDefault();
    emit("close");
  }
}
</script>

<template>
  <Transition name="drawer">
    <div v-if="entryId" class="drawer-wrap">
      <div class="backdrop" @click="emit('close')" />
      <aside ref="root" class="drawer" tabindex="-1" @keydown="onKeydown">
        <header class="head">
          <div class="head-title">
            <template v-if="detail">
              <span class="order">{{ padOrder(detail.entry.book_order) }}</span>
              · {{ detail.entry.title }}
            </template>
            <span v-else>{{ loading ? "加载中…" : entryId }}</span>
          </div>
          <VButton class="close" title="关闭 (Esc)" @click="emit('close')"
            >×</VButton
          >
        </header>

        <div v-if="detail" class="body">
          <!-- 决策 -->
          <section class="sec decisions">
            <VButton
              :class="{ on: decision === 'include' }"
              @click="decide('include')"
              >加入</VButton
            >
            <VButton
              :class="{ on: decision === 'exclude' }"
              @click="decide('exclude')"
              >不加入</VButton
            >
            <VButton
              :class="{ on: decision === 'undecided' }"
              @click="decide('undecided')"
              >未定</VButton
            >
            <span class="dim"
              >{{ detail.entry.rating }} · {{ detail.entry.path.join(" / ")
              }}<template v-if="detail.entry.update_batches.length">
                · 批次 {{ detail.entry.update_batches.join("/") }}</template
              ></span
            >
          </section>

          <!-- 大图轮播 -->
          <section class="sec carousel">
            <div class="stage">
              <img
                v-if="currentSrc"
                :src="currentSrc"
                :alt="detail.entry.title"
              />
              <div v-else class="placeholder">
                {{
                  !images.length
                    ? "这条没有配图"
                    : current?.error
                      ? `下载失败：${current.error}`
                      : "尚未下载展示图"
                }}
              </div>
              <VButton v-if="images.length > 1" class="nav left" @click="prev"
                >‹</VButton
              >
              <VButton v-if="images.length > 1" class="nav right" @click="next"
                >›</VButton
              >
              <span v-if="images.length" class="counter"
                >{{ index + 1 }}/{{ images.length }}</span
              >
            </div>
            <div v-if="current" class="img-meta">
              <span class="dim"
                >{{ current.image_file
                }}<template v-if="current.bytes">
                  · {{ fmtBytes(current.bytes) }}</template
                ><template v-if="current.original_local_path">
                  · 已有原图</template
                ></span
              >
              <pre v-if="current.raw_tag" class="raw">{{
                current.raw_tag
              }}</pre>
              <span v-else class="dim">（这张图没有自己的 raw_tag）</span>
            </div>
            <div class="dl-row">
              <VButton
                :disabled="!!downloading"
                @click="download('display_all')"
                >下载全部展示图<template v-if="missingDisplay"
                  >（缺 {{ missingDisplay }}）</template
                ></VButton
              >
              <VButton :disabled="!!downloading" @click="download('originals')"
                >下载原图<template v-if="missingOriginal"
                  >（缺 {{ missingOriginal }}）</template
                ></VButton
              >
              <span v-if="totalBytes" class="dim"
                >展示图合计 {{ fmtBytes(totalBytes) }}</span
              >
            </div>
            <ProgressBar
              v-if="downloading"
              :value="downloadProgress"
              :label="
                downloading.ev
                  ? `${downloading.ev.message} ${downloading.ev.images_done}/${downloading.ev.images_total}`
                  : '排队中…'
              "
            />
          </section>

          <!-- tags -->
          <section class="sec">
            <div class="sec-head">
              tags
              <VButton class="mini" @click="copy(detail.entry.tags, ' tags')"
                >复制</VButton
              >
            </div>
            <pre class="mono">{{ detail.entry.tags }}</pre>
          </section>
          <section v-if="detail.entry.negative" class="sec">
            <div class="sec-head">
              negative
              <VButton
                class="mini"
                @click="copy(detail.entry.negative!, ' negative')"
                >复制</VButton
              >
            </div>
            <pre class="mono">{{ detail.entry.negative }}</pre>
          </section>
          <section v-if="characterPrompts.length" class="sec">
            <div class="sec-head">character prompts</div>
            <div v-for="(cp, i) in characterPrompts" :key="i" class="cp">
              <div class="cp-label">{{ cp.label }}</div>
              <pre class="mono">{{ cp.prompt }}</pre>
              <pre v-if="cp.negative" class="mono neg">− {{ cp.negative }}</pre>
            </div>
          </section>

          <!-- 画师解析 -->
          <section v-if="detail.parse" class="sec">
            <div class="sec-head">
              画师解析
              <VButton
                v-if="artistLine"
                class="mini"
                @click="copy(artistLine, '画师串')"
                >复制画师串</VButton
              >
            </div>
            <div class="table-scroll" v-if="detail.parse.artists.length">
              <table class="tbl">
                <thead>
                  <tr>
                    <th>#</th>
                    <th>画师</th>
                    <th>权重</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="(a, i) in detail.parse.artists" :key="a.name">
                    <td>{{ i + 1 }}</td>
                    <td class="mono">{{ a.name }}</td>
                    <td>{{ a.weight }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
            <div v-else class="dim">没解析出画师</div>
            <div v-if="detail.parse.warnings.length" class="warn">
              {{ detail.parse.warnings.join("；") }}
            </div>
          </section>

          <!-- 同画师 -->
          <section v-if="detail.siblings.length" class="sec">
            <div class="sec-head">
              同画师的其他条目（{{ detail.siblings.length }}）
            </div>
            <ul class="siblings">
              <li v-for="s in detail.siblings" :key="s.entry_id">
                <a href="#" @click.prevent="emit('navigate', s.entry_id)">{{
                  s.title
                }}</a>
                <span class="dim">
                  · {{ s.codex_id }} · {{ DECISION_LABEL[s.decision] }}</span
                >
              </li>
            </ul>
          </section>

          <!-- 备注 -->
          <section class="sec">
            <div class="sec-head">备注</div>
            <textarea
              v-model="note"
              rows="3"
              placeholder="失焦自动保存"
              @blur="saveNote"
            />
          </section>

          <!-- 历史 -->
          <section class="sec">
            <div class="sec-head">决策历史</div>
            <div class="table-scroll" v-if="detail.audit.length">
              <table class="tbl">
                <tbody>
                  <tr v-for="a in detail.audit" :key="a.id">
                    <td class="dim">{{ fmtDateTime(a.at) }}</td>
                    <td>
                      {{
                        a.from_decision ? DECISION_LABEL[a.from_decision] : "—"
                      }}
                      → {{ DECISION_LABEL[a.to_decision] }}
                    </td>
                    <td class="dim">{{ VIA_LABEL[a.via] }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
            <div v-else class="dim">还没有决策记录</div>
          </section>

          <section class="sec dim small">
            entry_id {{ detail.entry.entry_id }} · v{{
              detail.entry.entry_version
            }}
            · 首见 {{ detail.entry.first_seen_release }} · 末见
            {{ detail.entry.last_seen_release }}
            <template v-if="detail.entry.removed_at">
              · 已从上游移除
              {{ fmtDateTime(detail.entry.removed_at) }}</template
            >
            <template v-if="detail.entry.note">
              · 上游备注：{{ detail.entry.note }}</template
            >
          </section>
        </div>
      </aside>
    </div>
  </Transition>
</template>

<style scoped>
.drawer-wrap {
  position: fixed;
  inset: 0;
  z-index: 50;
}
.backdrop {
  position: absolute;
  inset: 0;
  background: var(--backdrop);
}
.drawer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 560px;
  max-width: 90vw;
  background: var(--panel);
  border-left: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  outline: none;
  box-shadow: -8px 0 30px var(--backdrop);
}
.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 0.15s;
}
.drawer-enter-active .drawer,
.drawer-leave-active .drawer {
  transition: transform 0.18s ease-out;
}
.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}
.drawer-enter-from .drawer,
.drawer-leave-to .drawer {
  transform: translateX(40px);
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--line);
}
.head-title {
  flex: 1;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.order {
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.close {
  padding: 0 10px;
  font-size: 18px;
  line-height: 26px;
}
.body {
  flex: 1;
  overflow: auto;
  padding: 10px 14px 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.sec-head {
  font-weight: 600;
  font-size: 12px;
  color: var(--muted);
  margin-bottom: 4px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.decisions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.decisions button.on {
  background: var(--accent);
  color: var(--on-accent);
  border-color: var(--accent);
}
.stage {
  position: relative;
  background: var(--bg);
  border-radius: var(--radius);
  min-height: 240px;
  max-height: 480px;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.stage img {
  max-width: 100%;
  max-height: 480px;
  object-fit: contain;
  display: block;
}
.placeholder {
  color: var(--muted);
  font-size: 12px;
  padding: 40px;
  text-align: center;
}
.nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  padding: 0 10px;
  font-size: 22px;
  line-height: 36px;
  opacity: 0.8;
}
.nav.left {
  left: 8px;
}
.nav.right {
  right: 8px;
}
.counter {
  position: absolute;
  bottom: 6px;
  right: 8px;
  padding: 1px 8px;
  border-radius: 10px;
  background: var(--black-overlay);
  color: var(--overlay-text);
  font-size: 12px;
}
.img-meta {
  margin-top: 6px;
  font-size: 12px;
}
.raw {
  margin: 4px 0 0;
}
.dl-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  flex-wrap: wrap;
}
pre.mono,
pre.raw {
  white-space: pre-wrap;
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  background: var(--bg);
  padding: 8px;
  border-radius: var(--radius);
  margin: 0;
  max-height: 220px;
  overflow: auto;
}
pre.neg {
  opacity: 0.8;
}
.cp {
  margin-bottom: 8px;
}
.cp-label {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 2px;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 3px 6px;
  border-bottom: 1px solid var(--line);
}
.tbl .mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.siblings {
  margin: 0;
  padding-left: 18px;
  font-size: 13px;
}
.siblings a {
  color: var(--accent);
}
textarea {
  width: 100%;
  resize: vertical;
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
.warn {
  color: var(--danger);
  font-size: 12px;
  margin-top: 4px;
}
</style>
