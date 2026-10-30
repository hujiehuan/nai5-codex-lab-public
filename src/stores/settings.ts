import { defineStore } from "pinia";
import { ref } from "vue";
import { api, errMessage } from "../api";
import type { Settings, SettingsPatch } from "../types";
import { useToast } from "./toast";

export const useSettings = defineStore("settings", () => {
  const toast = useToast();
  const settings = ref<Settings | null>(null);
  const loading = ref(false);
  const readError = ref("");
  const saving = ref(false);
  /** account → 是否已设置 */
  const secrets = ref<Record<string, boolean>>({});
  let loadPromise: Promise<void> | null = null;


  async function reload() {
    loading.value = true;
    readError.value = "";
    try {
      settings.value = await api.settingsGet();
    } catch (e) {
      readError.value = errMessage(e);
      toast.error(e, "读取设置失败");
    } finally {
      loading.value = false;
    }
  }

  async function ensureLoaded() {
    if (settings.value) return;
    if (!loadPromise)
      loadPromise = reload().finally(() => {
        loadPromise = null;
      });
    await loadPromise;
  }

  async function save(patch: SettingsPatch): Promise<boolean> {
    saving.value = true;
    try {
      settings.value = await api.settingsSet(patch);
      return true;
    } catch (e) {
      toast.error(e, "保存设置失败");
      return false;
    } finally {
      saving.value = false;
    }
  }

  async function refreshSecret(account: string) {
    try {
      secrets.value = {
        ...secrets.value,
        [account]: await api.secretHas(account),
      };
    } catch (e) {
      toast.error(e, "读取钥匙串失败");
    }
  }

  async function setSecret(account: string, value: string): Promise<boolean> {
    try {
      await api.secretSet(account, value);
      secrets.value = { ...secrets.value, [account]: true };
      return true;
    } catch (e) {
      toast.error(e, "写入钥匙串失败");
      return false;
    }
  }

  async function deleteSecret(account: string): Promise<boolean> {
    try {
      await api.secretDelete(account);
      secrets.value = { ...secrets.value, [account]: false };
      return true;
    } catch (e) {
      toast.error(e, "删除钥匙串失败");
      return false;
    }
  }

  return {
    readError,
    settings,
    loading,
    saving,
    secrets,
    reload,
    ensureLoaded,
    save,
    refreshSecret,
    setSecret,
    deleteSecret,
  };
});
