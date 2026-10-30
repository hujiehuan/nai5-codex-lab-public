<script setup lang="ts">
defineProps<{
  kind: "paid" | "free" | "local" | "model" | "unknown";
  label?: string;
}>();
const labels = {
  paid: "计费路径",
  free: "不额外计费",
  local: "本机计算，不计费",
  model: "模型 API 调用",
  unknown: "费用依插件设置",
};
</script>
<template>
  <span class="cost" :class="kind"
    ><span aria-hidden="true">●</span>{{ label ?? labels[kind] }}</span
  >
</template>
<style scoped>
.cost {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  padding: 2px 8px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 550;
  line-height: 1.6;
  white-space: normal;
  color: var(--muted);
  background: var(--field);
}
.paid,
.model {
  color: var(--paid);
  background: var(--paid-soft);
}
.free,
.local {
  color: var(--green);
  background: var(--green-soft);
}
</style>
