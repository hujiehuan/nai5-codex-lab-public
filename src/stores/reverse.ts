import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api, Channel, errMessage } from "../api";
import type {
  BatchResults,
  BatchSummary,
  CompareResult,
  Decision,
  Question,
  QuestionQuery,
  Rating,
  ReverseProvider,
  ReverseProviderInput,
  ReverseRuleset,
  ReverseRulesetInput,
  ReverseTemplate,
  ReverseTemplateInput,
  RenderArtistMode,
  RunDetail,
  RunEvent,
  RunRow,
} from "../types";
import { NON_IMAGE_TOAST, splitDropPaths, type DropSplit } from "../lib/dnd";
import { questionIsProse } from "../lib/prose";
import {
  defaultRenderForm,
  renderRequestFields,
  replaceRun,
  rerenderParams,
  type RenderForm,
} from "../lib/reverse-summary";
import { useToast } from "./toast";

export interface QuestionFilterState {
  codex_id: string;
  rating: Rating[];
  path_prefix: string; // 用 / 分隔
  decision: Decision[];
  main_image_only: boolean;
  keyword: string;
  limit: number;
  /** v0.2.4（§11.4）"只看标签题"：纯前端过滤（lib/prose.ts 判散文），不进 QuestionQuery、切换不重拉。 */
  tags_only: boolean;
}

export function localQuestionId(path: string): string {
  return `local:${path}`;
}

export function buildQuestionQuery(f: QuestionFilterState): QuestionQuery {
  const q: QuestionQuery = {
    main_image_only: f.main_image_only,
    limit: f.limit,
    offset: 0,
  };
  if (f.codex_id) q.codex_id = f.codex_id;
  if (f.rating.length) q.rating = [...f.rating];
  const prefix = f.path_prefix
    .split("/")
    .map((s) => s.trim())
    .filter(Boolean);
  if (prefix.length) q.path_prefix = prefix;
  if (f.decision.length) q.decision = [...f.decision];
  if (f.keyword.trim()) q.keyword = f.keyword.trim();
  return q;
}

/** 同名模板/规则集取最高版本，按名字分组后排序。 */
export function sortVersioned<T extends { name: string; version: number }>(
  list: T[],
): T[] {
  return [...list].sort((a, b) =>
    a.name === b.name ? b.version - a.version : a.name.localeCompare(b.name),
  );
}

export const useReverse = defineStore("reverse", () => {
  const toast = useToast();

  const templates = ref<ReverseTemplate[]>([]);
  const rulesets = ref<ReverseRuleset[]>([]);
  const providers = ref<ReverseProvider[]>([]);
  const configLoading = ref(false);

  const questions = ref<Question[]>([]);
  const questionsLoading = ref(false);
  const questionFilter = ref<QuestionFilterState>({
    codex_id: "nai5_community_pack",
    rating: ["safe"],
    path_prefix: "",
    decision: [],
    main_image_only: true,
    keyword: "",
    limit: 200,
    tags_only: false,
  });
  const localImages = ref<string[]>([]);
  const selectedQuestionIds = ref<Set<string>>(new Set());

  const templateId = ref<number | null>(null);
  const rulesetId = ref<number | null>(null);
  const providerIds = ref<number[]>([]);
  /**
   * v0.2.4（§11.4）RunConfig 的重画表单；runBatch 时经 renderRequestFields 变成 RunBatchRequest 的重画键（§12.6 含画师来源与张数，
   * §12.7 含精准反推的六个 style_* 键）。会话内保留，不写 localStorage（与画风反推面板的表单一样）。
   */
  const renderForm = ref<RenderForm>(defaultRenderForm());
  /** 正在单条重画的 run_id（同一时刻只放一个），null = 空闲。 */
  const rerenderingRunId = ref<number | null>(null);
  /**
   * v0.3.0（§12.5）"当前 run"：最近在单题面板打开的那条（面板是模态的，关掉后仍记住；换批次清掉）。
   * 画风反推面板的"用它重画"打到它身上，来源下拉也把它的原图排最前。
   */
  const currentRun = ref<RunRow | null>(null);

  const batches = ref<BatchSummary[]>([]);
  const currentBatchId = ref<string | null>(null);
  const results = ref<BatchResults | null>(null);
  const resultsLoading = ref(false);
  const runDetail = ref<RunDetail | null>(null);
  const runDetailLoading = ref(false);
  const runDetailError = ref("");
  let detailSequence = 0;

  const running = ref(false);
  /** 正在执行的批次与结果栏正在查看的历史批次分开，停止始终指向前者。 */
  const activeBatchId = ref<string | null>(null);
  const cancelling = ref(false);
  const runProgress = ref<RunEvent | null>(null);
  type BatchExecution = {
    batchId: string | null;
    terminal: boolean;
    selectionRevision: number;
    total: number;
  };
  let execution: BatchExecution | null = null;
  let selectionRevision = 0;
  const compare = ref<CompareResult | null>(null);
  const compareLoading = ref(false);
  const compareError = ref("");

  const sortedTemplates = computed(() => sortVersioned(templates.value));
  const sortedRulesets = computed(() => sortVersioned(rulesets.value));
  const currentTemplate = computed(
    () =>
      templates.value.find((t) => t.template_id === templateId.value) ?? null,
  );
  const currentRuleset = computed(
    () => rulesets.value.find((r) => r.ruleset_id === rulesetId.value) ?? null,
  );
  const selectedCount = computed(() => selectedQuestionIds.value.size);
  /** 题库列表实际显示的题："只看标签题"时去掉散文题（§11.4）。 */
  const visibleQuestions = computed(() =>
    questionFilter.value.tags_only
      ? questions.value.filter((q) => !questionIsProse(q))
      : questions.value,
  );
  /** 当前拉到的题里散文题的数量（给筛选行的提示）。 */
  const proseCount = computed(
    () => questions.value.filter((q) => questionIsProse(q)).length,
  );
  const canRun = computed(
    () =>
      !running.value &&
      selectedQuestionIds.value.size > 0 &&
      templateId.value !== null &&
      rulesetId.value !== null &&
      providerIds.value.length > 0,
  );

  const configError = ref("");
  const questionsError = ref("");
  const batchesError = ref("");
  const resultsError = ref("");
  const batchesLoading = ref(false);
  async function loadConfig() {
    configError.value = "";
    configLoading.value = true;
    try {
      const [t, r, p] = await Promise.all([
        api.reverseTemplatesList(),
        api.reverseRulesetsList(),
        api.reverseProvidersList(),
      ]);
      if (![t, r, p].every(Array.isArray)) throw Error("实验配置列表格式错误");
      templates.value = t;
      rulesets.value = r;
      providers.value = p;
      if (
        templateId.value === null ||
        !t.some((x) => x.template_id === templateId.value)
      )
        templateId.value = sortVersioned(t)[0]?.template_id ?? null;
      if (
        rulesetId.value === null ||
        !r.some((x) => x.ruleset_id === rulesetId.value)
      )
        rulesetId.value = sortVersioned(r)[0]?.ruleset_id ?? null;
      providerIds.value = providerIds.value.filter((id) =>
        p.some((x) => x.provider_id === id && x.enabled),
      );
    } catch (e) {
      configError.value = errMessage(e);
      toast.error(e, "读取实验配置失败");
    } finally {
      configLoading.value = false;
    }
  }

  async function loadBatches() {
    batchesError.value = "";
    batchesLoading.value = true;
    try {
      const data = await api.reverseBatchesList();
      if (!Array.isArray(data)) throw Error("批次列表格式错误");
      batches.value = data;
      // 终止事件丢失时查数据库恢复，包括取消状态；浏览历史不会改变运行状态。
      const ctx = execution;
      if (running.value && ctx?.batchId && !ctx.terminal) {
        const b = batches.value.find((x) => x.batch_id === ctx.batchId);
        const status =
          b?.status === "done"
            ? "batch_done"
            : b?.status === "failed"
              ? "batch_failed"
              : b?.status === "cancelled"
                ? "batch_cancelled"
                : null;
        if (status && b) {
          let saved: BatchResults | undefined;
          try {
            saved = await api.reverseBatchResults(ctx.batchId);
          } catch {
            /* 仍用已收到的进度恢复操作按钮。 */
          }
          if (execution !== ctx || ctx.terminal) return;
          const total = runProgress.value?.total ?? ctx.total;
          finishBatch(
            ctx,
            {
              batch_id: ctx.batchId,
              run_id: null,
              status,
              done:
                status === "batch_done"
                  ? total
                  : Math.max(
                      runProgress.value?.done ?? 0,
                      saved?.runs.length ?? 0,
                    ),
              total,
              message: null,
            },
            saved,
          );
        }
      }
    } catch (e) {
      batchesError.value = errMessage(e);
      toast.error(e, "读取批次失败");
    } finally {
      batchesLoading.value = false;
    }
  }

  async function loadAll() {
    await Promise.all([loadConfig(), loadBatches()]);
  }

  async function loadQuestions() {
    questionsError.value = "";
    questionsLoading.value = true;
    try {
      const data = await api.reverseQuestions(
        buildQuestionQuery(questionFilter.value),
      );
      if (!Array.isArray(data)) throw Error("题目列表格式错误");
      questions.value = data;
    } catch (e) {
      questionsError.value = errMessage(e);
      toast.error(e, "读取题库失败");
    } finally {
      questionsLoading.value = false;
    }
  }

  function setQuestionFilter(patch: Partial<QuestionFilterState>) {
    questionFilter.value = { ...questionFilter.value, ...patch };
    // tags_only 是纯前端过滤，只改它不重拉
    if (Object.keys(patch).some((k) => k !== "tags_only")) void loadQuestions();
  }

  function toggleQuestion(id: string) {
    const s = new Set(selectedQuestionIds.value);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selectedQuestionIds.value = s;
  }
  /** 全选只勾当前显示的题（"只看标签题"时不会把散文题一起勾进去）。 */
  function selectAllQuestions() {
    const s = new Set(selectedQuestionIds.value);
    for (const q of visibleQuestions.value) s.add(q.question_id);
    selectedQuestionIds.value = s;
  }
  function clearQuestions() {
    selectedQuestionIds.value = new Set();
  }

  /** 把一个本地绝对路径加进题库并勾选；已在列表里的只勾选不重复加。"添加本地图片"与拖放（§10.3）共用这一条路。 */
  async function addLocalImagePath(path: string) {
    if (!path) return;
    // v0.2.5：先拷进数据目录（asset 协议范围内），拿回稳定路径再入题库
    let stored = path;
    try {
      stored = await api.reverseImportLocalImage(path);
    } catch (e) {
      toast.error(e, "导入本地图片失败");
      return;
    }
    if (!localImages.value.includes(stored))
      localImages.value = [...localImages.value, stored];
    const s = new Set(selectedQuestionIds.value);
    s.add(localQuestionId(stored));
    selectedQuestionIds.value = s;
  }
  async function addLocalImage() {
    try {
      const path = await api.reversePickLocalImage();
      if (path) await addLocalImagePath(path);
    } catch (e) {
      toast.error(e, "选择本地图片失败");
    }
  }
  /** §10.3 拖放：png / jpg / jpeg / webp / gif 逐个 addLocalImagePath，其余忽略并提示一次；返回分拣结果给组件 / 测试用。 */
  async function addDroppedPaths(paths: readonly string[]): Promise<DropSplit> {
    const split = splitDropPaths(paths);
    for (const p of split.images) await addLocalImagePath(p);
    if (split.others.length) toast.push(NON_IMAGE_TOAST);
    return split;
  }
  function removeLocalImage(path: string) {
    localImages.value = localImages.value.filter((p) => p !== path);
    const s = new Set(selectedQuestionIds.value);
    s.delete(localQuestionId(path));
    selectedQuestionIds.value = s;
  }

  async function saveTemplate(
    input: ReverseTemplateInput,
  ): Promise<ReverseTemplate | null> {
    try {
      const saved = await api.reverseTemplateSave(input);
      await loadConfig();
      templateId.value = saved.template_id;
      toast.success(`模板 ${saved.name} v${saved.version} 已保存`);
      return saved;
    } catch (e) {
      toast.error(e, "保存模板失败");
      return null;
    }
  }

  async function saveRuleset(
    input: ReverseRulesetInput,
  ): Promise<ReverseRuleset | null> {
    try {
      const saved = await api.reverseRulesetSave(input);
      await loadConfig();
      rulesetId.value = saved.ruleset_id;
      toast.success(`规则集 ${saved.name} v${saved.version} 已保存`);
      return saved;
    } catch (e) {
      toast.error(e, "保存规则集失败");
      return null;
    }
  }

  async function saveProvider(
    input: ReverseProviderInput,
    apiKey: string,
  ): Promise<ReverseProvider | null> {
    try {
      const saved = await api.reverseProviderSave(input);
      if (apiKey.trim())
        await api.secretSet(`provider:${saved.provider_id}`, apiKey.trim());
      await loadConfig();
      toast.success(`模型 ${saved.name} 已保存`);
      return saved;
    } catch (e) {
      toast.error(e, "保存模型失败");
      return null;
    }
  }

  async function deleteProvider(providerId: number): Promise<boolean> {
    try {
      await api.reverseProviderDelete(providerId);
      providerIds.value = providerIds.value.filter((id) => id !== providerId);
      await loadConfig();
      return true;
    } catch (e) {
      toast.error(e, "删除模型失败");
      return false;
    }
  }

  function toggleProvider(id: number) {
    providerIds.value = providerIds.value.includes(id)
      ? providerIds.value.filter((x) => x !== id)
      : [...providerIds.value, id];
  }

  function bindBatch(ctx: BatchExecution, batchId: string): boolean {
    if (
      execution !== ctx ||
      ctx.terminal ||
      (ctx.batchId !== null && ctx.batchId !== batchId)
    )
      return false;
    if (ctx.batchId === null) {
      ctx.batchId = batchId;
      activeBatchId.value = batchId;
      // 启动时自动选中新批次；等待启动期间若用户已选择历史，则保留他的选择。
      if (selectionRevision === ctx.selectionRevision) {
        currentBatchId.value = batchId;
        currentRun.value = null;
        results.value = null;
      }
    }
    return true;
  }

  function finishBatch(ctx: BatchExecution, e: RunEvent, saved?: BatchResults) {
    if (execution !== ctx || ctx.terminal) return;
    ctx.terminal = true;
    running.value = false;
    cancelling.value = false;
    activeBatchId.value = null;
    runProgress.value = e;
    if (e.status === "batch_cancelled")
      toast.push(`批次已停止：${e.done}/${e.total}，已完成结果已保存`);
    else if (e.status === "batch_done")
      toast.success(`批次完成：${e.done}/${e.total}`);
    else toast.error(e.message ?? "请查看已保存的批次结果", "批次失败");
    if (currentBatchId.value === e.batch_id) {
      if (saved) results.value = saved;
      else void refreshResults();
    }
  }

  async function cancelBatch() {
    const ctx = execution;
    if (!running.value || !ctx?.batchId || ctx.terminal || cancelling.value)
      return;
    cancelling.value = true;
    try {
      await api.reverseCancelBatch(ctx.batchId);
      // 接受停止请求不等于任务结束；保持停止中直到终止事件或轮询确认。
    } catch (e) {
      if (execution !== ctx || ctx.terminal) return;
      cancelling.value = false;
      toast.error(e, "停止批次失败，可重试");
    }
  }

  async function runBatch(note: string) {
    if (!canRun.value || templateId.value === null || rulesetId.value === null)
      return;
    const req = {
      question_ids: [...selectedQuestionIds.value],
      template_id: templateId.value,
      ruleset_id: rulesetId.value,
      provider_ids: [...providerIds.value],
      note: note.trim() || undefined,
      ...renderRequestFields(renderForm.value),
    };
    const ctx: BatchExecution = {
      batchId: null,
      terminal: false,
      selectionRevision,
      total: req.question_ids.length * req.provider_ids.length,
    };
    execution = ctx;
    running.value = true;
    cancelling.value = false;
    activeBatchId.value = null;
    runProgress.value = null;
    const ch = new Channel<RunEvent>();
    ch.onmessage = (e) => {
      if (!bindBatch(ctx, e.batch_id)) return;
      runProgress.value = e;
      if (
        e.status === "batch_done" ||
        e.status === "batch_failed" ||
        e.status === "batch_cancelled"
      ) {
        finishBatch(ctx, e);
        void loadBatches();
      }
    };
    try {
      const { batch_id } = await api.reverseRunBatch(req, ch);
      bindBatch(ctx, batch_id);
    } catch (e) {
      if (execution !== ctx || ctx.terminal) return;
      // started 已到达时后台可能仍在跑，保留停止入口与轮询。
      if (ctx.batchId === null) {
        ctx.terminal = true;
        running.value = false;
        cancelling.value = false;
      }
      toast.error(
        e,
        ctx.batchId ? "启动确认失败，正在核对批次状态" : "启动批次失败",
      );
    }
  }

  async function selectBatch(batchId: string | null) {
    selectionRevision += 1;
    if (batchId !== currentBatchId.value) currentRun.value = null;
    currentBatchId.value = batchId;
    results.value = null;
    if (!batchId) return;
    resultsLoading.value = true;
    resultsError.value = "";
    try {
      const r = await api.reverseBatchResults(batchId);
      if (currentBatchId.value === batchId) results.value = r;
    } catch (e) {
      resultsError.value = errMessage(e);
      toast.error(e, "读取批次结果失败");
    } finally {
      resultsLoading.value = false;
    }
  }

  /** 结果表静默重拉（不清空、不转圈）：单条重画后汇总行的 render_n / quality_avg 要跟着变。失败不提示，行已经就地换过了。 */
  async function refreshResults() {
    const batchId = currentBatchId.value;
    if (!batchId) return;
    try {
      const r = await api.reverseBatchResults(batchId);
      if (r && currentBatchId.value === batchId) results.value = r;
    } catch {
      // 静默：结果行已经用 reverse_render_run 返回的那条换过了
    }
  }

  /**
   * v0.2.4（§11.4）单条重画并评分，覆盖旧结果：角色与固定种子沿用批次当时的，成对评委用表单里当前选的（空 = 插件默认）。
   * v0.3.0（§12.5 / §12.6）：`artist` + `artistMode`——画风反推"用它重画"传 (命中串, "given")；单题面板的"重画"两个都不传，
   * 画师来源与张数沿用实验配置里当前选的（与批次相同）；规则见 lib/reverse-summary.ts rerenderParams。
   * v0.3.1（§12.7）：来源是 style（精准反推）时把子表单的六个 style_* 键一起发；其余模式发契约缺省。
   * 返回的 RunRow 就地换进结果表、打开的详情与当前 run；`render_error` 非空按失败提示（后端不抛，把原因放在行里）。
   */
  async function rerender(
    runId: number,
    artist: string | null = null,
    artistMode: RenderArtistMode | null = null,
  ): Promise<RunRow | null> {
    if (rerenderingRunId.value !== null) return null;
    rerenderingRunId.value = runId;
    try {
      const p = rerenderParams(renderForm.value, artist, artistMode);
      const row = await api.reverseRenderRun(
        runId,
        renderForm.value.judge_provider_id.trim() || null,
        p.artist,
        p.artist_mode,
        p.count,
        p.style,
      );
      if (results.value)
        results.value = {
          ...results.value,
          runs: replaceRun(results.value.runs, row),
        };
      if (runDetail.value?.run.run_id === runId)
        runDetail.value = { ...runDetail.value, run: row };
      if (currentRun.value?.run_id === runId) currentRun.value = row;
      if (row.render_error)
        toast.error(row.render_error, `run #${runId} 重画失败`);
      else toast.success(`run #${runId} 重画完成`);
      await refreshResults();
      return row;
    } catch (e) {
      toast.error(e, `run #${runId} 重画失败`);
      return null;
    } finally {
      rerenderingRunId.value = null;
    }
  }

  async function openRun(runId: number) {
    const generation = ++detailSequence;
    runDetailLoading.value = true;
    runDetailError.value = "";
    runDetail.value = null;
    try {
      const data = await api.reverseRunGet(runId);
      if (generation !== detailSequence) return;
      runDetail.value = data;
      currentRun.value = data.run;
    } catch (e) {
      if (generation === detailSequence) {
        runDetailError.value = errMessage(e);
        toast.error(e, "读取单题失败");
      }
    } finally {
      if (generation === detailSequence) runDetailLoading.value = false;
    }
  }
  function closeRun() {
    ++detailSequence;
    runDetail.value = null;
    runDetailLoading.value = false;
    runDetailError.value = "";
  }

  async function judge(
    runId: number,
    score: number,
    comment: string,
  ): Promise<boolean> {
    try {
      await api.reverseJudge(runId, score, comment);
      if (runDetail.value?.run.run_id === runId) {
        runDetail.value = {
          ...runDetail.value,
          judgment: { score, comment, at: new Date().toISOString() },
        };
      }
      toast.success("已记录打分");
      return true;
    } catch (e) {
      toast.error(e, "打分失败");
      return false;
    }
  }

  async function runCompare(a: string, b: string) {
    compareLoading.value = true;
    compareError.value = "";
    compare.value = null;
    try {
      compare.value = await api.reverseCompare(a, b);
    } catch (e) {
      compareError.value = errMessage(e);
      toast.error(e, "对比失败");
    } finally {
      compareLoading.value = false;
    }
  }

  return {
    configError,
    questionsError,
    batchesError,
    resultsError,
    batchesLoading,
    templates,
    rulesets,
    providers,
    configLoading,
    questions,
    questionsLoading,
    questionFilter,
    localImages,
    selectedQuestionIds,
    templateId,
    rulesetId,
    providerIds,
    renderForm,
    rerenderingRunId,
    currentRun,
    batches,
    currentBatchId,
    results,
    resultsLoading,
    runDetail,
    runDetailLoading,
    runDetailError,
    running,
    activeBatchId,
    cancelling,
    runProgress,
    compare,
    compareLoading,
    compareError,
    sortedTemplates,
    sortedRulesets,
    currentTemplate,
    currentRuleset,
    selectedCount,
    visibleQuestions,
    proseCount,
    canRun,
    loadConfig,
    loadBatches,
    loadAll,
    loadQuestions,
    setQuestionFilter,
    toggleQuestion,
    selectAllQuestions,
    clearQuestions,
    addLocalImage,
    addLocalImagePath,
    addDroppedPaths,
    removeLocalImage,
    saveTemplate,
    saveRuleset,
    saveProvider,
    deleteProvider,
    toggleProvider,
    runBatch,
    cancelBatch,
    selectBatch,
    refreshResults,
    rerender,
    openRun,
    closeRun,
    judge,
    runCompare,
  };
});
