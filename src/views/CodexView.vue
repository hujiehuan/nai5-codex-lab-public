<script setup lang="ts">
import VButton from "../ui/VButton.vue";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useRoute, useRouter } from "vue-router";
import { useCodex } from "../stores/codex";
import { usePrefs } from "../stores/prefs";
import { useToast } from "../stores/toast";
import CodexTree from "../components/CodexTree.vue";
import FilterBar from "../components/FilterBar.vue";
import Pager from "../components/Pager.vue";
import EntryGrid from "../components/EntryGrid.vue";
import EntryDrawer from "../components/EntryDrawer.vue";
import PeekOverlay from "../components/PeekOverlay.vue";
import { cardSizeKeyAction, keyToMove, moveFocus } from "../lib/grid";
import { keyToDecision } from "../lib/decision";
import { createPeekTracker, PEEK_HOLD_MS } from "../lib/peek";
import {
  ARTIST_SOURCES,
  CODEX_ARTIST_ID,
  isArtistBook,
  NAX_ARTIST_ID,
  NAX_ARTIST_URL,
} from "../lib/artist-sources";
import {
  isSuozhangBook,
  SUOZHANG_SITE_URL,
  SUOZHANG_SOURCES,
} from "../lib/codex-sources";
import type { Decision, EntryCard as EntryCardT } from "../types";

const props = defineProps<{ codexId?: string }>();
const route = useRoute();
const router = useRouter();
const codex = useCodex();
const prefs = usePrefs();
const toast = useToast();
const grid = ref<InstanceType<typeof EntryGrid> | null>(null);
const DEFAULT_ID = CODEX_ARTIST_ID;
const artistPage = computed(() => isArtistBook(props.codexId ?? DEFAULT_ID));
const naxPage = computed(() => props.codexId === NAX_ARTIST_ID);
const suozhangPage = computed(() => isSuozhangBook(props.codexId));

async function openExternal(url: string, label: string) {
  try {
    if (import.meta.env.MODE === "mock")
      window.open(url, "_blank", "noopener,noreferrer");
    else await openUrl(url);
  } catch (e) {
    toast.error(e, `打开${label}失败`);
  }
}
const openNax = () => openExternal(NAX_ARTIST_URL, " NAX 网站");
const openSuozhangSite = () => openExternal(SUOZHANG_SITE_URL, "法典图鉴");

// 防重入但不丢请求：正在进书时又被叫（快速切书、横幅的 query 变化）只记下最新序号，
// 这一轮跑完发现序号变了就按当时的 props/route 再跑一次，保证最终停在 URL 指向的那本书。
let entering = false;
let enterSeq = 0;
async function enter() {
  enterSeq += 1;
  if (entering) return;
  entering = true;
  try {
    let seen = enterSeq;
    await enterInner();
    while (seen !== enterSeq) {
      seen = enterSeq;
      await enterInner();
    }
  } finally {
    entering = false;
  }
}
async function enterInner() {
  if (!props.codexId) {
    await router.replace(`/codex/${DEFAULT_ID}`);
    return;
  }
  const firstSeen =
    typeof route.query.first_seen === "string" ? route.query.first_seen : null;
  await codex.open(props.codexId, { firstSeen });
  if (firstSeen) await router.replace({ path: route.path });
}
onMounted(enter);
watch(
  () => props.codexId,
  () => {
    peek.cancel();
    stopPeek();
    void enter();
  },
);
// 同步页横幅跳到当前这本书时 codexId 不变，只有 query 变
watch(
  () => route.query.first_seen,
  (v) => {
    if (typeof v === "string" && v) void enter();
  },
);

// ---- 按住看大图（v0.1.3）----
// 状态机在 lib/peek.ts；这里只管计时器、window 监听与覆盖层。按下时才挂 window 监听，结束即拆。
// 覆盖层 pointer-events: none，所以 pointerup 不管落在哪都能到 window。
const peek = createPeekTracker();
const peekEntry = ref<EntryCardT | null>(null);
let peekIndex = -1;
let peekTimer: ReturnType<typeof setTimeout> | null = null;
let peekListening = false;

function onImagePointerdown(index: number, ev: PointerEvent) {
  stopPeek();
  peek.down(ev.clientX, ev.clientY, performance.now(), ev);
  if (peek.phase !== "pending") return; // 右键 / 修饰键：不计时，原有行为
  peekIndex = index;
  peekTimer = setTimeout(onPeekTimer, PEEK_HOLD_MS);
  listenPeek(true);
}
function onPeekTimer() {
  peekTimer = null;
  if (peek.tick(performance.now()) !== "peek") return;
  const entry = codex.items[peekIndex] ?? null;
  if (!entry) {
    peek.cancel();
    stopPeek();
    return;
  }
  peekEntry.value = entry;
}
function onPeekMove(ev: PointerEvent) {
  peek.move(ev.clientX, ev.clientY);
  if (peek.phase === "idle") stopPeek(); // 移出 6 px：取消计时，随后的 click 按原逻辑（三态循环）
}
function onPeekUp() {
  peek.up(performance.now()); // peeking → armed，紧随的 click 在 onImageClick / onCardClick 里吞掉
  stopPeek();
}
function onPeekCancel() {
  peek.cancel();
  stopPeek();
}
/** peek 期间 Escape：关覆盖层；按键可能还没松，随后的 up / click 由 tracker 的 armed 吞掉。捕获阶段拦下，网格的 Escape（清选择）不再响应 */
function onPeekKeydown(ev: KeyboardEvent) {
  if (ev.key !== "Escape" || peek.phase !== "peeking") return;
  ev.preventDefault();
  ev.stopPropagation();
  onPeekCancel();
}
/** armed 期间任何位置的下一次按下（含卡片文字区、空白处）都清掉欠账，免得误吞那次正常点击 */
function onAnyPointerdown() {
  peek.disarm();
}
function listenPeek(on: boolean) {
  if (on === peekListening) return;
  peekListening = on;
  if (on) {
    window.addEventListener("pointermove", onPeekMove);
    window.addEventListener("pointerup", onPeekUp);
    window.addEventListener("pointercancel", onPeekCancel);
    window.addEventListener("blur", onPeekCancel);
    window.addEventListener("keydown", onPeekKeydown, true);
  } else {
    window.removeEventListener("pointermove", onPeekMove);
    window.removeEventListener("pointerup", onPeekUp);
    window.removeEventListener("pointercancel", onPeekCancel);
    window.removeEventListener("blur", onPeekCancel);
    window.removeEventListener("keydown", onPeekKeydown, true);
  }
}
function stopPeek() {
  if (peekTimer !== null) {
    clearTimeout(peekTimer);
    peekTimer = null;
  }
  listenPeek(false);
  peekIndex = -1;
  peekEntry.value = null;
  window.removeEventListener("pointerdown", onAnyPointerdown, true);
  if (peek.armed)
    window.addEventListener("pointerdown", onAnyPointerdown, {
      capture: true,
      once: true,
    });
}
onBeforeUnmount(() => {
  peek.cancel();
  stopPeek();
  window.removeEventListener("pointerdown", onAnyPointerdown, true);
});

function onImageClick(index: number, ev: MouseEvent) {
  if (peek.consumeClick() === "suppress-click") return; // 刚结束的 peek 紧随而来的 click：不切三态
  const entry = codex.items[index];
  if (!entry) return;
  codex.setFocus(index);
  if (ev.shiftKey) {
    codex.selectRange(index);
    return;
  }
  if (ev.metaKey || ev.ctrlKey) {
    codex.toggleSelect(index);
    return;
  }
  codex.setAnchor(index);
  if (ev.altKey) {
    void codex.setDecision([entry.entry_id], "exclude", "click");
    return;
  }
  void codex.cycle(entry.entry_id, "click");
}

function onCardClick(index: number, ev: MouseEvent) {
  if (peek.consumeClick() === "suppress-click") return; // 松在同一张卡的文字区：click 落到 .card
  codex.setFocus(index);
  if (ev.shiftKey) codex.selectRange(index);
  else if (ev.metaKey || ev.ctrlKey) codex.toggleSelect(index);
  else codex.setAnchor(index);
}

function onContext(index: number) {
  const entry = codex.items[index];
  if (!entry) return;
  codex.setFocus(index);
  codex.openDrawer(entry.entry_id);
}

function onKeydown(ev: KeyboardEvent) {
  if (codex.codexId !== props.codexId) return;
  const total = codex.items.length;
  const cols = grid.value?.getColumns() ?? 1;
  const cmd = ev.metaKey || ev.ctrlKey;

  if (cmd && ev.key.toLowerCase() === "z") {
    ev.preventDefault();
    void codex.undo();
    return;
  }
  if (cmd && ev.key.toLowerCase() === "a") {
    ev.preventDefault();
    codex.selectAll();
    return;
  }
  // ⌘←/→ 翻页；不带修饰键的 ←/→ 仍是移动焦点
  if (cmd && ev.key === "ArrowLeft") {
    ev.preventDefault();
    codex.prevPage();
    return;
  }
  if (cmd && ev.key === "ArrowRight") {
    ev.preventDefault();
    codex.nextPage();
    return;
  }
  // ⌘⇧= / ⌘⇧- 卡片放大 / 缩小一档（带 ⇧，不抢整页缩放的 ⌘= / ⌘-）；stopPropagation 免得外层缩放监听也响应
  const sizeAction = cardSizeKeyAction(ev);
  if (sizeAction) {
    ev.preventDefault();
    ev.stopPropagation();
    if (sizeAction === "bigger") prefs.biggerCard();
    else prefs.smallerCard();
    return;
  }
  if (cmd || ev.altKey) return;

  const mv = keyToMove(ev.key);
  if (mv) {
    ev.preventDefault();
    codex.setFocus(moveFocus(codex.focus, mv, cols, total));
    if (ev.shiftKey && codex.focus !== null) codex.selectRange(codex.focus);
    return;
  }
  const dec = keyToDecision(ev.key);
  if (dec) {
    ev.preventDefault();
    const ids = codex.selectedCount
      ? codex.selectedIds()
      : codex.focusedEntry
        ? [codex.focusedEntry.entry_id]
        : [];
    void codex.setDecision(ids, dec, "key");
    return;
  }
  if (ev.key === " ") {
    ev.preventDefault();
    if (codex.focusedEntry) codex.toggleDrawer(codex.focusedEntry.entry_id);
    return;
  }
  if (ev.key === "Enter") {
    ev.preventDefault();
    if (codex.focusedEntry) codex.openDrawer(codex.focusedEntry.entry_id);
    return;
  }
  if (ev.key === "Escape") {
    if (codex.drawerEntryId) codex.closeDrawer();
    else codex.clearSelection();
  }
}

/** 分页条 / 底栏按钮 / 抽屉关闭后把焦点交还网格，否则 ⌘←/→、A/X/U 等快捷键得再点一下网格才响应 */
function focusGrid() {
  grid.value?.focusContainer();
}

async function batch(d: Decision) {
  focusGrid();
  await codex.setDecision(codex.selectedIds(), d, "batch");
}

function onBatchClear() {
  codex.clearSelection();
  focusGrid();
}

function onTreeSelect(path: string[]) {
  const same =
    codex.filter.path_prefix.length === path.length &&
    path.every((p, i) => p === codex.filter.path_prefix[i]);
  codex.setFilter({ path_prefix: same ? [] : path });
}

function onDrawerClose() {
  codex.closeDrawer();
  focusGrid();
}
</script>

<template>
  <div class="codex">
    <aside class="tree-pane">
      <div v-if="codex.booksError" class="read-error" role="alert">
        书目读取失败：{{ codex.booksError
        }}<VButton @click="codex.loadBooks">重试</VButton>
      </div>
      <div v-else-if="codex.book" class="book-head">
        <div class="book-title">{{ codex.book.title }}</div>
        <div class="book-meta">
          {{ naxPage ? codex.book.version : `v${codex.book.version}`
          }}<template v-if="codex.book.author">
            · {{ codex.book.author }}</template
          >
          · {{ codex.book.entry_count }} 条
        </div>
        <div class="book-meta">
          加入 {{ codex.book.counts.include }} · 不加入
          {{ codex.book.counts.exclude }} · 未定
          {{ codex.book.counts.undecided }}
        </div>
      </div>
      <div v-else class="book-head book-meta">
        {{ codex.booksLoading ? "读取书目…" : "尚无数据，请先同步" }}
      </div>
      <VButton
        class="tree-root"
        :class="{ active: !codex.filter.path_prefix.length }"
        @click="codex.setFilter({ path_prefix: [] })"
        >全部目录</VButton
      >
      <div class="tree-scroll">
        <CodexTree
          :nodes="codex.book?.tree ?? []"
          :prefix="[]"
          :active="codex.filter.path_prefix"
          @select="onTreeSelect"
        />
      </div>
    </aside>

    <section class="main">
      <header v-if="artistPage" class="artist-sources">
        <div class="source-heading">
          <h1>画师词典</h1>
          <nav class="source-tabs" aria-label="画师来源">
            <RouterLink
              v-for="source in ARTIST_SOURCES"
              :key="source.id"
              :to="`/codex/${source.id}`"
              class="source-tab"
              :aria-current="props.codexId === source.id ? 'page' : undefined"
              >{{ source.label }}</RouterLink
            >
          </nav>
          <VButton v-if="naxPage" class="source-link" @click="openNax"
            >打开 NAX 网站 ↗</VButton
          >
        </div>
        <p class="source-description">
          {{
            naxPage
              ? "NAX V5 Full 单画师测试图，可预览、筛选并复制画师标签。首次同步图片较多，之后会检查更新并补下载。"
              : "法典画师词条与示例图，可预览、筛选并复制画师串。"
          }}
          两个来源随图库一起同步。
        </p>
        <p
          v-if="
            naxPage && !codex.book && !codex.booksLoading && !codex.booksError
          "
          class="source-empty"
        >
          尚未同步 NAX 画师库。<RouterLink to="/sync">去同步图库 →</RouterLink>
        </p>
      </header>
      <header v-else-if="suozhangPage" class="artist-sources">
        <div class="source-heading">
          <h1>所长法典</h1>
          <nav class="source-tabs" aria-label="法典分册">
            <RouterLink
              v-for="source in SUOZHANG_SOURCES"
              :key="source.id"
              :to="`/codex/${source.id}`"
              class="source-tab"
              :aria-current="props.codexId === source.id ? 'page' : undefined"
              >{{ source.label }}</RouterLink
            >
          </nav>
          <VButton class="source-link" @click="openSuozhangSite"
            >打开法典图鉴 ↗</VButton
          >
        </div>
        <p class="source-description">
          戒红所整理的 N5 法典：按用途分类的场景、服装、动作配方与示例图，随图库一起同步。
          这里可以浏览、复制和标记；标记只留在本机，不会推送到插件参考池。
        </p>
        <p
          v-if="!codex.book && !codex.booksLoading && !codex.booksError"
          class="source-empty"
        >
          还没有同步这本法典。<RouterLink to="/sync">去同步图库 →</RouterLink>
        </p>
      </header>
      <div v-if="codex.listError" class="read-error" role="alert">
        条目读取失败：{{ codex.listError
        }}<VButton @click="codex.reload()">重试</VButton>
      </div>
      <FilterBar v-else>
        <Pager @navigate="focusGrid" />
      </FilterBar>
      <EntryGrid
        ref="grid"
        :items="codex.codexId === props.codexId ? codex.items : []"
        :selected="codex.selected"
        :focus="codex.focus"
        :loading="codex.loading"
        :error="codex.listError"
        :reset-key="codex.listVersion"
        :card-size="prefs.cardSize"
        @image-click="onImageClick"
        @image-pointerdown="onImagePointerdown"
        @card-click="onCardClick"
        @context="onContext"
        @keydown="onKeydown"
      />
      <div v-if="codex.selectedCount" class="batchbar">
        <span>已选 {{ codex.selectedCount }} 条</span>
        <VButton class="primary" @click="batch('include')">全部加入</VButton>
        <VButton @click="batch('exclude')">全部不加入</VButton>
        <VButton @click="batch('undecided')">全部未定</VButton>
        <VButton @click="onBatchClear()">取消选择</VButton>
      </div>
      <div v-else class="hint">
        点击图片循环三态 · ⇧ 连选 · ⌘ 加选 · ⌥ 直接不加入 · 右键详情 ·
        方向键/HJKL 移动 · A 加入 X 不加入 U 未定 · 空格预览 · ⌘Z 撤销 · ⌘A 全选
        · ⌘←/→ 翻页 · ⌘⇧= / ⌘⇧- 卡片大小 · 按住左键看大图
      </div>
    </section>

    <EntryDrawer
      :entry-id="codex.drawerEntryId"
      @close="onDrawerClose"
      @navigate="codex.openDrawer($event)"
    />
    <PeekOverlay :entry="peekEntry" />
  </div>
</template>

<style scoped>
.codex {
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  height: 100%;
  min-height: 0;
}
.tree-pane {
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--line);
  background: var(--panel);
  min-height: 0;
}
.book-head {
  padding: 12px 12px 8px;
  border-bottom: 1px solid var(--line);
}
.book-title {
  font-weight: 600;
}
.book-meta {
  font-size: 12px;
  color: var(--muted);
  margin-top: 2px;
}
.tree-root {
  margin: 8px 8px 2px;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
}
.tree-root:hover {
  background: var(--bg);
}
.tree-root.active {
  background: var(--accent);
  color: var(--on-accent);
}
.tree-scroll {
  flex: 1;
  overflow: auto;
  padding: 0 8px 12px;
}
.main {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
}
.artist-sources {
  padding: 12px 16px 10px;
  border-bottom: 1px solid var(--line);
  background: var(--panel);
}
.source-heading {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px 16px;
}
.source-heading h1 {
  font-size: 16px;
  line-height: 1.5;
  margin: 0;
}
.source-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.source-tab {
  display: inline-flex;
  align-items: center;
  min-height: 30px;
  padding: 4px 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
  color: var(--ink);
  font-size: 13px;
  text-decoration: none;
}
.source-tab[aria-current="page"] {
  color: var(--on-accent);
  background: var(--accent);
  border-color: var(--accent);
}
.source-tab:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.source-description,
.source-empty {
  margin: 6px 0 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--muted);
}
.source-link {
  margin-left: auto;
}
.source-empty a {
  color: var(--accent);
}
.batchbar,
.hint {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  border-top: 1px solid var(--line);
  background: var(--panel);
  font-size: 12px;
}
.hint {
  color: var(--muted);
}
.batchbar {
  flex-wrap: wrap;
}
.hint {
  line-height: 1.8;
  flex-wrap: wrap;
}
@media (max-width: 900px) {
  .codex {
    grid-template-columns: 170px minmax(0, 1fr);
  }
}
@media (max-width: 700px) {
  .codex {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(500px, 1fr);
    height: auto;
    min-height: 100%;
  }
  .tree-pane {
    max-height: 220px;
  }
  .main {
    min-height: 650px;
  }
  .tree-scroll {
    max-height: 100px;
  }
}
</style>
