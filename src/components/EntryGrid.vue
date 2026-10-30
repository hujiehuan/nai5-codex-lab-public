<script setup lang="ts">
// 窗口化渲染的卡片网格：只渲染可视行 ±2 行。一页最多 200 张，换页由 store 显式分页（v0.1.2），不再滚到底加载。
// 卡片尺寸由 cardSize 档位派生（v0.1.3）：所有布局数字来自 cardMetrics(cardSize)，换档位时按焦点索引重定位滚动。
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import type { EntryCard as EntryCardT } from "../types";
import EntryCard from "./EntryCard.vue";
import {
  type CardSize,
  cardMetrics,
  cellPosition,
  computeColumns,
  firstVisibleIndex,
  gridHeight,
  rowCount,
  scrollTopAfterResize,
  scrollTopForRow,
  visibleRowRange,
} from "../lib/grid";

const props = defineProps<{
  items: EntryCardT[];
  selected: Set<string>;
  focus: number | null;
  loading: boolean;
  error?: string;
  /** 列表整体换掉（换页 / 换筛选）时 +1，网格据此回顶。 */
  resetKey: number;
  /** 卡片档位 s / m / l / xl（全局偏好，prefs store） */
  cardSize: CardSize;
}>();
const emit = defineEmits<{
  imageClick: [index: number, ev: MouseEvent];
  imagePointerdown: [index: number, ev: PointerEvent];
  cardClick: [index: number, ev: MouseEvent];
  context: [index: number];
  keydown: [ev: KeyboardEvent];
}>();

const container = ref<HTMLElement | null>(null);
const width = ref(0);
const height = ref(0);
const scrollTop = ref(0);
let observer: ResizeObserver | null = null;

const metrics = computed(() => cardMetrics(props.cardSize));
const columns = computed(() => computeColumns(width.value, metrics.value));
const rows = computed(() => rowCount(props.items.length, columns.value));
const range = computed(() =>
  visibleRowRange(scrollTop.value, height.value, metrics.value, rows.value),
);
const totalHeight = computed(() => gridHeight(rows.value, metrics.value));

const visible = computed(() => {
  const out: { index: number; entry: EntryCardT; top: number; left: number }[] =
    [];
  const cols = columns.value;
  const m = metrics.value;
  const startIdx = range.value.start * cols;
  const endIdx = Math.min(props.items.length, range.value.end * cols);
  for (let i = startIdx; i < endIdx; i += 1) {
    const pos = cellPosition(i, cols, m);
    out.push({ index: i, entry: props.items[i], top: pos.top, left: pos.left });
  }
  return out;
});

function onScroll() {
  scrollTop.value = container.value?.scrollTop ?? 0;
}

function measure() {
  const el = container.value;
  if (!el) return;
  width.value = el.clientWidth;
  height.value = el.clientHeight;
}

onMounted(() => {
  measure();
  if (typeof ResizeObserver !== "undefined" && container.value) {
    observer = new ResizeObserver(measure);
    observer.observe(container.value);
  }
});
onBeforeUnmount(() => observer?.disconnect());

watch(
  () => props.focus,
  (f) => {
    const el = container.value;
    if (f === null || !el) return;
    const row = Math.floor(f / columns.value);
    const target = scrollTopForRow(
      row,
      el.scrollTop,
      height.value,
      metrics.value,
    );
    if (target !== null) el.scrollTop = target;
  },
);

/** 换页 / 换筛选后回到顶部（store 每次 reload 落地把 resetKey +1） */
watch(
  () => props.resetKey,
  () => {
    const el = container.value;
    if (!el) return;
    el.scrollTop = 0;
    scrollTop.value = 0;
  },
);

/**
 * 换档位：焦点条目的索引不变，但列数与行高都变了，旧 scrollTop 对应的是别的行。
 * 在 DOM 还没重排前（watch 默认 pre flush）按旧档位记下锚点（焦点，没焦点就取视口首项）与它在视口里的位置，
 * 等 spacer 换成新高度后（nextTick）把 scrollTop 设到 scrollTopAfterResize 算出的值——锚点仍在原地且整行可见。
 */
watch(
  () => props.cardSize,
  (size, oldSize) => {
    const el = container.value;
    if (!el || size === oldSize) return;
    const fromMetrics = cardMetrics(oldSize);
    const from = {
      columns: computeColumns(width.value, fromMetrics),
      metrics: fromMetrics,
      scrollTop: el.scrollTop,
    };
    const anchor =
      props.focus ??
      firstVisibleIndex(from.scrollTop, from.columns, fromMetrics);
    const target = scrollTopAfterResize(
      anchor,
      from,
      { columns: columns.value, metrics: metrics.value },
      height.value,
    );
    void nextTick(() => {
      el.scrollTop = target;
      scrollTop.value = el.scrollTop;
    });
  },
);

function getColumns() {
  return columns.value;
}
function focusContainer() {
  container.value?.focus();
}
defineExpose({ getColumns, focusContainer });
</script>

<template>
  <div
    ref="container"
    class="grid"
    tabindex="0"
    @scroll.passive="onScroll"
    @keydown="emit('keydown', $event)"
  >
    <div class="spacer" :style="{ height: totalHeight + 'px' }">
      <EntryCard
        v-for="v in visible"
        :key="v.entry.entry_id"
        :entry="v.entry"
        :metrics="metrics"
        :selected="selected.has(v.entry.entry_id)"
        :focused="focus === v.index"
        :style="{
          position: 'absolute',
          top: v.top + 'px',
          left: v.left + 'px',
        }"
        @image-click="emit('imageClick', v.index, $event)"
        @image-pointerdown="emit('imagePointerdown', v.index, $event)"
        @card-click="emit('cardClick', v.index, $event)"
        @contextmenu="emit('context', v.index)"
      />
    </div>
    <div v-if="loading" class="status">加载中…</div>
    <div v-else-if="error" class="status read-error" role="alert">
      读取失败，尚不能确定符合条件的条目
    </div>
    <div v-else-if="!items.length" class="status empty">没有符合条件的条目</div>
  </div>
</template>

<style scoped>
.grid {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: auto;
  outline: none;
}
.grid:focus-visible {
  box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--accent) 40%, transparent);
}
.spacer {
  position: relative;
  width: 100%;
}
.status {
  position: sticky;
  bottom: 8px;
  margin: 0 auto;
  width: fit-content;
  padding: 4px 12px;
  border-radius: 12px;
  background: var(--panel);
  border: 1px solid var(--line);
  font-size: 12px;
  color: var(--muted);
}
.status.empty {
  position: absolute;
  inset: 40px 0 auto 0;
}
</style>
