<script setup lang="ts">
import VButton from "../ui/VButton.vue";
// 分页条：第 N / M 页 · 共 total 条、上一页 / 下一页、页码输入（回车跳转，越界钳制）、每页条数、排序键与方向。
// 所有改动走 store（setPage / setPageSize / setSort / toggleSortDir）；动作完成后 emit navigate，
// 由 CodexView 把焦点交还网格，这样 ⌘←/→ 与 A/X/U 等快捷键不用再点一下网格。
// v0.1.3：右端多了"卡片 小/中/大/特大"四档（全局偏好，prefs store），快捷键 ⌘⇧= / ⌘⇧-。
import { computed, ref, watch } from "vue";
import { useCodex } from "../stores/codex";
import { usePrefs } from "../stores/prefs";
import {
  ALL_SORTS,
  PAGE_SIZES,
  SORT_LABEL,
  defaultSortDir,
  isPageSize,
  type SortKey,
} from "../lib/query";
import {
  CARD_SIZES,
  CARD_SIZE_LABEL,
  cardMetrics,
  type CardSize,
} from "../lib/grid";

const codex = useCodex();
const prefs = usePrefs();
const emit = defineEmits<{ navigate: [] }>();

const pageInput = ref(String(codex.filter.page));
watch(
  () => codex.filter.page,
  (p) => {
    pageInput.value = String(p);
  },
);

function done() {
  emit("navigate");
}

/** 回车跳页：非数字忽略，越界由 store 的 clampPage 钳到 [1, pageCount]。 */
function jump() {
  const n = Number.parseInt(pageInput.value, 10);
  if (Number.isFinite(n)) codex.setPage(n);
  pageInput.value = String(codex.filter.page);
  done();
}
function revert() {
  pageInput.value = String(codex.filter.page);
}

const pageSize = computed({
  get: () => codex.filter.page_size,
  set: (v: number) => {
    if (isPageSize(v)) codex.setPageSize(v);
    done();
  },
});
const sort = computed({
  get: () => codex.filter.sort,
  set: (v: SortKey) => {
    codex.setSort(v);
    done();
  },
});

const DIR_LABEL = { asc: "升序", desc: "降序" } as const;
const dirTitle = computed(() => {
  const cur = DIR_LABEL[codex.sortDir];
  const dflt = DIR_LABEL[defaultSortDir(codex.filter.sort)];
  return codex.sortDirIsDefault
    ? `当前${cur}（${SORT_LABEL[codex.filter.sort]}的缺省方向），点击切换`
    : `当前${cur}（缺省为${dflt}），点击切换`;
});

function prev() {
  codex.prevPage();
  done();
}
function next() {
  codex.nextPage();
  done();
}
function flip() {
  codex.toggleSortDir();
  done();
}

function pickSize(size: CardSize) {
  prefs.setCardSize(size);
  done();
}
function sizeTitle(size: CardSize): string {
  const m = cardMetrics(size);
  const img = size === "l" || size === "xl" ? "展示图" : "缩略图";
  return `${CARD_SIZE_LABEL[size]}：${m.cardWidth} px 宽，用${img}（⌘⇧= 放大 / ⌘⇧- 缩小）`;
}
</script>

<template>
  <div class="pager">
    <span class="pos"
      >第 {{ codex.filter.page }} / {{ codex.pageCount }} 页 · 共
      {{ codex.total }} 条</span
    >
    <VButton
      :disabled="codex.filter.page <= 1"
      title="上一页（⌘←）"
      @click="prev"
      >‹ 上一页</VButton
    >
    <input
      v-model="pageInput"
      class="page"
      type="text"
      inputmode="numeric"
      :title="`跳到第几页（1–${codex.pageCount}），回车跳转`"
      @keydown.enter.prevent="jump"
      @blur="revert"
    />
    <VButton
      :disabled="codex.filter.page >= codex.pageCount"
      title="下一页（⌘→）"
      @click="next"
      >下一页 ›</VButton
    >
    <label
      >每页
      <select v-model="pageSize" title="每页条数">
        <option v-for="s in PAGE_SIZES" :key="s" :value="s">{{ s }}</option>
      </select>
    </label>
    <label
      >排序
      <select v-model="sort" title="排序">
        <option v-for="s in ALL_SORTS" :key="s" :value="s">
          {{ SORT_LABEL[s] }}
        </option>
      </select>
    </label>
    <VButton class="dir" :title="dirTitle" @click="flip">
      {{ codex.sortDir === "asc" ? "↑ 升序" : "↓ 降序"
      }}<span v-if="codex.sortDirIsDefault" class="dim">（缺省）</span>
    </VButton>
    <span class="sizes" role="group" aria-label="卡片大小"
      >卡片
      <VButton
        v-for="s in CARD_SIZES"
        :key="s"
        class="size"
        :class="{ active: prefs.cardSize === s }"
        :aria-pressed="prefs.cardSize === s"
        :title="sizeTitle(s)"
        @click="pickSize(s)"
        >{{ CARD_SIZE_LABEL[s] }}</VButton
      >
    </span>
  </div>
</template>

<style scoped>
.pager {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 8px;
  font-size: 12px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.pos {
  white-space: nowrap;
}
button,
select,
input {
  padding: 3px 8px;
  font-size: 12px;
}
.page {
  width: 52px;
  text-align: center;
}
label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}
.dir {
  min-width: 64px;
}
.dim {
  opacity: 0.7;
}
.sizes {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}
.sizes .size {
  padding: 3px 7px;
  min-width: 30px;
}
.sizes .size + .size {
  margin-left: -1px;
}
.sizes .size.active {
  background: var(--accent);
  color: var(--on-accent);
  border-color: var(--accent);
}
</style>
