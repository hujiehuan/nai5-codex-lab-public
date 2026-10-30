import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { api, errMessage } from "../api";
import {
  modelSelectionIssue,
  parseModelSelection,
  unsupportedModelSelection,
  type ModelSelection,
} from "../lib/model-selection";

export const usePluginModels = defineStore("plugin-models", () => {
  const selection = ref<ModelSelection | null>(null),
    error = ref<string | null>(null),
    loading = ref(false),
    legacy = ref(false);
  let pending: Promise<boolean> | null = null;
  const ids = computed(() => selection.value?.effective ?? []);
  const ready = computed(
    () => !!selection.value && !error.value && !loading.value,
  );
  const description = computed(() =>
    loading.value
      ? "正在刷新模型名单…"
      : error.value && selection.value
        ? `上次成功读取 ${ids.value.length} 个候选；本次读取失败`
        : !selection.value
          ? "模型名单尚未读取"
          : legacy.value
            ? `旧插件模型列表 · ${ids.value.length} 个候选`
            : selection.value.enabled
              ? `使用插件总览的模型名单 · ${ids.value.length} 个可用${selection.value.missing.length ? `，${selection.value.missing.length} 个已失效` : ""}`
              : `插件未启用模型筛选 · ${ids.value.length} 个可用`,
  );
  function issue(
    selected: readonly string[],
    candidates?: readonly string[],
  ): string | null {
    if (error.value) return `模型名单读取失败：${error.value}`;
    if (!selection.value || loading.value) return "请等模型名单读取完成";
    return modelSelectionIssue(
      selected,
      candidates
        ? ids.value.filter((id) => candidates.includes(id))
        : ids.value,
    );
  }
  function refresh(): Promise<boolean> {
    if (pending) return pending;
    loading.value = true;
    error.value = null;
    pending = (async () => {
      try {
        let value: ModelSelection;
        try {
          value = parseModelSelection(
            await api.pluginModelSelection(),
          );
          legacy.value = false;
        } catch (e) {
          if (!unsupportedModelSelection(e)) throw e;
          const list = await api.pluginProviders();
          value = parseModelSelection({
            enabled: false,
            selected: [],
            available: list,
            effective: list,
            missing: [],
          });
          legacy.value = true;
        }
        selection.value = value;
        return true;
      } catch (e) {
        error.value = errMessage(e);
        return false;
      } finally {
        loading.value = false;
        pending = null;
      }
    })();
    return pending;
  }
  return {
    selection,
    ids,
    ready,
    description,
    error,
    loading,
    legacy,
    issue,
    refresh,
  };
});
