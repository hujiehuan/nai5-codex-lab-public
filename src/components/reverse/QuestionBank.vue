<script setup lang="ts">
import VButton from "../../ui/VButton.vue";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { imgSrc } from "../../api";
import { localQuestionId, useReverse } from "../../stores/reverse";
import { DECISION_LABEL } from "../../lib/decision";
import { isDragActive } from "../../lib/dnd";
import {
  PROSE_KIND_LABEL,
  previewAsLines,
  proseKind,
  questionIsProse,
} from "../../lib/prose";
import { PROSE_RUN_NOTE } from "../../lib/reverse-summary";
import { ALL_DECISIONS } from "../../lib/query";
import { debounce } from "../../lib/debounce";
import { NAX_ARTIST_ID } from "../../lib/artist-sources";
import type { Decision, Question } from "../../types";

const rev = useReverse();

const keyword = ref(rev.questionFilter.keyword);
const pathPrefix = ref(rev.questionFilter.path_prefix);
const pushText = debounce(() => {
  if (
    keyword.value !== rev.questionFilter.keyword ||
    pathPrefix.value !== rev.questionFilter.path_prefix
  ) {
    rev.setQuestionFilter({
      keyword: keyword.value,
      path_prefix: pathPrefix.value,
    });
  }
}, 300);
watch([keyword, pathPrefix], () => pushText());

const codexId = computed({
  get: () => rev.questionFilter.codex_id,
  set: (v: string) => rev.setQuestionFilter({ codex_id: v }),
});
const mainOnly = computed({
  get: () => rev.questionFilter.main_image_only,
  set: (v: boolean) => rev.setQuestionFilter({ main_image_only: v }),
});
// ---- 散文题（CONTRACT §11.4）----
// "只看标签题"是纯前端过滤（store 的 visibleQuestions），切换不重拉；徽标 title 说明命中了 §11.1 的哪条规则。
const tagsOnly = computed({
  get: () => rev.questionFilter.tags_only,
  set: (v: boolean) => rev.setQuestionFilter({ tags_only: v }),
});
function proseTitle(q: Question): string {
  if (typeof q.prose === "boolean") return PROSE_RUN_NOTE;
  const k = proseKind(previewAsLines(q.truth_preview));
  return `${PROSE_RUN_NOTE}（按预览判定：${k ? PROSE_KIND_LABEL[k] : "散文"}）`;
}
function toggleDecision(d: Decision) {
  const s = new Set(rev.questionFilter.decision);
  if (s.has(d)) s.delete(d);
  else s.add(d);
  rev.setQuestionFilter({ decision: ALL_DECISIONS.filter((x) => s.has(x)) });
}

const selectedInList = computed(
  () =>
    rev.visibleQuestions.filter((q) =>
      rev.selectedQuestionIds.has(q.question_id),
    ).length,
);

// ---- 拖图进题库（CONTRACT §10.3）----
// 只在题库挂载期间订阅 Tauri webview 的 drag-drop 事件（= 只在反推实验室页面生效），卸载时退订。
// vitest / 纯浏览器里没有 Tauri：动态 import + try/catch，取不到就静默，"添加本地图片"按钮照常可用。
const dragOver = ref(false);
let unlistenDrop: (() => void) | null = null;
let unmounted = false;
async function subscribeDragDrop() {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    const off = await getCurrentWebview().onDragDropEvent((event) => {
      const e = event.payload;
      if (e.type === "drop") {
        dragOver.value = false;
        rev.addDroppedPaths(e.paths);
      } else {
        dragOver.value = isDragActive(e.type);
      }
    });
    // 订阅完成前组件已经卸载（快速切页）→ 立刻退订
    if (unmounted) off();
    else unlistenDrop = off;
  } catch {
    // 非 Tauri 环境或 webview 不支持拖放：忽略
  }
}

onMounted(() => {
  if (!rev.questions.length) void rev.loadQuestions();
  void subscribeDragDrop();
});
onBeforeUnmount(() => {
  unmounted = true;
  unlistenDrop?.();
  unlistenDrop = null;
});
</script>

<template>
  <div class="bank" :class="{ dragover: dragOver }">
    <div class="filters">
      <div class="row">
        <select v-model="codexId">
          <option value="nai5_community_pack">精选图包</option>
          <option value="artist_nai5_personal">画师词典</option>
          <option :value="NAX_ARTIST_ID">NAX · V5 画师测试库</option>
          <option value="local_pack">本地图包</option>
          <option value="local_artist">本地画师</option>
          <option value="">全部</option>
        </select>
      </div>
      <div class="row">
        <span class="group">
          <label v-for="d in ALL_DECISIONS" :key="d"
            ><input
              type="checkbox"
              :checked="rev.questionFilter.decision.includes(d)"
              @change="toggleDecision(d)"
            />
            {{ DECISION_LABEL[d] }}</label
          >
        </span>
        <label><input v-model="mainOnly" type="checkbox" /> 只取主图</label>
        <label
          title="隐藏真值是散文（XML 块 / 代码围栏 / 整段自然语言）的条目；它们不计 P/R/F1"
          ><input v-model="tagsOnly" type="checkbox" /> 只看标签题</label
        >
      </div>
      <div class="row">
        <input v-model="pathPrefix" placeholder="目录前缀，如 图包/示例甲" />
        <input v-model="keyword" type="search" placeholder="关键词" />
        <VButton
          class="mini"
          :disabled="rev.questionsLoading"
          @click="rev.loadQuestions()"
          >查询</VButton
        >
      </div>
      <div class="row dim">
        {{
          rev.questionsError
            ? "读取失败，题目数未知"
            : rev.questionsLoading
              ? "读取中…"
              : `${rev.visibleQuestions.length} 题`
        }}<template v-if="rev.proseCount">
          · 散文 {{ rev.proseCount
          }}{{ tagsOnly ? "（已隐藏）" : "" }}</template
        >
        · 已勾 {{ selectedInList }}
        <VButton class="mini" @click="rev.selectAllQuestions()">全选</VButton>
        <VButton class="mini" @click="rev.clearQuestions()">清空</VButton>
      </div>
    </div>

    <div v-if="rev.questionsError" class="read-error" role="alert">
      {{ rev.questionsError }}<VButton @click="rev.loadQuestions">重试</VButton>
    </div>
    <p
      v-else-if="!rev.questionsLoading && !rev.visibleQuestions.length"
      class="dim"
    >
      没有符合当前条件的题目。
    </p>
    <ul v-if="!rev.questionsError" class="list">
      <li
        v-for="q in rev.visibleQuestions"
        :key="q.question_id"
        :class="{ on: rev.selectedQuestionIds.has(q.question_id) }"
        @click="rev.toggleQuestion(q.question_id)"
      >
        <input
          type="checkbox"
          :checked="rev.selectedQuestionIds.has(q.question_id)"
          @click.stop="rev.toggleQuestion(q.question_id)"
        />
        <img
          v-if="imgSrc(q.thumb_path)"
          :src="imgSrc(q.thumb_path)!"
          loading="lazy"
          decoding="async"
          alt=""
        />
        <div v-else class="noimg">无图</div>
        <div class="info">
          <div class="t" :title="q.title">
            <span
              v-if="questionIsProse(q)"
              class="badge prose"
              :title="proseTitle(q)"
              >散文</span
            >{{ q.title
            }}<span v-if="q.ordinal" class="dim"> #{{ q.ordinal + 1 }}</span>
          </div>
          <div class="dim small" :title="q.truth_preview">
            {{ q.rating }} · {{ q.truth_source }} · {{ q.truth_preview }}
          </div>
        </div>
      </li>
    </ul>

    <div class="local">
      <div class="row">
        <VButton class="mini" @click="rev.addLocalImage()"
          >添加本地图片</VButton
        >
        <span class="dim">或把图片拖进来 · 无标准答案，只看输出</span>
      </div>
      <ul v-if="rev.localImages.length" class="list local-list">
        <li
          v-for="p in rev.localImages"
          :key="p"
          :class="{ on: rev.selectedQuestionIds.has(localQuestionId(p)) }"
          @click="rev.toggleQuestion(localQuestionId(p))"
        >
          <input
            type="checkbox"
            :checked="rev.selectedQuestionIds.has(localQuestionId(p))"
            @click.stop="rev.toggleQuestion(localQuestionId(p))"
          />
          <img :src="imgSrc(p)!" loading="lazy" alt="" />
          <div class="info">
            <div class="t mono" :title="p">{{ p }}</div>
          </div>
          <VButton
            class="mini"
            title="移除"
            @click.stop="rev.removeLocalImage(p)"
            >×</VButton
          >
        </li>
      </ul>
    </div>
    <div v-if="dragOver" class="drop-hint">松开：加为本地图片题</div>
  </div>
</template>

<style scoped>
.bank {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.bank.dragover {
  outline: 2px dashed var(--accent);
  outline-offset: -4px;
}
.drop-hint {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-weight: 600;
  font-size: 14px;
  pointer-events: none;
}
.filters {
  padding: 8px 10px;
  border-bottom: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.row input:not([type="checkbox"]) {
  flex: 1;
  min-width: 80px;
}
.group {
  display: inline-flex;
  gap: 6px;
}
label {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  white-space: nowrap;
  cursor: pointer;
}
.list {
  list-style: none;
  margin: 0;
  padding: 4px;
  overflow: auto;
  flex: 1;
  min-height: 0;
}
.list li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 6px;
  border-radius: 6px;
  cursor: pointer;
}
.list li:hover {
  background: var(--bg);
}
.list li.on {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
}
.list img,
.noimg {
  width: 48px;
  height: 48px;
  object-fit: cover;
  border-radius: 4px;
  background: var(--bg);
  flex: 0 0 48px;
}
.noimg {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: var(--muted);
}
.info {
  min-width: 0;
  flex: 1;
}
.t {
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.badge {
  display: inline-block;
  padding: 0 5px;
  margin-right: 4px;
  font-size: 10px;
  line-height: 15px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--line);
  color: var(--muted);
  vertical-align: 1px;
}
.badge.prose {
  color: var(--paid);
  border-color: var(--paid);
}
.small {
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.local {
  border-top: 1px solid var(--line);
  padding: 8px 10px;
  font-size: 12px;
  max-height: 40%;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.local-list {
  padding: 0;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
}
.mini {
  padding: 1px 8px;
  font-size: 11px;
}
.dim {
  color: var(--muted);
}
</style>
