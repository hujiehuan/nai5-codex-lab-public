<script setup lang="ts">
defineProps<{
  kind: "paid" | "free" | "local" | "model" | "unknown";
  label?: string;
}>();
const labels = {
  paid: "Anlas",
  free: "免费点数",
  local: "不消耗点数 / Anlas",
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
