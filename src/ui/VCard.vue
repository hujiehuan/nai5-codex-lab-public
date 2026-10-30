<script setup lang="ts">
defineProps<{ title?: string; subtitle?: string; flush?: boolean }>();
</script>
<template>
  <section class="vcard" :class="{ flush }">
    <header v-if="title || $slots.actions" class="vcard-head">
      <div>
        <h2>{{ title }}</h2>
        <p v-if="subtitle">{{ subtitle }}</p>
      </div>
      <slot name="actions" />
    </header>
    <div class="vcard-body"><slot /></div>
  </section>
</template>
<style scoped>
.vcard {
  min-width: 0;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--panel);
  box-shadow: var(--shadow);
}
.vcard-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px;
  border-bottom: 1px solid var(--line);
  flex-wrap: wrap;
}
.vcard-head h2 {
  font-size: 15px;
  margin: 0;
  font-weight: 650;
}
.vcard-head p {
  font-size: 12px;
  color: var(--muted);
  margin: 4px 0 0;
}
.vcard-body {
  padding: 18px;
  min-width: 0;
}
.flush .vcard-body {
  padding: 0;
}
</style>
