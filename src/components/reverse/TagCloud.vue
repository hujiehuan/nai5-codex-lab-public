<script setup lang="ts">
import type { CloudTag } from "../../lib/tagcloud";
import { TAG_CLASS_LABEL } from "../../lib/tagcloud";

defineProps<{ tags: CloudTag[]; title: string }>();
</script>

<template>
  <div class="cloud">
    <div class="cloud-title">
      {{ title }} <span class="dim">({{ tags.length }})</span>
    </div>
    <div class="chips">
      <span
        v-for="t in tags"
        :key="t.tag"
        class="chip"
        :class="[t.cls, { contradiction: t.contradiction }]"
        :title="
          TAG_CLASS_LABEL[t.cls] +
          (t.contradiction ? ' · 与 negative 矛盾' : '')
        "
        >{{ t.tag }}</span
      >
      <span v-if="!tags.length" class="dim">（空）</span>
    </div>
  </div>
</template>

<style scoped>
.cloud-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--muted);
  margin-bottom: 4px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.chip {
  padding: 1px 8px;
  border-radius: 10px;
  font-size: 12px;
  line-height: 18px;
  border: 1px solid transparent;
  background: var(--bg);
  color: var(--ink);
}
.chip.hit {
  background: var(--green-soft);
  color: var(--green);
  border-color: var(--green);
}
.chip.miss {
  color: var(--muted);
  border-color: var(--line);
}
.chip.expected {
  color: var(--muted);
  border-style: dashed;
  border-color: var(--line);
  opacity: 0.8;
}
.chip.extra {
  background: var(--paid-soft);
  color: var(--paid);
  border-color: var(--paid);
}
.chip.violation {
  background: var(--danger-soft);
  color: var(--danger);
  border-color: var(--danger);
}
.chip.contradiction {
  outline: 2px solid var(--danger);
  outline-offset: 0;
}
.dim {
  color: var(--muted);
  font-weight: 400;
}
</style>
