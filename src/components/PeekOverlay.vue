<script setup lang="ts">
// 按住看大图的覆盖层（v0.1.3）：全窗口、半透明黑底、图片居中。pointer-events: none——
// 按住期间的 pointermove / pointerup 照样落到下面的卡片与 window，松开一定能被 CodexView 收到。
// 挂到 body（Teleport），不受 .content 的 overflow / 网格的绝对定位影响。
import { computed } from "vue";
import { imgSrc } from "../api";
import type { EntryCard } from "../types";
import { padOrder } from "../lib/format";

const props = defineProps<{ entry: EntryCard | null }>();

// 展示图优先（完整分辨率，400–650 KB）；没下载就退到 320 px 缩略图并在角落标注；都没有给占位
const src = computed(() =>
  imgSrc(
    props.entry ? (props.entry.image_path ?? props.entry.thumb_path) : null,
  ),
);
const usingThumb = computed(
  () => !!props.entry && !props.entry.image_path && !!props.entry.thumb_path,
);
const meta = computed(() => {
  const e = props.entry;
  if (!e) return "";
  return `${e.rating} · ${e.batches.length ? e.batches.join("/") : "—"}`;
});
</script>

<template>
  <Teleport to="body">
    <Transition name="peek">
      <div v-if="entry" class="peek" aria-hidden="true">
        <span
          v-if="entry.image_count > 1"
          class="corner count"
          title="多图条目：按住只看主图，全部图片在详情抽屉里"
          >1/{{ entry.image_count }}</span
        >
        <span v-if="usingThumb" class="corner thumb-note">未下载展示图</span>
        <div class="stage">
          <img
            v-if="src"
            :src="src"
            :alt="entry.title"
            decoding="async"
            draggable="false"
          />
          <div v-else class="placeholder">未下载</div>
        </div>
        <div class="caption">
          <span class="head" :title="entry.title"
            ><span class="order">{{ padOrder(entry.book_order) }}</span> ·
            {{ entry.title }}</span
          >
          <span class="tags" :title="entry.tags_first_line">{{
            entry.tags_first_line
          }}</span>
          <span class="meta">{{ meta }}</span>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.peek {
  position: fixed;
  inset: 0;
  z-index: 100; /* 抽屉 50 / 反推面板 55 / 模型弹层 60 之上，Toast 1000 之下 */
  pointer-events: none;
  user-select: none;
  background: var(--black-overlay);
  color: var(--overlay-text);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 14px 16px;
}
/* 图片区吃掉除底部一行之外的全部高度；img 的 100% 以它为准，vw / vh 只是上限——
   整页 zoom（⌘= / ⌘-）下 WebKit 的视口单位可能大于实际窗口，取 min 保证不溢出 */
.stage {
  flex: 1 1 auto;
  min-height: 0;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
.stage img {
  max-width: min(96vw, 100%);
  max-height: min(92vh, 100%);
  object-fit: contain;
  display: block;
  box-shadow: 0 10px 40px var(--black-overlay);
}
.placeholder {
  width: 320px;
  height: 320px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px dashed var(--overlay-border);
  border-radius: var(--radius);
  color: var(--overlay-muted);
}
/* 底部一行：编号 · title | tags 首行（超长省略）| rating · 批次；只有 tags 可压缩，标题最多占 40% */
.caption {
  flex: 0 0 auto;
  width: 100%;
  display: flex;
  align-items: baseline;
  justify-content: center;
  gap: 14px;
  font-size: 13px;
  line-height: 20px;
}
.head {
  flex: 0 0 auto;
  max-width: 40%;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.order {
  font-variant-numeric: tabular-nums;
  font-weight: 500;
  opacity: 0.7;
}
.tags {
  flex: 0 1 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  opacity: 0.85;
}
.meta {
  flex: 0 0 auto;
  opacity: 0.7;
}
.corner {
  position: absolute;
  top: 12px;
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--black-overlay);
  font-size: 12px;
  line-height: 18px;
}
.corner.count {
  left: 12px;
  font-variant-numeric: tabular-nums;
}
.corner.thumb-note {
  right: 12px;
  color: var(--overlay-warning);
}
/* 进出各 80 ms 淡入淡出；系统减少动态效果时直接切 */
.peek-enter-active,
.peek-leave-active {
  transition: opacity 80ms linear;
}
.peek-enter-from,
.peek-leave-to {
  opacity: 0;
}
@media (prefers-reduced-motion: reduce) {
  .peek-enter-active,
  .peek-leave-active {
    transition: none;
  }
}
</style>
