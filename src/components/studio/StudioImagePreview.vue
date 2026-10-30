<script setup lang="ts">
import { ref, watch } from "vue";
import { NModal } from "naive-ui";
import { imgSrc } from "../../api";
import VButton from "../../ui/VButton.vue";

const props = defineProps<{ path: string | null }>();
const emit = defineEmits<{ close: [] }>();
const frame = ref<HTMLElement | null>(null);
const state = ref<"loading" | "ready" | "error">("loading");
const attempt = ref(0);
watch(() => props.path, () => { state.value = "loading"; attempt.value = 0; });
function retry() {
  // Keep keyboard focus in the dialog before removing the focused retry button.
  frame.value?.querySelector<HTMLButtonElement>(".close-image")?.focus({ preventScroll: true });
  state.value = "loading";
  attempt.value++;
}
</script>

<template>
  <NModal :show="!!path" transform-origin="center" @update:show="(show) => { if (!show) emit('close'); }">
    <!-- The installed Naive UI focus trap locates its content by the DIV root. -->
    <div v-if="path" ref="frame" class="studio-image-preview" role="dialog" aria-modal="true" aria-label="实验原图" tabindex="-1">
      <header class="preview-toolbar">
        <strong>实验原图</strong>
        <VButton class="close-image" @click="emit('close')">关闭原图</VButton>
      </header>
      <div class="preview-stage" :aria-busy="state === 'loading'">
        <img :key="`${path}:${attempt}`" :src="imgSrc(path) ?? undefined" alt="实验原图"
          :class="{ 'preview-hidden': state !== 'ready' }" draggable="false"
          @load="state = 'ready'" @error="state = 'error'" />
        <p v-if="state === 'loading'" class="preview-status" role="status">正在加载原图…</p>
        <div v-else-if="state === 'error'" class="preview-status" role="alert">
          <p>原图加载失败，请重试。</p>
          <VButton @click="retry">重试加载</VButton>
        </div>
      </div>
    </div>
  </NModal>
</template>

<style scoped>
/* NModal supplies focus trapping, Escape and focus restoration. Its default
   auto margins must not combine with our fixed, viewport-sized image frame. */
.studio-image-preview {
  position: fixed;
  inset: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  overflow: hidden;
  background: var(--panel);
  color: var(--ink);
}
.preview-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex: none; }
.preview-stage { position: relative; flex: 1; min-width: 0; min-height: 0; background: var(--field); border-radius: var(--radius); overflow: hidden; }
.preview-stage img { display: block; width: 100%; height: 100%; object-fit: contain; }
.preview-hidden { visibility: hidden; }
.preview-status { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; margin: 0; color: var(--muted); }
</style>
