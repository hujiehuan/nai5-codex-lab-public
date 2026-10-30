<script setup lang="ts">
import VButton from "../ui/VButton.vue";
import VCost from "../ui/VCost.vue";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useReverse } from "../stores/reverse";
import QuestionBank from "../components/reverse/QuestionBank.vue";
import RunConfig from "../components/reverse/RunConfig.vue";
import BatchResults from "../components/reverse/BatchResults.vue";
import RunDetailPanel from "../components/reverse/RunDetailPanel.vue";
import ComparePanel from "../components/reverse/ComparePanel.vue";
import StylePanel from "../components/reverse/StylePanel.vue";

const rev = useReverse();
/** 底部两个可折叠面板互斥：风格检索（§12.5）/ 对比两个批次；null = 都收起。 */
const bottomTab = ref<"style" | "compare" | null>(null);
function toggleTab(t: "style" | "compare") {
  bottomTab.value = bottomTab.value === t ? null : t;
}
let timer: number | null = null;

onMounted(() => {
  void rev.loadAll();
  // 批次在跑时轮询批次表：Channel 的终止事件丢了也能把"运行 batch"按钮恢复过来（见 stores/reverse.ts loadBatches）
  timer = window.setInterval(() => {
    if (rev.running) void rev.loadBatches();
  }, 3000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});
</script>

<template>
  <div class="lab" data-testid="eval">
    <header class="page-heading lab-heading">
      <div>
        <p class="eyebrow">NAI5 · REVERSE EVAL</p>
        <h1>反推评测</h1>
        <p>
          同一批题目横向比较多个视觉模型：P/R/F1、延迟、泄漏与拒答，可选重画后由评审模型打分。
        </p>
      </div>
      <VCost kind="model" label="反推 / 评分会调用模型" />
    </header>
    <div class="cols">
      <section class="col bank">
        <div class="col-head">题库</div>
        <QuestionBank />
      </section>
      <section class="col config">
        <div class="col-head">实验配置</div>
        <RunConfig />
      </section>
      <section class="col results">
        <div class="col-head">结果</div>
        <BatchResults />
      </section>
    </div>
    <footer
      class="bottom"
      :class="{ open: bottomTab !== null, style: bottomTab === 'style' }"
    >
      <div class="bottom-head">
        <VButton
          class="tab"
          :class="{ on: bottomTab === 'style' }"
          @click="toggleTab('style')"
          >{{
            bottomTab === "style" ? "▾" : "▸"
          }}
          风格检索（查询图按余弦取最相近的风格配方）</VButton
        >
        <VButton
          class="tab"
          :class="{ on: bottomTab === 'compare' }"
          @click="toggleTab('compare')"
          >{{
            bottomTab === "compare" ? "▾" : "▸"
          }}
          对比两个批次（只显示分数变化的题）</VButton
        >
      </div>
      <StylePanel v-if="bottomTab === 'style'" />
      <ComparePanel v-if="bottomTab === 'compare'" />
    </footer>
    <RunDetailPanel />
  </div>
</template>

<style scoped>
.lab {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.cols {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(240px, 0.9fr) minmax(300px, 1fr) minmax(
      280px,
      1.2fr
    );
  gap: 14px;
  padding: 0 18px 14px;
}
.col {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
  border-right: 1px solid var(--line);
  background: var(--panel);
}
.col {
  border: 1px solid var(--line);
  border-radius: 12px;
  box-shadow: var(--shadow);
  overflow: hidden;
}
.col:last-child {
  border-right: 1px solid var(--line);
}
.col-head {
  padding: 8px 12px;
  font-weight: 600;
  font-size: 12px;
  color: var(--muted);
  border-bottom: 1px solid var(--line);
}
.col > :last-child {
  flex: 1;
  min-height: 0;
}
.bottom {
  border-top: 1px solid var(--line);
  background: var(--panel);
  max-height: 40%;
  display: flex;
  flex-direction: column;
}
.bottom.style {
  max-height: 55%;
}
.bottom-head {
  display: flex;
  gap: 18px;
  padding: 6px 12px;
  font-size: 12px;
  color: var(--muted);
  user-select: none;
  flex-wrap: wrap;
}
.bottom-head .tab {
  cursor: pointer;
  height: auto;
  white-space: normal;
  text-align: left;
}
.bottom-head .tab.on {
  color: var(--ink);
  font-weight: 600;
}
.bottom.open > :last-child {
  overflow: auto;
}
@media (max-width: 1100px) {
  .lab {
    height: auto;
    min-height: 100%;
  }
  .cols {
    grid-template-columns: minmax(0, 1fr);
    flex: none;
  }
  .col {
    min-height: 280px;
    max-height: 680px;
  }
  .col > :last-child {
    overflow: auto;
  }
  .bottom,
  .bottom.style {
    max-height: none;
  }
  .bottom.open > :last-child {
    max-height: 600px;
  }
  .bottom-head {
    gap: 8px;
  }
}
</style>
