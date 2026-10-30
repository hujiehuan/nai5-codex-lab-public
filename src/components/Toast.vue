<script setup lang="ts">
import { useToast } from "../stores/toast";

const toast = useToast();
</script>

<template>
  <div class="toasts" aria-live="polite">
    <div
      v-for="t in toast.items"
      :key="t.id"
      :class="['toast', t.kind]"
      @click="toast.remove(t.id)"
    >
      {{ t.message }}
    </div>
  </div>
</template>

<style scoped>
.toasts {
  position: fixed;
  right: 16px;
  bottom: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 1000;
  max-width: 420px;
}
.toast {
  padding: 10px 14px;
  border-radius: var(--radius);
  background: var(--panel);
  border: 1px solid var(--line);
  border-left: 4px solid var(--accent);
  color: var(--ink);
  box-shadow: 0 6px 20px var(--backdrop);
  cursor: pointer;
  font-size: 13px;
  word-break: break-all;
}
.toast.success {
  border-left-color: var(--green);
}
.toast.error {
  border-left-color: var(--danger);
}
</style>
