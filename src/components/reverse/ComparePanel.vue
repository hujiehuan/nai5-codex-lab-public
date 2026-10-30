<script setup lang="ts">
import VButton from "../../ui/VButton.vue";
import { computed, ref } from "vue";
import { useReverse } from "../../stores/reverse";
import { fmtDelta, fmtPct, fmtTime } from "../../lib/format";

const rev = useReverse();
const a = ref("");
const b = ref("");
const changed = computed(() =>
  (rev.compare?.rows ?? []).filter((r) => r.delta !== null && r.delta !== 0),
);
const unchanged = computed(
  () => (rev.compare?.rows.length ?? 0) - changed.value.length,
);

function run() {
  if (a.value && b.value && a.value !== b.value)
    void rev.runCompare(a.value, b.value);
}
</script>

<template>
  <div class="compare">
    <div class="head">
      <span class="k">对比</span>
      <select v-model="a">
        <option value="">批次 A…</option>
        <option v-for="x in rev.batches" :key="x.batch_id" :value="x.batch_id">
          {{ fmtTime(x.created_at) }} · {{ x.template }} / {{ x.ruleset }} ·
          {{ x.providers.join("+") }}
        </option>
      </select>
      <select v-model="b">
        <option value="">批次 B…</option>
        <option v-for="x in rev.batches" :key="x.batch_id" :value="x.batch_id">
          {{ fmtTime(x.created_at) }} · {{ x.template }} / {{ x.ruleset }} ·
          {{ x.providers.join("+") }}
        </option>
      </select>
      <VButton
        :disabled="!a || !b || a === b || rev.compareLoading"
        @click="run"
        >对比</VButton
      >
      <span v-if="rev.compare" class="dim">
        平均 F1：A {{ fmtPct(rev.compare.avg_f1_a) }} → B
        {{ fmtPct(rev.compare.avg_f1_b) }} · 变化 {{ changed.length }} 题 · 不变
        {{ unchanged }} 题
      </span>
    </div>
    <div v-if="rev.compareError" class="read-error" role="alert">
      {{ rev.compareError }}
    </div>
    <div v-else-if="rev.compareLoading" class="dim" aria-busy="true">
      正在对比…
    </div>
    <div class="table-scroll" v-else-if="rev.compare && changed.length">
      <table class="tbl">
        <thead>
          <tr>
            <th>question</th>
            <th>F1 A</th>
            <th>F1 B</th>
            <th>Δ</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="r in changed"
            :key="r.question_id"
            :class="{ up: (r.delta ?? 0) > 0, down: (r.delta ?? 0) < 0 }"
          >
            <td class="mono">{{ r.question_id }}</td>
            <td>{{ fmtPct(r.f1_a) }}</td>
            <td>{{ fmtPct(r.f1_b) }}</td>
            <td>
              <b>{{ fmtDelta(r.delta) }}</b>
            </td>
            <td>
              <VButton
                v-if="r.run_a !== null"
                class="mini"
                @click="rev.openRun(r.run_a)"
                >A</VButton
              >
              <VButton
                v-if="r.run_b !== null"
                class="mini"
                @click="rev.openRun(r.run_b)"
                >B</VButton
              >
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div v-else-if="rev.compare" class="dim">两个批次逐题 F1 没有差异</div>
  </div>
</template>

<style scoped>
.compare {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 10px;
  font-size: 13px;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.head select {
  max-width: 300px;
}
.k {
  font-weight: 600;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 3px 6px;
  border-bottom: 1px solid var(--line);
  white-space: nowrap;
}
.tbl th {
  color: var(--muted);
  font-weight: 500;
}
tr.up td b {
  color: var(--green);
}
tr.down td b {
  color: var(--danger);
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.mini {
  padding: 0 6px;
  font-size: 11px;
}
.dim {
  color: var(--muted);
  font-size: 12px;
}
</style>
