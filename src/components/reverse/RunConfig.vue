<script setup lang="ts">
import VCost from "../../ui/VCost.vue";
import VButton from "../../ui/VButton.vue";
import { computed, onMounted, onBeforeUnmount, reactive, ref, watch } from "vue";
import { RouterLink } from "vue-router";
import { usePluginModels } from "../../stores/plugin-models";
import { useReverse } from "../../stores/reverse";
import { useStyle } from "../../stores/style";
import ProgressBar from "../ProgressBar.vue";
import ProviderManager from "./ProviderManager.vue";
import { DECISION_LABEL } from "../../lib/decision";
import {
  PROVIDER_KIND_LABEL,
  providerKind,
  providerSubtitle,
  providerTitle,
} from "../../lib/provider";
import { ALL_DECISIONS } from "../../lib/query";
import {
  BATCH_ARTIST_MODES,
  RENDER_ARTIST_MODE_HINT,
  RENDER_ARTIST_MODE_LABEL,
  STYLE_FALLBACKS,
  STYLE_FALLBACK_HINT,
  STYLE_FALLBACK_LABEL,
  STYLE_INDEX_MISSING_NOTE,
  STYLE_RENDER_TOP_K_MAX,
  STYLE_RENDER_TOP_K_MIN,
  judgeOptions,
  renderCountOptions,
  renderSeedError,
  styleMinScoreError,
} from "../../lib/reverse-summary";
import { styleReady, styleStatusLine } from "../../lib/style";
import type { Decision } from "../../types";

const rev = useReverse();
const style = useStyle();

// ---- 模板 ----
const templateId = computed({
  get: () => rev.templateId ?? "",
  set: (v: number | "") => {
    rev.templateId = v === "" ? null : Number(v);
  },
});
const editingTemplate = ref(false);
const tForm = reactive({ name: "", body: "", note: "" });
function newTemplateVersion(blank = false) {
  const cur = rev.currentTemplate;
  tForm.name = blank ? "" : (cur?.name ?? "");
  tForm.body = blank ? "" : (cur?.body ?? "");
  tForm.note = "";
  editingTemplate.value = true;
}
async function saveTemplate() {
  if (!tForm.name.trim() || !tForm.body.trim()) return;
  const saved = await rev.saveTemplate({
    name: tForm.name.trim(),
    body: tForm.body,
    parent_id:
      rev.currentTemplate && rev.currentTemplate.name === tForm.name.trim()
        ? rev.currentTemplate.template_id
        : null,
    note: tForm.note.trim() || undefined,
  });
  if (saved) editingTemplate.value = false;
}

// ---- 规则集 ----
const rulesetId = computed({
  get: () => rev.rulesetId ?? "",
  set: (v: number | "") => {
    rev.rulesetId = v === "" ? null : Number(v);
  },
});
const editingRuleset = ref(false);
const rForm = reactive({
  name: "",
  banned: "",
  appearance_regex: "",
  accessory_ok_regex: "",
  refusal_regex: "",
  always_drop: "",
  max_words: 4,
  max_len: 40,
  note: "",
});
function newRulesetVersion(blank = false) {
  const cur = blank ? null : rev.currentRuleset;
  rForm.name = cur?.name ?? "";
  rForm.banned = (cur?.banned ?? []).join("\n");
  rForm.appearance_regex = cur?.appearance_regex ?? "";
  rForm.accessory_ok_regex = cur?.accessory_ok_regex ?? "";
  rForm.refusal_regex = cur?.refusal_regex ?? "";
  rForm.always_drop = (cur?.always_drop ?? []).join("\n");
  rForm.max_words = cur?.max_words ?? 4;
  rForm.max_len = cur?.max_len ?? 40;
  rForm.note = "";
  editingRuleset.value = true;
}
function splitLines(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);
}
const regexError = computed(() => {
  for (const [k, v] of [
    ["appearance_regex", rForm.appearance_regex],
    ["accessory_ok_regex", rForm.accessory_ok_regex],
    ["refusal_regex", rForm.refusal_regex],
  ] as const) {
    if (!v) continue;
    try {
      new RegExp(v);
    } catch (e) {
      return `${k}：${(e as Error).message}`;
    }
  }
  return null;
});
async function saveRuleset() {
  if (!rForm.name.trim() || regexError.value) return;
  const saved = await rev.saveRuleset({
    name: rForm.name.trim(),
    banned: splitLines(rForm.banned),
    appearance_regex: rForm.appearance_regex,
    accessory_ok_regex: rForm.accessory_ok_regex,
    refusal_regex: rForm.refusal_regex,
    always_drop: splitLines(rForm.always_drop),
    max_words: Math.max(1, Math.round(rForm.max_words) || 1),
    max_len: Math.max(1, Math.round(rForm.max_len) || 1),
    note: rForm.note.trim() || undefined,
  });
  if (saved) editingRuleset.value = false;
}

// ---- 模型与运行 ----
const showProviders = ref(false);
const note = ref("");
const enabledProviders = computed(() => rev.providers.filter((p) => p.enabled));
const progress = computed(() => {
  const p = rev.runProgress;
  if (!p || !p.total) return null;
  return p.done / p.total;
});
const progressLabel = computed(() => {
  const p = rev.runProgress;
  if (!p) return "正在启动…";
  const status = rev.cancelling
    ? "停止中…"
    : {
        started: "运行中",
        run_done: "运行中",
        run_failed: "本条失败",
        batch_done: "已完成",
        batch_failed: "批次失败",
        batch_cancelled: "已停止",
      }[p.status];
  return `${p.done}/${p.total} · ${status}${p.message ? " · " + p.message : ""}`;
});
watch(showProviders, (v) => {
  if (!v) void rev.loadConfig();
});

// ---- 重画对照（CONTRACT §11.4 / §12.6）----
// 表单存在 store（rev.renderForm），runBatch 时变成 RunBatchRequest 的 render / render_character / render_seed / judge_provider_id
// 与 §12.6 的 render_artist_mode（批次只有 画风池随机 / 不带画风；"指定画师"只在底部「画风反推」用）/ render_count（插件默认 / 1–4）。
// 成对评委下拉：第一次勾上"重画"时取一次 plugin_providers()，可刷新；不可达栏内灰字给原因，不 toast（同模型管理对话框）。
const pluginModels = usePluginModels();
const judgeIds = computed(() => pluginModels.ready ? pluginModels.ids : null);
const judgeError = computed(() => pluginModels.error);
const judgeLoading = computed(() => pluginModels.loading);
function missingPlugin(p: { kind: string; plugin_provider_id?: string | null }) { return p.kind === "plugin" && (!pluginModels.ready || !pluginModels.ids.includes(p.plugin_provider_id ?? "")); }
const selectedModelIssue = computed(() => {
  const ids = rev.providers.filter((p) => rev.providerIds.includes(p.provider_id) && p.kind === "plugin").map((p) => p.plugin_provider_id ?? "");
  if (rev.renderForm.render && rev.renderForm.judge_provider_id) ids.push(rev.renderForm.judge_provider_id);
  return ids.length ? pluginModels.issue(ids) : null;
});
async function run() {
  await pluginModels.refresh();
  if (!runBlocked.value) await rev.runBatch(note.value);
}
const judgeOpts = computed(() =>
  judgeOptions(judgeIds.value, rev.renderForm.judge_provider_id),
);
const countOpts = renderCountOptions();
const seedError = computed(() => renderSeedError(rev.renderForm.seed));
// ---- 精准反推（§12.7）：画师来源 = style 时展开子表单（top_k / 决策范围 / 最低相似度 / 兜底 / 减库均值）----
// 索引未建 / 模型未下载只提示不挡：后端在批次开始前就 bad_request、不消耗点数，且 store 里的状态可能是旧的；状态读不到（null）不提示。
const styleMode = computed(
  () => rev.renderForm.render && rev.renderForm.artist_mode === "style",
);
const minScoreError = computed(() =>
  styleMode.value ? styleMinScoreError(rev.renderForm.style.min_score) : null,
);
const styleModelStatus = computed(() =>
  rev.renderForm.style.model
    ? (style.models.find((m) => m.model === rev.renderForm.style.model) ?? null)
    : style.status,
);
const styleIndexMissing = computed(
  () =>
    styleMode.value &&
    styleModelStatus.value !== null &&
    !styleReady(styleModelStatus.value),
);
const styleStatusText = computed(() =>
  styleModelStatus.value
    ? `${styleModelStatus.value.label} · ${styleStatusLine(styleModelStatus.value)}`
    : "",
);
watch(
  [
    () => rev.renderForm.style.model,
    () => styleModelStatus.value?.model,
    () => styleModelStatus.value?.default_center,
  ],
  () => {
    if (styleModelStatus.value)
      rev.renderForm.style.center = styleModelStatus.value.default_center;
  },
);
function toggleStyleDecision(d: Decision) {
  const s = new Set(rev.renderForm.style.decisions);
  if (s.has(d)) s.delete(d);
  else s.add(d);
  rev.renderForm.style.decisions = ALL_DECISIONS.filter((x) => s.has(x));
}
watch(
  styleMode,
  (on) => {
    if (on && !style.statusLoading) void style.refreshStatus();
  },
  { immediate: true },
);
const runBlocked = computed(
  () =>
    !rev.canRun ||
    !!selectedModelIssue.value ||
    (rev.renderForm.render &&
      (seedError.value !== null || minScoreError.value !== null)),
);
/** 运行按钮上的后缀：`· 重画` / `· 重画×3`，精准反推再加 `· 检索画风`。 */
const renderSuffix = computed(() =>
  rev.renderForm.render
    ? ` · 重画${rev.renderForm.count ? `×${rev.renderForm.count}` : ""}${styleMode.value ? " · 检索画风" : ""}`
    : "",
);
async function loadJudgeProviders() { await pluginModels.refresh(); }
function onFocus() { void loadJudgeProviders(); }
onMounted(() => { void loadJudgeProviders(); window.addEventListener("focus", onFocus); });
onBeforeUnmount(() => window.removeEventListener("focus", onFocus));

watch(
  () => rev.renderForm.render,
  (on) => {
    if (on && judgeIds.value === null && !judgeLoading.value)
      void loadJudgeProviders();
  },
  { immediate: true },
);
</script>

<template>
  <div class="config">
    <div v-if="rev.configError" class="read-error" role="alert">
      配置读取失败：{{ rev.configError
      }}<VButton @click="rev.loadConfig">重试</VButton>
    </div>
    <div v-else-if="rev.configLoading" class="dim" aria-busy="true">
      读取配置…
    </div>
    <section class="blk">
      <div class="blk-head">
        模板
        <VButton
          class="mini"
          @click="newTemplateVersion(false)"
          :disabled="!rev.currentTemplate"
          >新建版本</VButton
        >
        <VButton class="mini" @click="newTemplateVersion(true)"
          >空白模板</VButton
        >
      </div>
      <select v-model="templateId">
        <option
          v-for="t in rev.sortedTemplates"
          :key="t.template_id"
          :value="t.template_id"
        >
          {{ t.name }} v{{ t.version }}
        </option>
      </select>
      <div v-if="rev.currentTemplate && !editingTemplate" class="preview">
        <pre>{{ rev.currentTemplate.body }}</pre>
        <div class="dim small">{{ rev.currentTemplate.note ?? "" }}</div>
      </div>
      <div v-if="editingTemplate" class="editor">
        <label>名称（同名自动 version+1） <input v-model="tForm.name" /></label>
        <textarea v-model="tForm.body" rows="12" spellcheck="false" />
        <label>备注 <input v-model="tForm.note" /></label>
        <div class="row">
          <VButton class="primary" @click="saveTemplate">保存为新版本</VButton
          ><VButton @click="editingTemplate = false">取消</VButton>
        </div>
      </div>
    </section>

    <section class="blk">
      <div class="blk-head">
        规则集
        <VButton
          class="mini"
          @click="newRulesetVersion(false)"
          :disabled="!rev.currentRuleset"
          >新建版本</VButton
        >
        <VButton class="mini" @click="newRulesetVersion(true)">空白</VButton>
      </div>
      <select v-model="rulesetId">
        <option
          v-for="r in rev.sortedRulesets"
          :key="r.ruleset_id"
          :value="r.ruleset_id"
        >
          {{ r.name }} v{{ r.version }}
        </option>
      </select>
      <div v-if="rev.currentRuleset && !editingRuleset" class="preview small">
        <div>
          banned {{ rev.currentRuleset.banned.length }} 项 · always_drop
          {{ rev.currentRuleset.always_drop.length }} 项 · max_words
          {{ rev.currentRuleset.max_words }} · max_len
          {{ rev.currentRuleset.max_len }}
        </div>
        <div class="dim mono">
          appearance: {{ rev.currentRuleset.appearance_regex || "—" }}
        </div>
        <div class="dim mono">
          accessory_ok: {{ rev.currentRuleset.accessory_ok_regex || "—" }}
        </div>
        <div class="dim mono">
          refusal: {{ rev.currentRuleset.refusal_regex || "—" }}
        </div>
      </div>
      <div v-if="editingRuleset" class="editor">
        <label>名称 <input v-model="rForm.name" /></label>
        <label
          >banned（每行一个，子串匹配）
          <textarea v-model="rForm.banned" rows="5" spellcheck="false" />
        </label>
        <label
          >appearance_regex
          <input
            v-model="rForm.appearance_regex"
            class="mono"
            spellcheck="false"
        /></label>
        <label
          >accessory_ok_regex
          <input
            v-model="rForm.accessory_ok_regex"
            class="mono"
            spellcheck="false"
        /></label>
        <label
          >refusal_regex
          <input v-model="rForm.refusal_regex" class="mono" spellcheck="false"
        /></label>
        <label
          >always_drop（每行一个）
          <textarea v-model="rForm.always_drop" rows="3" spellcheck="false" />
        </label>
        <div class="row">
          <label
            >max_words
            <input v-model.number="rForm.max_words" type="number" min="1"
          /></label>
          <label
            >max_len
            <input v-model.number="rForm.max_len" type="number" min="1"
          /></label>
        </div>
        <label>备注 <input v-model="rForm.note" /></label>
        <div v-if="regexError" class="err">{{ regexError }}</div>
        <div class="row">
          <VButton class="primary" :disabled="!!regexError" @click="saveRuleset"
            >保存为新版本</VButton
          ><VButton @click="editingRuleset = false">取消</VButton>
        </div>
      </div>
    </section>

    <section class="blk">
      <div class="blk-head">
        模型 <VButton class="mini" @click="showProviders = true">管理</VButton>
      </div>
      <div class="plist">
        <label
          v-for="p in enabledProviders"
          :key="p.provider_id"
          :class="{ nokey: !p.has_key }"
          :title="providerTitle(p)"
        >
          <input
            type="checkbox"
            :checked="rev.providerIds.includes(p.provider_id)"
            :disabled="missingPlugin(p) && !rev.providerIds.includes(p.provider_id)"
            @change="rev.toggleProvider(p.provider_id)"
          />
          {{ p.name }} <span v-if="missingPlugin(p)" class="dim small"> · 名单外或尚未读取</span>
          <span class="badge" :class="providerKind(p)">{{
            PROVIDER_KIND_LABEL[providerKind(p)]
          }}</span>
          <span class="dim small">{{ providerSubtitle(p) }}</span>
        </label>
        <div v-if="!enabledProviders.length" class="dim small">
          没有启用的模型，先去「管理」添加。
        </div>
      </div>
    </section>

    <section class="blk render">
      <div class="blk-head">重画对照</div>
      <label
        class="chk"
        title="每条解析成功的 run 都交给插件按同一角色重画并打质量分（尺寸按原图比例、画师来源与张数按下面选的，与 插件 /反推 同一套函数）；有原图时再跑成对评委给「像原图」分。每张消耗 NovelAI 点数；是否消耗 Anlas 取决于插件生成设置"
      >
        <input v-model="rev.renderForm.render" type="checkbox" />
        反推后重画并评分
      </label>
      <template v-if="rev.renderForm.render">
        <div class="row">
          <label class="fld"
            >角色
            <input
              v-model="rev.renderForm.character"
              placeholder="薇拉"
              title="留空 = 薇拉；与 /画 同款解析（角色池名 / 别名 / 随机）"
          /></label>
          <label class="fld"
            >固定种子
            <input
              v-model="rev.renderForm.seed"
              :class="{ bad: seedError }"
              inputmode="numeric"
              placeholder="随机"
              title="空 = 每张随机并回传；填了则整批同一个种子"
          /></label>
        </div>
        <div v-if="seedError" class="err">{{ seedError }}</div>
        <label class="fld"
          >成对评委
          <span class="row">
            <select
              v-model="rev.renderForm.judge_provider_id"
              title="像原图 分由哪个 provider 打；插件默认 = 插件里配置的默认评委"
            >
              <option v-for="o in judgeOpts" :key="o.value" :value="o.value" :disabled="o.disabled">
                {{ o.label }}
              </option>
            </select>
            <VButton
              class="mini"
              :disabled="judgeLoading"
              @click="loadJudgeProviders"
              >{{ judgeLoading ? "取中…" : "刷新" }}</VButton
            >
          </span>
        </label>
        <div v-if="judgeError" class="dim small">
          插件不可达：{{ judgeError }}
        </div>
        <div class="row">
          <label class="fld"
            >画师来源
            <select
              v-model="rev.renderForm.artist_mode"
              :title="RENDER_ARTIST_MODE_HINT[rev.renderForm.artist_mode]"
            >
              <option
                v-for="m in BATCH_ARTIST_MODES"
                :key="m"
                :value="m"
                :title="RENDER_ARTIST_MODE_HINT[m]"
              >
                {{ RENDER_ARTIST_MODE_LABEL[m] }}
              </option>
            </select>
          </label>
          <label class="fld"
            >张数
            <select
              v-model="rev.renderForm.count"
              title="每条 run 出几张：全部评分、只把最高分那张当重画图（与 /反推 相同）；插件默认 = 插件配置 reverse_draw.candidates（通常 2）。每张消耗点数，可能消耗 Anlas"
            >
              <option
                v-for="o in countOpts"
                :key="String(o.value)"
                :value="o.value"
              >
                {{ o.label }}
              </option>
            </select>
          </label>
        </div>
        <div v-if="styleMode" class="style-blk">
          <p class="dim small">
            桌面批次目前取向量排名第一的可用画风；插件中的候选复审暂不作用于此页。
          </p>
          <label class="fld"
            >画风检索模型
            <select
              v-model="rev.renderForm.style.model"
              :disabled="rev.running"
              title="启动批次后固定模型，后续切换当前模型不改变此批次"
            >
              <option :value="null">
                采用启动批次时的当前模型{{
                  style.status ? `（${style.status.label}）` : ""
                }}
              </option>
              <option v-for="m in style.models" :key="m.model" :value="m.model">
                {{ m.label }} ·
                {{
                  !m.present
                    ? "未下载"
                    : m.indexed
                      ? `已索引 ${m.indexed}`
                      : "未建索引"
                }}
              </option>
            </select>
          </label>
          <div class="row">
            <label class="fld"
              >检索候选 top_k
              <input
                v-model.number="rev.renderForm.style.top_k"
                type="number"
                :min="STYLE_RENDER_TOP_K_MIN"
                :max="STYLE_RENDER_TOP_K_MAX"
                title="画风索引里取前几条候选（1–20，缺省 5），按决策范围过滤后用第一条有画师串的"
              />
            </label>
            <label class="fld"
              >最低相似度
              <input
                v-model="rev.renderForm.style.min_score"
                :class="{ bad: minScoreError }"
                inputmode="decimal"
                placeholder="不设门槛"
                title="0–1 的余弦相似度；命中低于它时按兜底方式处理。留空 = 不设门槛。不同模型的分数不可直接比较，请按各模型的对照结果设置。"
              />
            </label>
          </div>
          <div v-if="minScoreError" class="err">{{ minScoreError }}</div>
          <span class="fld"
            >决策范围
            <span class="group">
              <label v-for="d in ALL_DECISIONS" :key="d"
                ><input
                  type="checkbox"
                  :checked="rev.renderForm.style.decisions.includes(d)"
                  @change="toggleStyleDecision(d)"
                />
                {{ DECISION_LABEL[d] }}</label
              >
              <span class="dim small"
                >一个都不勾 = 全部；黑名单命中的永不用</span
              >
            </span>
          </span>
          <div class="row">
            <label class="fld"
              >兜底
              <select
                v-model="rev.renderForm.style.fallback"
                :title="STYLE_FALLBACK_HINT[rev.renderForm.style.fallback]"
              >
                <option
                  v-for="f in STYLE_FALLBACKS"
                  :key="f"
                  :value="f"
                  :title="STYLE_FALLBACK_HINT[f]"
                >
                  {{ STYLE_FALLBACK_LABEL[f] }}
                </option>
              </select>
            </label>
            <label
              class="chk"
              title="查询向量与画风词典向量减去相同的库均值，再计算余弦相似度。"
              ><input v-model="rev.renderForm.style.center" type="checkbox" />
              减库均值</label
            >
          </div>
          <div v-if="styleIndexMissing" class="notice">
            <span
              >{{ STYLE_INDEX_MISSING_NOTE
              }}<span v-if="styleStatusText" class="dim"
                >（{{ styleStatusText }}）</span
              ></span
            >
            <RouterLink to="/settings" class="link">去设置页 →</RouterLink>
            <VButton
              class="mini"
              :disabled="style.statusLoading"
              @click="style.refreshStatus"
              >{{ style.statusLoading ? "刷新中…" : "刷新" }}</VButton
            >
          </div>
          <div v-else-if="styleStatusText" class="dim small">
            画风索引：{{ styleStatusText }}
          </div>
        </div>
        <div class="dim small">
          「指定画师」只在底部「画风反推」的“用它重画”里用；「精准反推」= 每条
          run
          自动检索最像的画风并翻译成指定画师（§12.7）；尺寸按原图宽高比选（与插件
          /反推
          同一函数）。散文题与本地图片题也会重画；两个分数在结果表与单题面板里看。
        </div>
      </template>
    </section>

    <div class="cost-line">
      <VCost kind="model" label="反推 / 评分调用模型" /><template
        v-if="rev.renderForm.render"
        ><VCost kind="free" label="重画消耗点数" /><VCost
          kind="paid"
          label="可能消耗 Anlas"
      /></template>
    </div>
    <p v-if="selectedModelIssue" class="read-error" role="alert">{{ selectedModelIssue }}</p>
    <section class="blk run" :class="{ active: rev.running }">
      <input
        v-if="!rev.running"
        v-model="note"
        placeholder="批次备注（可空）"
      />
      <VButton
        v-if="!rev.running"
        class="primary"
        :disabled="runBlocked"
        @click="run"
      >
        运行 batch（{{ rev.selectedCount }} 题 ×
        {{ rev.providerIds.length }} 模型{{ renderSuffix }}）
      </VButton>
      <VButton
        v-else
        class="stop"
        :disabled="!rev.activeBatchId || rev.cancelling"
        @click="rev.cancelBatch()"
      >
        {{
          rev.cancelling
            ? "停止中…"
            : !rev.activeBatchId
              ? "正在启动…"
              : "停止运行"
        }}
      </VButton>
      <div v-if="rev.running" class="dim small">
        保存已完成结果；已发出的远程请求可能仍会完成。
      </div>
      <ProgressBar
        v-if="rev.running || rev.runProgress"
        :value="progress"
        :danger="rev.runProgress?.status === 'batch_failed'"
        :label="progressLabel"
      />
    </section>

    <ProviderManager v-if="showProviders" @close="showProviders = false" />
  </div>
</template>

<style scoped>
.config {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 10px;
  overflow: auto;
  height: 100%;
  font-size: 13px;
}
.blk {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.blk-head {
  font-weight: 600;
  font-size: 12px;
  color: var(--muted);
  display: flex;
  align-items: center;
  gap: 6px;
}
.preview pre {
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 11px;
  background: var(--bg);
  padding: 8px;
  border-radius: var(--radius);
  max-height: 160px;
  overflow: auto;
  margin: 0;
}
.preview.small {
  font-size: 12px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.editor {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.editor label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12px;
}
.editor textarea {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  resize: vertical;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.row label {
  flex: 1;
}
.plist {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.plist label {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
.plist label.nokey {
  opacity: 0.6;
}
.badge {
  display: inline-block;
  padding: 0 5px;
  font-size: 10px;
  line-height: 15px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--line);
  color: var(--muted);
}
.badge.plugin {
  color: var(--accent);
  border-color: var(--accent);
}
.run {
  gap: 8px;
}
.run.active {
  position: sticky;
  bottom: 0;
  z-index: 1;
  flex-shrink: 0;
  background: var(--panel);
  padding: 10px 0 2px;
  border-top: 1px solid var(--line);
}
.run .stop {
  background: var(--danger);
  border-color: var(--danger);
  color: var(--on-danger);
  font-weight: 600;
}
.run :deep(.pb-label) {
  overflow-wrap: anywhere;
}
.render .chk {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
.render .fld {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12px;
  flex: 1;
  min-width: 0;
}
.render .fld > .row {
  display: flex;
  gap: 6px;
}
.render .fld select {
  flex: 1;
  min-width: 0;
}
.render input.bad {
  border-color: var(--danger);
}
.style-blk {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 6px 8px;
  border: 1px dashed var(--line);
  border-radius: var(--radius);
}
.style-blk .row {
  align-items: flex-end;
}
.style-blk .group {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  color: var(--ink);
}
.style-blk .group label {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  white-space: nowrap;
  cursor: pointer;
  flex: 0 0 auto;
}
.style-blk .fld > .group {
  font-size: 12px;
}
.style-blk .chk {
  flex: 0 0 auto;
  align-self: flex-end;
  padding-bottom: 4px;
}
.notice {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--paid);
  background: var(--paid-soft);
  padding: 6px 10px;
  border-radius: var(--radius);
}
.link {
  color: var(--accent);
  font-weight: 600;
  text-decoration: none;
  cursor: pointer;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mini {
  padding: 1px 8px;
  font-size: 11px;
}
.dim {
  color: var(--muted);
}
.small {
  font-size: 11px;
}
.err {
  color: var(--danger);
  font-size: 12px;
}
</style>
