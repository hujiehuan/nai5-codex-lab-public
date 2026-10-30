<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  /** 0–1；null 表示不确定（跑马灯） */
  value: number | null;
  label?: string;
  danger?: boolean;
}>();

const pct = computed(() =>
  props.value === null ? 0 : Math.max(0, Math.min(1, props.value)) * 100,
);
</script>

<template>
  <div class="pb">
    <div class="pb-track" :class="{ indeterminate: value === null, danger }">
      <div
        class="pb-fill"
        :style="{ width: value === null ? '30%' : pct + '%' }"
      />
    </div>
    <div v-if="label" class="pb-label">{{ label }}</div>
  </div>
</template>

<style scoped>
.pb {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.pb-track {
  position: relative;
  height: 8px;
  border-radius: 4px;
  background: var(--line);
  overflow: hidden;
}
.pb-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 4px;
  transition: width 0.2s;
}
.pb-track.danger .pb-fill {
  background: var(--danger);
}
.pb-track.indeterminate .pb-fill {
  position: absolute;
  animation: slide 1.2s linear infinite;
}
.pb-label {
  font-size: 12px;
  color: var(--muted);
}
@keyframes slide {
  from {
    left: -30%;
  }
  to {
    left: 100%;
  }
}
</style>
