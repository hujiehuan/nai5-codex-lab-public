<script setup lang="ts">
import { computed } from "vue";
import { NTag } from "naive-ui";
const props = defineProps<{ status: unknown }>();
const states: Record<
  string,
  { label: string; type: "success" | "warning" | "error" | "info" | "default" }
> = {
  ok: { label: "✓ 完成", type: "success" },
  running: { label: "◷ 进行中", type: "info" },
  fail: { label: "× 失败", type: "error" },
  skip: { label: "↪ 跳过", type: "default" },
  refused: { label: "已拒绝", type: "warning" },
  degraded: { label: "已回退", type: "warning" },
  partial: { label: "部分完成", type: "warning" },
};
const state = computed(
  () =>
    states[String(props.status)] ?? {
      label: String(props.status ?? "未知"),
      type: "default" as const,
    },
);
</script>
<template>
  <NTag size="small" :type="state.type" round>{{ state.label }}</NTag>
</template>
