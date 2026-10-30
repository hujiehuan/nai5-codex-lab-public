// 画风模型与画风检索（CONTRACT §12.2 / §12.5）：模型状态、下载 / 重建索引的 Channel（存活于 store，切页不丢）、检索结果、面板表单与来源选择。
// 设置页的"画风模型"卡片与反推实验室的 StylePanel 共用这一个 store。
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api, Channel, errMessage } from "../api";
import type { DownloadEvent, StyleClassifierStatus, StyleClassifyResult, StyleModelStatus, StyleSearchRequest, StyleSearchResult, SyncEvent } from "../types";
import { defaultStyleForm, embedLabel, isStyleSetupError, styleReady, type StyleForm } from "../lib/style";
import { createTiming, finishTiming, recordProgress, type IndexTiming } from "../lib/index-timing";
import { useToast } from "./toast";

export interface StyleComparison {
  model: string;
  label: string;
  phase: "queued" | "searching" | "done" | "error";
  result: StyleSearchResult | null;
  error: string | null;
  elapsed_ms: number | null;
}
export interface ClassifierComparison {
  model: string;
  label: string;
  phase: StyleComparison["phase"];
  result: StyleClassifyResult | null;
  error: string | null;
  elapsed_ms: number | null;
}

export const useStyle = defineStore("style", () => {
  const toast = useToast();
  const status = ref<StyleModelStatus | null>(null);
  const models = ref<StyleModelStatus[]>([]);
  const classifiers = ref<StyleClassifierStatus[]>([]);
  const selecting = ref(false);
  const statusLoading = ref(false);
  /** 真正抛错时的文本（读不到状态）；面板内显示并 toast。 */
  const statusError = ref<string | null>(null);

  /** 下载中；download 是最新一条事件（还没收到时 null → 跑马灯）。 */
  const downloading = ref(false);
  const downloadingAll = ref(false);
  const downloadModel = ref<string | null>(null);
  const downloadQueue = ref<string[]>([]);
  const downloadPosition = ref(0);
  let downloadCancelled = false;
  const download = ref<DownloadEvent | null>(null);
  /** 重建索引中；rebuild 是最新一条 SyncEvent（phase=embedding → done | error）。 */
  const rebuilding = ref(false);
  const rebuildModel = ref<string | null>(null);
  const rebuild = ref<SyncEvent | null>(null);
  const rebuildTiming = ref<IndexTiming | null>(null);

  const searching = ref(false);
  const result = ref<StyleSearchResult | null>(null);
  const comparisons = ref<StyleComparison[]>([]);
  const classifierComparisons = ref<ClassifierComparison[]>([]);
  const searchError = ref<string | null>(null);
  /** searchError 是 bad_request（模型未下载 / 索引为空）：面板给"去设置页"的链接。 */
  const searchNeedsSetup = ref(false);
  const lastRequest = ref<StyleSearchRequest | null>(null);
  /** 面板表单与来源选择（会话内保留，不写 localStorage）。 */
  const form = ref<StyleForm>(defaultStyleForm());
  const sourceId = ref<string | null>(null);

  const busy = computed(() => downloading.value || downloadingAll.value || rebuilding.value || selecting.value);
  const readyModels = computed(() => models.value.filter(styleReady));
  const readyClassifiers = computed(() => classifiers.value.filter((m) => m.present && m.runtime_ready));

  function rememberModel(next: StyleModelStatus) {
    if (!status.value || status.value.model === next.model) status.value = next;
    const i = models.value.findIndex((m) => m.model === next.model);
    if (i < 0) models.value.push(next);
    else models.value[i] = next;
  }

  async function refreshStatus() {
    statusLoading.value = true;
    try {
      status.value = await api.styleModelStatus();
      models.value = await api.styleModelsList();
      classifiers.value = await api.styleClassifiersList();
      statusError.value = null;
    } catch (e) {
      statusError.value = errMessage(e);
      toast.error(e, "读取画风模型状态失败");
    } finally {
      statusLoading.value = false;
    }
  }

  async function selectModel(model: string) {
    if (busy.value || searching.value || model === status.value?.model) return;
    selecting.value = true;
    try {
      const next = await api.styleModelSelect(model);
      status.value = next;
      rememberModel(next);
      toast.success(`当前画风模型：${next.label}`);
    } catch (e) {
      toast.error(e, "切换画风模型失败");
    } finally {
      selecting.value = false;
    }
  }

  /** style_model_download 在下载结束后才返回（成功给最新状态）；进度事件按文件发。取消 / 失败 → 命令抛错，按失败提示并重读状态。 */
  async function downloadOne(model?: string): Promise<boolean> {
    downloading.value = true;
    downloadModel.value = model ?? status.value?.model ?? null;
    download.value = null;
    const ch = new Channel<DownloadEvent>();
    ch.onmessage = (e) => {
      download.value = e;
    };
    // 经函数取最后一条事件：TS 看不见闭包里的赋值，直接读 download.value 会被收窄成 null
    const last = (): DownloadEvent | null => download.value;
    try {
      const next = await api.styleModelDownload(ch, model);
      rememberModel(next);
      const err = last()?.error;
      if (err) toast.error(err, "下载画风模型失败");
      else if (!downloadCancelled) toast.success(`${next.label} 已下载`);
      return !err && !downloadCancelled;
    } catch (e) {
      if (!downloadCancelled) toast.error(e, "下载画风模型失败");
      void refreshStatus();
      return false;
    } finally {
      downloading.value = false;
    }
  }
  async function startDownload(model?: string) {
    if (busy.value || searching.value) return;
    downloadCancelled = false;
    downloadQueue.value = [];
    await downloadOne(model ?? status.value?.model);
  }
  /** 依次下载缺失的模型；取消或任一下载失败后停止队列，已有模型保留。 */
  async function downloadAll() {
    if (busy.value || searching.value) return;
    downloadingAll.value = true;
    downloadCancelled = false;
    try {
      models.value = await api.styleModelsList();
      classifiers.value = await api.styleClassifiersList();
      const missing = [...models.value.filter((m) => !m.present).map((m) => ({ model: m.model, classifier: false })),
        ...classifiers.value.filter((m) => !m.present || !m.runtime_ready).map((m) => ({ model: m.model, classifier: true }))];
      downloadQueue.value = missing.map((m) => m.model);
      downloadPosition.value = 0;
      for (const item of missing) {
        if (downloadCancelled) break;
        downloadPosition.value += 1;
        if (item.classifier && classifiers.value.some((m) => m.model === item.model && m.present && m.runtime_ready)) continue;
        if (!await (item.classifier ? downloadClassifierOne(item.model) : downloadOne(item.model))) break;
      }
      if (!downloadQueue.value.length) toast.success("所有画风模型均已下载");
    } catch (e) {
      toast.error(e, "读取模型目录失败");
    } finally {
      downloadingAll.value = false;
    }
  }
  async function downloadClassifierOne(model: string): Promise<boolean> {
    downloading.value = true;
    downloadModel.value = model;
    download.value = null;
    const ch = new Channel<DownloadEvent>();
    ch.onmessage = (e) => { download.value = e; };
    const last = (): DownloadEvent | null => download.value;
    try {
      const next = await api.styleClassifierDownload(model, ch);
      // 分类器共用 ONNX 运行时，补齐后重读目录，让其他已下载模型也立即可用。
      classifiers.value = await api.styleClassifiersList();
      const i = classifiers.value.findIndex((m) => m.model === next.model);
      if (i < 0) classifiers.value.push(next); else classifiers.value[i] = next;
      if (last()?.error) throw new Error(last()!.error!);
      if (!downloadCancelled) toast.success(`${next.label} 已下载`);
      return !downloadCancelled;
    } catch (e) {
      if (!downloadCancelled) toast.error(e, "下载画师识别模型失败");
      void refreshStatus();
      return false;
    } finally {
      downloading.value = false;
    }
  }
  async function startClassifierDownload(model: string) {
    if (busy.value || searching.value) return;
    downloadCancelled = false;
    downloadQueue.value = [];
    await downloadClassifierOne(model);
  }
  async function cancelDownload() {
    downloadCancelled = true;
    try {
      await api.styleModelDownloadCancel();
      toast.push("已请求取消下载", "info");
    } catch (e) {
      toast.error(e, "取消下载失败");
    }
  }

  /** style_index_rebuild 与 sync_start 一样立即返回，靠 Channel 的 done / error 收尾；同步正在跑 → sync_running。 */
  async function startRebuild(force: boolean, model?: string) {
    if (busy.value || searching.value) return;
    rebuilding.value = true;
    const fixedModel = model ?? status.value?.model;
    rebuildModel.value = fixedModel ?? null;
    rebuild.value = null;
    rebuildTiming.value = createTiming(performance.now());
    let deviceRefreshed = false;
    const ch = new Channel<SyncEvent>();
    ch.onmessage = (e) => {
      rebuild.value = e;
      const now = performance.now();
      if (rebuildTiming.value && typeof e.embed_total === "number") {
        rebuildTiming.value = recordProgress(rebuildTiming.value, e.embed_total, (e.embed_done ?? 0) + (e.embed_failed ?? 0), now);
      }
      // 初始 0/total 事件在加载模型之前；首批落库后才能读到真正使用的设备。
      if (!deviceRefreshed && e.phase === "embedding" && (e.embed_done ?? 0) + (e.embed_failed ?? 0) > 0) {
        deviceRefreshed = true;
        void refreshStatus();
      }
      if (e.phase === "done" || e.phase === "error") {
        rebuilding.value = false;
        if (rebuildTiming.value) rebuildTiming.value = finishTiming(rebuildTiming.value, now);
        // 后端三个 embed 键恒带（缺省 0）：有东西可算时给计数，模型未下载 / 无事可做时用它的 message（如"画风模型未下载，跳过索引"）
        if (e.phase === "done" && e.message === "已取消") toast.push("画风索引已取消，已完成结果已保留", "info");
        else if (e.phase === "done") toast.success(e.embed_total ? `${embedLabel(e)} · 完成` : e.message || "画风索引完成");
        else toast.error(e.message, "重建画风索引失败");
        void refreshStatus();
      }
    };
    try {
      await api.styleIndexRebuild(force, ch, fixedModel);
    } catch (e) {
      rebuilding.value = false;
      if (rebuildTiming.value) rebuildTiming.value = finishTiming(rebuildTiming.value, performance.now());
      toast.error(e, "启动重建索引失败");
    }
  }
  /** §12.2：建索引与同步共用取消令牌（sync_cancel）。 */
  async function cancelRebuild() {
    try {
      await api.syncCancel();
      toast.push("已请求取消建索引", "info");
    } catch (e) {
      toast.error(e, "取消失败");
    }
  }

  /** 检索失败不 toast：错误留在面板里（bad_request 时附"去设置页"）。 */
  async function search(req: StyleSearchRequest) {
    if (searching.value || busy.value) return;
    searching.value = true;
    searchError.value = null;
    searchNeedsSetup.value = false;
    comparisons.value = [];
    classifierComparisons.value = [];
    lastRequest.value = req;
    try {
      result.value = await api.styleSearch(req);
    } catch (e) {
      result.value = null;
      searchError.value = errMessage(e);
      searchNeedsSetup.value = isStyleSetupError(e);
    } finally {
      searching.value = false;
      void refreshStatus();
    }
  }
  /** 对比始终复用启动时的图片与参数，依次运行，不把不同模型的分数混排。 */
  async function compare(req: StyleSearchRequest) {
    if (searching.value || busy.value) return;
    const candidates = readyModels.value.map((m) => ({ model: m.model, label: m.label }));
    const classifierCandidates = readyClassifiers.value.map((m) => ({ model: m.model, label: m.label }));
    if (!candidates.length && !classifierCandidates.length) {
      searchError.value = "先在设置页准备画风索引模型或画师识别模型";
      searchNeedsSetup.value = true;
      return;
    }
    const snapshot = { ...req, source: { ...req.source }, codex_ids: [...req.codex_ids], decisions: [...req.decisions] };
    searching.value = true;
    result.value = null;
    searchError.value = null;
    searchNeedsSetup.value = false;
    lastRequest.value = snapshot;
    comparisons.value = candidates.map((m) => ({ ...m, phase: "queued", result: null, error: null, elapsed_ms: null }));
    classifierComparisons.value = classifierCandidates.map((m) => ({ ...m, phase: "queued", result: null, error: null, elapsed_ms: null }));
    try {
      for (let i = 0; i < candidates.length; i += 1) {
        const group = comparisons.value[i];
        group.phase = "searching";
        const started = performance.now();
        try {
          group.result = await api.styleSearch({ ...snapshot, source: { ...snapshot.source }, codex_ids: [...snapshot.codex_ids], decisions: [...snapshot.decisions], model: group.model });
          group.phase = "done";
        } catch (e) {
          group.error = errMessage(e);
          group.phase = "error";
        } finally {
          group.elapsed_ms = Math.max(0, Math.round(performance.now() - started));
        }
      }
      for (const group of classifierComparisons.value) {
        group.phase = "searching";
        const started = performance.now();
        try {
          group.result = await api.styleClassify({ source: { ...snapshot.source }, model: group.model, top_k: snapshot.top_k });
          group.phase = "done";
        } catch (e) {
          group.error = errMessage(e);
          group.phase = "error";
        } finally {
          group.elapsed_ms = Math.max(0, Math.round(performance.now() - started));
        }
      }
    } finally {
      searching.value = false;
      void refreshStatus();
    }
  }
  function clearSearch() {
    if (searching.value) return;
    result.value = null;
    comparisons.value = [];
    classifierComparisons.value = [];
    searchError.value = null;
    searchNeedsSetup.value = false;
  }

  return {
    status, models, classifiers, readyModels, readyClassifiers, selecting, statusLoading, statusError, downloading, downloadingAll, downloadModel, downloadQueue, downloadPosition, download, rebuilding, rebuildModel, rebuild, rebuildTiming, searching, result, comparisons, classifierComparisons, searchError, searchNeedsSetup, lastRequest, form, sourceId, busy,
    refreshStatus, selectModel, startDownload, startClassifierDownload, downloadAll, cancelDownload, startRebuild, cancelRebuild, search, compare, clearSearch,
  };
});
