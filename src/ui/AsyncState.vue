<script setup lang="ts">
import { NAlert, NButton, NEmpty, NSkeleton } from "naive-ui";
defineProps<{
  loading?: boolean;
  error?: string | null;
  empty?: boolean;
  emptyText?: string;
}>();
defineEmits<{ retry: [] }>();
</script>
<template>
  <div v-if="loading" aria-busy="true" aria-label="加载中" class="loading">
    <NSkeleton text :repeat="3" />
  </div>
  <NAlert v-else-if="error" type="error" title="读取失败" role="alert"
    ><p>{{ error }}</p>
    <NButton size="small" @click="$emit('retry')">重试</NButton></NAlert
  ><NEmpty v-else-if="empty" :description="emptyText ?? '暂无记录'" /><slot
    v-else
  />
</template>
<style scoped>
.loading {
  min-height: 90px;
}
p {
  margin: 0 0 10px;
  overflow-wrap: anywhere;
}
</style>
