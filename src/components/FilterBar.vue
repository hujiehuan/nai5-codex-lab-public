<script setup lang="ts">
import VButton from "../ui/VButton.vue";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useCodex } from "../stores/codex";
import { debounce } from "../lib/debounce";
import { DECISION_LABEL } from "../lib/decision";
import { ALL_DECISIONS, isFilterActive } from "../lib/query";
import type { Decision } from "../types";

const codex = useCodex();

const keyword = ref(codex.filter.keyword);
const pushKeyword = debounce((v: string) => {
  if (v !== codex.filter.keyword) codex.setFilter({ keyword: v });
}, 200);
watch(keyword, (v) => pushKeyword(v));
watch(
  () => codex.filter.keyword,
  (v) => {
    if (v !== keyword.value) keyword.value = v;
  },
);
onBeforeUnmount(() => pushKeyword.cancel());

function toggleDecision(d: Decision) {
  const set = new Set(codex.filter.decision);
  if (set.has(d)) set.delete(d);
  else set.add(d);
  codex.setFilter({ decision: [...set] });
}

const batch = computed({
  get: () => codex.filter.batch ?? "",
  set: (v: string) => codex.setFilter({ batch: v || null }),
});
const newOnly = computed({
  get: () => !!codex.filter.first_seen_release,
  set: (v: boolean) =>
    codex.setFilter({ first_seen_release: v ? codex.currentRelease : null }),
});
const active = computed(() => isFilterActive(codex.filter, codex.defaults));
</script>

<template>
  <div class="filterbar">
    <div class="row">
      <select v-model="batch" title="更新批次">
        <option value="">全部批次</option>
        <option
          v-for="u in codex.book?.update_filters ?? []"
          :key="u.id"
          :value="u.id"
        >
          {{ u.label }}{{ u.latest ? "（最新）" : "" }}
        </option>
      </select>
      <label
        :title="
          codex.currentRelease
            ? `first_seen_release = ${codex.currentRelease}`
            : '尚未同步'
        "
      >
        <input
          v-model="newOnly"
          type="checkbox"
          :disabled="!codex.currentRelease && !codex.filter.first_seen_release"
        />
        本次新增
      </label>
      <span class="group" title="决策">
        <label v-for="d in ALL_DECISIONS" :key="d"
          ><input
            type="checkbox"
            :checked="codex.filter.decision.includes(d)"
            @change="toggleDecision(d)"
          />
          {{ DECISION_LABEL[d] }}</label
        >
      </span>
      <input
        v-model="keyword"
        class="kw"
        type="search"
        placeholder="关键词（title / tags）"
      />
      <label
        ><input
          type="checkbox"
          :checked="codex.filter.multi_image_only"
          @change="
            codex.setFilter({
              multi_image_only: !codex.filter.multi_image_only,
            })
          "
        />
        只看多图</label
      >
      <label
        ><input
          type="checkbox"
          :checked="codex.filter.dup_only"
          @change="codex.setFilter({ dup_only: !codex.filter.dup_only })"
        />
        只看重复组</label
      >
      <VButton v-if="active" class="reset" @click="codex.resetFilter()"
        >重置筛选</VButton
      >
    </div>
    <div class="counts">
      <span class="summary">
        当前筛选 {{ codex.total }} 条 · 加入 {{ codex.counts.include }} · 不加入
        {{ codex.counts.exclude }} · 未定 {{ codex.counts.undecided }} · 第
        {{ codex.filter.page }}/{{ codex.pageCount }} 页
        <span v-if="codex.filter.first_seen_release" class="dim">
          · 新增于 {{ codex.filter.first_seen_release }}</span
        >
        <span v-if="codex.filter.path_prefix.length" class="dim">
          · {{ codex.filter.path_prefix.join(" / ") }}</span
        >
      </span>
      <!-- 分页条（Pager.vue）由 CodexView 放进来，与计数同一行 -->
      <slot />
    </div>
  </div>
</template>

<style scoped>
.filterbar {
  padding: 10px 16px 8px;
  border-bottom: 1px solid var(--line);
  background: var(--panel);
}
.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 14px;
  font-size: 13px;
}
.group {
  display: inline-flex;
  gap: 8px;
  padding: 2px 8px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
}
label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  cursor: pointer;
}
.kw {
  width: 220px;
}
.counts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 6px 16px;
  margin-top: 6px;
  font-size: 12px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.dim {
  opacity: 0.8;
}
.reset {
  padding: 3px 10px;
  font-size: 12px;
}
</style>
