<script setup lang="ts">
import { computed } from "vue";
import { imgSrc } from "../api";
import type { EntryCard as EntryCardT } from "../types";
import { lastSegment, padOrder } from "../lib/format";
import type { CardMetrics } from "../lib/grid";

const props = defineProps<{
  entry: EntryCardT;
  selected: boolean;
  focused: boolean;
  /** 当前档位的尺寸（EntryGrid 算一次传下来），宽高 / 图片区高 / 行高都从这里取 */
  metrics: CardMetrics;
}>();
const emit = defineEmits<{
  imageClick: [ev: MouseEvent];
  /** 图片区按下：CodexView 据此起 250 ms 计时做"按住看大图"（v0.1.3），不 preventDefault，点击语义不变 */
  imagePointerdown: [ev: PointerEvent];
  cardClick: [ev: MouseEvent];
  contextmenu: [ev: MouseEvent];
}>();

/**
 * 图片来源随档位：s / m 用缩略图（320px JPEG，几十 KB），l / xl 才加载 400–650 KB 的展示图；
 * 缺哪个就退到另一个。loading="lazy" 让不在视口的卡片不发请求。
 */
const large = computed(
  () => props.metrics.size === "l" || props.metrics.size === "xl",
);
const src = computed(() => {
  const e = props.entry;
  return imgSrc(
    large.value
      ? (e.image_path ?? e.thumb_path)
      : (e.thumb_path ?? e.image_path),
  );
});
const meta = computed(() =>
  [
    props.entry.rating,
    props.entry.batches.length ? props.entry.batches.join("/") : "—",
    lastSegment(props.entry.path) || "—",
  ].join(" · "),
);
const style = computed(() => ({
  width: `${props.metrics.cardWidth}px`,
  height: `${props.metrics.cardHeight}px`,
  "--thumb-h": `${props.metrics.thumbHeight}px`,
  "--line-h": `${props.metrics.lineHeight}px`,
}));
</script>

<template>
  <div
    class="card"
    :class="[entry.decision, `size-${metrics.size}`, { selected, focused }]"
    :style="style"
    :data-entry-id="entry.entry_id"
    @click="emit('cardClick', $event)"
    @contextmenu.prevent="emit('contextmenu', $event)"
  >
    <div
      class="thumb"
      @click.stop="emit('imageClick', $event)"
      @pointerdown="emit('imagePointerdown', $event)"
    >
      <img
        v-if="src"
        :src="src"
        loading="lazy"
        decoding="async"
        :alt="entry.title"
        draggable="false"
      />
      <div v-else class="placeholder">
        {{ entry.image_count === 0 ? "无配图" : "未下载" }}
      </div>
      <div class="overlay" />
      <span
        v-if="entry.decision === 'include'"
        class="badge check"
        title="已加入"
        >✓</span
      >
      <span
        v-if="entry.dup_count > 1"
        class="badge dup"
        :title="`同画师集合共 ${entry.dup_count} 条（重复组）`"
        >×{{ entry.dup_count }}</span
      >
      <span
        v-if="entry.image_count > 1"
        class="badge multi"
        :title="`${entry.image_count} 张图`"
        >{{ entry.image_count }} 图</span
      >
      <span v-if="entry.is_new" class="badge new" title="本次同步新增">新</span>
    </div>
    <div class="body">
      <div class="title" :title="entry.title">
        <span class="order">{{ padOrder(entry.book_order) }}</span> ·
        {{ entry.title }}
      </div>
      <div class="tags" :title="entry.tags_first_line">
        {{ entry.tags_first_line || " " }}
      </div>
      <div class="meta" :title="entry.path.join(' / ')">{{ meta }}</div>
    </div>
  </div>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  overflow: hidden;
  user-select: none;
  cursor: pointer;
  transition: border-color 0.1s;
}
.card.include {
  border: 2px solid var(--green);
}
.card.exclude {
  border-color: var(--line-strong);
  color: var(--muted);
}
.card.selected {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.card.focused {
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 45%, transparent);
}
/* 图片区正方形，高度 = 档位宽度（--thumb-h 由 metrics 注入）；include 态边框多 1px，两边共 2px 从这里补回 */
.thumb {
  position: relative;
  height: var(--thumb-h);
  flex: 0 0 var(--thumb-h);
  background: var(--bg);
  display: flex;
  align-items: center;
  justify-content: center;
}
.card.include .thumb {
  height: calc(var(--thumb-h) - 2px);
  flex-basis: calc(var(--thumb-h) - 2px);
}
.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.card.exclude .thumb img {
  filter: grayscale(0.6);
}
.placeholder {
  color: var(--muted);
  font-size: 12px;
}
.overlay {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
.card.include .overlay {
  background: var(--green-overlay);
}
.card.exclude .overlay {
  background: var(--black-overlay);
}
.badge {
  position: absolute;
  padding: 1px 6px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
  line-height: 16px;
  color: var(--overlay-text);
  background: var(--black-overlay);
}
.badge.check {
  top: 6px;
  left: 6px;
  background: var(--green-soft);
  color: var(--green);
  font-size: 13px;
}
.badge.dup {
  top: 6px;
  right: 6px;
  background: var(--accent);
  color: var(--on-accent);
}
.badge.multi {
  bottom: 6px;
  right: 6px;
}
.badge.new {
  bottom: 6px;
  left: 6px;
  background: var(--danger);
  color: var(--on-danger);
}
/* 文字区：上下 6px 内边距 + 三行 × --line-h + 两个 2px 行距 = metrics.bodyHeight */
.body {
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
}
.title,
.tags,
.meta {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
  line-height: var(--line-h);
}
.title {
  font-weight: 600;
  font-size: 13px;
}
.order {
  font-variant-numeric: tabular-nums;
  color: var(--muted);
  font-weight: 500;
}
.tags {
  color: var(--ink);
  opacity: 0.85;
}
.meta {
  color: var(--muted);
}
/* 特大档字号略放大（行高 20 已在 metrics 里给足） */
.card.size-xl .title {
  font-size: 14px;
}
.card.size-xl .tags,
.card.size-xl .meta {
  font-size: 13px;
}
.card.size-xl .badge {
  font-size: 12px;
  line-height: 18px;
  padding: 1px 8px;
}
.card.size-xl .badge.check {
  font-size: 14px;
}
</style>
