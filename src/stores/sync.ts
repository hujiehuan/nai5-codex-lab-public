import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api, Channel, errMessage } from "../api";
import {
  createTiming,
  finishTiming,
  recordProgress,
  type IndexTiming,
} from "../lib/index-timing";
import type { SyncEvent, SyncLog, SyncPhase, SyncStatus } from "../types";
import { useCodex } from "./codex";
import { useToast } from "./toast";
import { useStyle } from "./style";

export const PHASE_LABEL: Record<SyncEvent["phase"], string> = {
  idle: "空闲",
  manifest: "清单",
  entries: "条目",
  images: "图片",
  embedding: "画风索引", // v0.3.0（§12.2）
  done: "完成",
  error: "失败",
};

export const useSync = defineStore("sync", () => {
  const toast = useToast();
  const status = ref<SyncStatus | null>(null);
  const readError = ref("");
  const loading = ref(false);
  const logs = ref<SyncLog[]>([]);
  /** 当前（或最近一次）同步的最新事件 */
  const progress = ref<SyncEvent | null>(null);
  /** 本会话内 Channel 是否仍在收事件 */
  const channelActive = ref(false);
  /** 最近一次完成（包括部分来源失败），用于横幅 */
  const lastDone = ref<SyncEvent | null>(null);
  const starting = ref(false);
  /** 仅本轮画风索引阶段计时，不把之前的图片下载耗时算进去。 */
  const indexTiming = ref<IndexTiming | null>(null);
  /** 本轮同步的"结束后处理"（重拉书目等）是否已做：Channel 的 done 与轮询兜底可能各到一次，只做一次。 */
  let finishHandled = false;

  const running = computed(
    () => channelActive.value || (status.value?.running ?? false),
  );

  function onFinished(phase: SyncPhase) {
    if (finishHandled) return;
    finishHandled = true;
    // done 也可能只有部分来源完成；成功来源仍会更新书目与 release。
    if (phase === "done") void useCodex().afterSync();
    void useStyle().refreshStatus();
  }

  async function refresh() {
    loading.value = true;
    readError.value = "";
    try {
      const [s, l] = await Promise.all([api.syncStatus(), api.syncLogs(20)]);
      if (!Array.isArray(l)) throw Error("同步日志格式错误");
      const wasRunning = running.value;
      status.value = s;
      logs.value = l;
      if (s.running) {
        if (!wasRunning) {
          finishHandled = false; // 别处 / 上次会话发起的同步，本会话第一次看见它在跑
          indexTiming.value = null; // 没有该任务的 Channel 样本，不能复用旧任务 ETA
        }
        return;
      }
      if (indexTiming.value)
        indexTiming.value = finishTiming(indexTiming.value, performance.now());
      if (channelActive.value) {
        // Channel 的 done/error 丢了（后端 panic、取消路径没发终止事件）：以后端状态为准，按钮恢复可用
        channelActive.value = false;
        if (
          progress.value &&
          progress.value.phase !== "done" &&
          progress.value.phase !== "error"
        ) {
          progress.value = {
            ...progress.value,
            phase: s.phase,
            message: s.message || "同步已结束，但没有收到完成事件",
            images_total: s.images_total,
            images_done: s.images_done,
            images_failed: s.images_failed,
          };
          toast.push(
            `同步已结束（未收到完成事件），已按后端状态刷新${s.message ? `：${s.message}` : ""}`,
            s.message.includes("同步失败") ? "error" : "info",
          );
        }
      }
      if (wasRunning) onFinished(s.phase);
    } catch (e) {
      readError.value = errMessage(e);
      toast.error(e, "读取同步状态失败");
    } finally {
      loading.value = false;
    }
  }

  async function start(force: boolean) {
    if (running.value) {
      toast.push("同步正在进行中", "info");
      return;
    }
    starting.value = true;
    finishHandled = false;
    lastDone.value = null;
    progress.value = null;
    indexTiming.value = null;
    let deviceRefreshed = false;
    const ch = new Channel<SyncEvent>();
    ch.onmessage = (e) => {
      progress.value = e;
      const now = performance.now();
      if (e.phase === "embedding") {
        const timing = indexTiming.value ?? createTiming(now);
        indexTiming.value = recordProgress(
          timing,
          e.embed_total ?? 0,
          (e.embed_done ?? 0) + (e.embed_failed ?? 0),
          now,
        );
        if (
          !deviceRefreshed &&
          (e.embed_done ?? 0) + (e.embed_failed ?? 0) > 0
        ) {
          deviceRefreshed = true;
          void useStyle().refreshStatus();
        }
      }
      if (e.phase === "done" || e.phase === "error") {
        if (indexTiming.value)
          indexTiming.value = finishTiming(indexTiming.value, now);
        channelActive.value = false;
        if (e.phase === "done") {
          lastDone.value = e;
          const partial = e.message.includes("同步失败");
          const message = `${partial ? "同步部分完成" : "同步完成"}：新增 ${e.added} 条，变更 ${e.changed} 条${e.message ? ` · ${e.message}` : ""}`;
          if (partial) toast.push(message, "error", 7000);
          else toast.success(message);
        } else {
          toast.error(e.message, "同步失败");
        }
        onFinished(e.phase);
        void refresh();
      }
    };
    channelActive.value = true;
    try {
      await api.syncStart(force, ch);
      await refresh();
    } catch (e) {
      channelActive.value = false;
      if (indexTiming.value)
        indexTiming.value = finishTiming(indexTiming.value, performance.now());
      toast.error(e, "启动同步失败");
    } finally {
      starting.value = false;
    }
  }

  async function cancel() {
    try {
      await api.syncCancel();
      toast.push("已请求取消同步", "info");
      await refresh();
    } catch (e) {
      toast.error(e, "取消失败");
    }
  }

  function dismissBanner() {
    lastDone.value = null;
  }

  return {
    readError,
    loading,
    status,
    logs,
    progress,
    indexTiming,
    channelActive,
    lastDone,
    starting,
    running,
    refresh,
    start,
    cancel,
    dismissBanner,
  };
});
