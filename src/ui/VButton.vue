<script setup lang="ts">
import { computed, useAttrs } from "vue";
import { NButton } from "naive-ui";

defineOptions({ inheritAttrs: false });
const attrs = useAttrs();
// Retain existing action classes and handlers while giving all legacy screens
// the same focus, loading/disabled appearance and 30px minimum control height.
const type = computed(() =>
  String(attrs.class ?? "")
    .split(/\s+/)
    .includes("primary")
    ? "primary"
    : "default",
);
</script>
<template>
  <NButton v-bind="attrs" :type="type" size="small"><slot /></NButton>
</template>
