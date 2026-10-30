<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { RouterLink, RouterView, useRoute } from "vue-router";
import { api, errMessage } from "./api";
import type { AppInfo } from "./types";
import ThemeRoot from "./ui/ThemeRoot.vue";
import Toast from "./components/Toast.vue";
import { applyZoom, loadZoom, saveZoom, stepZoom, zoomActionForKey } from "./lib/zoom";
import { useUpdater } from "./stores/updater";
import VButton from "./ui/VButton.vue";
import { isArtistBook } from "./lib/artist-sources";
import { isSuozhangBook } from "./lib/codex-sources";

const mock = import.meta.env.MODE === "mock";
const info = ref<AppInfo | null>(null);
const bootError = ref<string | null>(null);
const zoom = ref(1);
const updater = useUpdater();
const route = useRoute();
const artistPage = computed(() => route.name === "codex" && isArtistBook(route.params.codexId));
const suozhangPage = computed(() => route.name === "codex" && isSuozhangBook(route.params.codexId));
const installDialog = ref<HTMLElement | null>(null);
watch(() => updater.installing, async (locked) => { if (locked) { await nextTick(); installDialog.value?.focus(); } });
function refreshUpdate() { void updater.refresh(); }
function jumpToUpdate() { requestAnimationFrame(() => document.getElementById("software-update")?.scrollIntoView({ block: "start" })); }

function setZoom(z: number) {
  zoom.value = z;
  applyZoom(z);
  saveZoom(z);
}
// 页面缩放快捷键：⌘= / ⌘+ 放大，⌘- 缩小，⌘0 复位。捕获阶段接管，避免 WebKit 自己处理或被网格快捷键吞掉。
function onKeydown(e: KeyboardEvent) {
  const action = zoomActionForKey(e);
  if (!action) return;
  e.preventDefault();
  e.stopPropagation();
  if (action === "reset") setZoom(1);
  else setZoom(stepZoom(zoom.value, action === "in" ? 1 : -1));
}

onMounted(async () => {
  setZoom(loadZoom());
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("focus", refreshUpdate);
  refreshUpdate();
  try {
    info.value = await api.appInfo();
  } catch (e) {
    bootError.value = errMessage(e);
  }
});
onBeforeUnmount(() => { window.removeEventListener("keydown", onKeydown, true); window.removeEventListener("focus", refreshUpdate); });
</script>

<template>
  <ThemeRoot><div class="shell" :inert="updater.installing">
    <nav class="sidebar" aria-label="主导航">
      <div class="brand"><span class="brand-mark">N</span><div>Codex Lab<small>NAI5 CODEX LAB</small></div></div>
      <div class="nav-group-label">LIBRARY · 图库</div>
      <RouterLink to="/sync" class="nav">同步</RouterLink>
      <RouterLink to="/codex/artist_nai5_personal" class="nav" :class="{ 'router-link-active': artistPage }" :aria-current="artistPage ? 'page' : undefined">画师词典</RouterLink>
      <RouterLink to="/codex/nai5_community_pack" class="nav">精选图包</RouterLink>
      <RouterLink to="/codex/suozhang_nai5" class="nav" :class="{ 'router-link-active': suozhangPage }" :aria-current="suozhangPage ? 'page' : undefined">所长法典</RouterLink>
      <div class="nav-group-label">LAB · 实验室</div>
      <RouterLink to="/studio" class="nav">总实验室</RouterLink>
      <div class="nav-group-label">WORKSPACE · 工作区</div>
      <RouterLink to="/settings" class="nav">设置</RouterLink>
      <div class="spacer" />
      <div class="meta" v-if="info"><RouterLink class="version-link" to="/settings#software-update" title="软件更新" @click="jumpToUpdate">v{{ info.version }} · 软件更新</RouterLink><div>图库 {{ info.latest_release ?? "未同步" }}</div></div>
      <div class="meta" v-if="zoom !== 1" title="⌘= 放大 · ⌘- 缩小 · ⌘0 复位">缩放 {{ Math.round(zoom * 100) }}%</div>
      <div class="meta error" v-if="bootError">{{ bootError }}</div>
    </nav>
    <main class="content">
      <div v-if="mock" class="offline-banner" role="status">离线演示 · 所有数据均为模拟，不会连接真实插件或产生费用</div>
      <RouterView />
    </main>
    <Toast />
  </div>
  <div v-if="updater.installing" class="update-lock">
    <section ref="installDialog" class="install-dialog" role="dialog" aria-modal="true" aria-labelledby="install-title" tabindex="-1">
      <h2 id="install-title">正在安装更新</h2>
      <p role="status">{{ updater.label }}</p>
      <p>保存与备份完成后应用会退出并重新打开。</p>
      <p v-if="updater.status?.message">{{ updater.status.message }}</p>
      <p v-if="updater.readError" class="error" role="alert">{{ updater.readError }}</p>
      <VButton v-if="updater.readError" :disabled="updater.loading" @click="updater.refresh">重试读取安装状态</VButton>
    </section>
  </div></ThemeRoot>
</template>

<style scoped>
.version-link{display:inline-flex;align-items:center;min-height:28px;color:var(--accent);text-decoration:none}.version-link:hover{text-decoration:underline}.update-lock{position:fixed;inset:0;z-index:20000;display:flex;align-items:center;justify-content:center;background:var(--panel);padding:24px}.install-dialog{max-width:480px;width:100%;border:1px solid var(--line);border-radius:var(--radius);padding:24px;background:var(--panel);box-shadow:var(--shadow);outline:none;overflow-wrap:anywhere}.install-dialog h2{font-size:18px;margin:0 0 16px}.install-dialog p{font-size:13px;line-height:1.6;color:var(--muted);margin:10px 0}.install-dialog .error{color:var(--danger)}
</style>
