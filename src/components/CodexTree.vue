<script setup lang="ts">
import { ref } from "vue";
import type { TreeNode } from "../types";

const props = defineProps<{
  nodes: TreeNode[];
  prefix: string[];
  active: string[];
  depth?: number;
}>();
const emit = defineEmits<{ select: [path: string[]] }>();

const collapsed = ref<Set<string>>(new Set());

function samePrefix(path: string[], len: number) {
  if (path.length !== len) return false;
  return path.every((p, i) => p === props.active[i]);
}
function isActive(path: string[]) {
  return samePrefix(path, props.active.length);
}
function isOnPath(path: string[]) {
  return (
    path.length < props.active.length &&
    path.every((p, i) => p === props.active[i])
  );
}
function toggle(name: string) {
  const s = new Set(collapsed.value);
  if (s.has(name)) s.delete(name);
  else s.add(name);
  collapsed.value = s;
}
</script>

<template>
  <ul class="tree" :class="{ nested: (depth ?? 0) > 0 }">
    <li v-for="n in nodes" :key="n.name">
      <div
        class="row"
        :class="{
          active: isActive([...prefix, n.name]),
          onpath: isOnPath([...prefix, n.name]),
        }"
        @click="emit('select', [...prefix, n.name])"
      >
        <span
          v-if="n.children.length"
          class="caret"
          @click.stop="toggle(n.name)"
          >{{ collapsed.has(n.name) ? "▸" : "▾" }}</span
        >
        <span v-else class="caret" />
        <span class="name" :title="n.name">{{ n.name }}</span>
        <span class="count">{{ n.count }}</span>
      </div>
      <CodexTree
        v-if="n.children.length && !collapsed.has(n.name)"
        :nodes="n.children"
        :prefix="[...prefix, n.name]"
        :active="active"
        :depth="(depth ?? 0) + 1"
        @select="emit('select', $event)"
      />
    </li>
  </ul>
</template>

<style scoped>
.tree {
  list-style: none;
  margin: 0;
  padding: 0;
}
.tree.nested {
  padding-left: 14px;
}
.row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 6px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
}
.row:hover {
  background: var(--bg);
}
.row.active {
  background: var(--accent);
  color: var(--on-accent);
}
.row.onpath .name {
  font-weight: 600;
}
.caret {
  width: 14px;
  text-align: center;
  color: var(--muted);
  flex: 0 0 14px;
}
.row.active .caret {
  color: var(--on-accent);
}
.name {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.count {
  color: var(--muted);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.row.active .count {
  color: var(--on-accent);
}
</style>
