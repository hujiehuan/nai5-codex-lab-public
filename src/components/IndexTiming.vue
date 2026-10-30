<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import {
  formatElapsed,
  formatRate,
  formatRemaining,
  timingView,
  type IndexTiming,
} from "../lib/index-timing";

const props = defineProps<{ timing: IndexTiming | null }>();
const now = ref(performance.now());
let timer: ReturnType<typeof setInterval> | null = null;

function stopClock() {
  if (timer !== null) clearInterval(timer);
  timer = null;
}
watch(
  () => props.timing?.finishedAt === null,
  (running) => {
    stopClock();
    now.value = performance.now();
    if (running)
      timer = setInterval(() => {
        now.value = performance.now();
      }, 1000);
  },
  { immediate: true },
);
onBeforeUnmount(stopClock);

const view = computed(() =>
  props.timing ? timingView(props.timing, now.value) : null,
);
</script>

<template>
  <div v-if="view" class="index-timing" aria-label="画风索引时间估算">
    <span>{{ formatElapsed(view.elapsedSeconds) }}</span>
    <strong
      v-if="view.remainingSeconds !== null"
      title="根据最近几批的实际速度估算，随计算进度更新"
    >
      {{ formatRemaining(view.remainingSeconds) }}
    </strong>
    <span v-if="view.itemsPerMinute !== null">{{
      formatRate(view.itemsPerMinute)
    }}</span>
    <span v-if="view.phase === 'estimating'">正在估算…</span>
    <span v-else-if="view.phase === 'stalled'">等待下一批，正在重新估算…</span>
    <span v-else-if="view.phase === 'finishing'">正在收尾…</span>
  </div>
</template>

<style scoped>
.index-timing {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 6px 16px;
  color: var(--muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.index-timing strong {
  color: var(--ink);
  font-weight: 600;
}
</style>
