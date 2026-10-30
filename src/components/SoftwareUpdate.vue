<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useUpdater } from "../stores/updater";
import VCard from "../ui/VCard.vue";
import VButton from "../ui/VButton.vue";
import ProgressBar from "./ProgressBar.vue";

const updater = useUpdater();
const route = useRoute();
const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | undefined;
const elapsed = computed(() => {
  const started = Date.parse(updater.status?.started_at ?? "");
  if (!Number.isFinite(started)) return "";
  const seconds = Math.max(0, Math.floor((now.value - started) / 1000));
  return seconds < 60 ? `已用 ${seconds} 秒` : `已用 ${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
});
onMounted(() => {
  void updater.refresh();
  timer = setInterval(() => { now.value = Date.now(); }, 1000);
  if (route.hash === "#software-update") requestAnimationFrame(() => document.getElementById("software-update")?.scrollIntoView({ block: "start" }));
});
onBeforeUnmount(() => clearInterval(timer));
</script>

<template>
  <VCard id="software-update" data-testid="software-update" title="软件更新" subtitle="在应用内准备新版本，完成后安装并重启。">
    <template #actions>
      <VButton v-if="updater.status?.source_dir" :disabled="!updater.canCheck" :loading="updater.pending === 'check' || updater.status?.phase === 'checking'" @click="updater.check">检查更新</VButton>
    </template>
    <div class="update-summary">
      <div>
        <p class="update-version">当前版本 <strong>{{ updater.status ? `v${updater.status.current_version}` : '尚未读取' }}</strong><span v-if="updater.status?.current_revision" class="revision">{{ updater.status.current_revision.slice(0, 8) }}</span></p>
        <p class="update-state" role="status">{{ updater.label }}</p>
      </div>
      <div v-if="updater.status?.candidate" class="candidate">
        <span>目标版本</span><strong>v{{ updater.status.candidate.version }}</strong><span class="revision">{{ updater.status.candidate.revision.slice(0, 8) }}</span>
      </div>
    </div>
    <p v-if="updater.readError" class="update-error" role="alert">{{ updater.readError }} <VButton :disabled="updater.loading" @click="updater.refresh">重试读取</VButton></p>
    <p v-if="updater.actionError" class="update-error" role="alert">{{ updater.actionError }}</p>
    <p v-if="updater.status?.message" :class="updater.status.phase === 'error' ? 'update-error' : 'update-note'" :role="updater.status.phase === 'error' ? 'alert' : undefined">{{ updater.status.message }}</p>
    <template v-if="updater.status && !updater.readError">
      <div v-if="!updater.status.source_dir" class="setup">
        <p>首次选择本机的 nai5-codex-lab 源码目录。更新使用已有的 Git 登录，并需要 Git、Node.js / npm、Rust / Cargo 和 Xcode 命令行工具。</p>
        <VButton class="primary" :disabled="!updater.status.supported || updater.busy" @click="updater.chooseSource">选择源码目录</VButton>
      </div>
      <details v-else class="update-source">
        <summary>更新来源 · 本机源码</summary>
        <p class="source-path">{{ updater.status.source_dir }}</p>
        <p class="update-note">使用已有 Git 登录检查稳定版本；在独立目录构建，保留这里尚未提交的修改。需要 Git、Node.js / npm、Rust / Cargo 和 Xcode 命令行工具。</p>
        <VButton :disabled="updater.busy || !updater.status.supported" @click="updater.chooseSource">更换源码目录</VButton>
      </details>
      <div v-if="updater.status.phase === 'checking' || updater.status.phase === 'preparing'" class="update-progress" role="progressbar" :aria-label="updater.status.stage || updater.label">
        <ProgressBar :value="null" :label="[updater.status.stage, elapsed].filter(Boolean).join(' · ')" />
      </div>
      <div class="update-actions">
        <VButton v-if="updater.canPrepare" class="primary" @click="updater.prepare">{{ updater.status.phase === 'error' || updater.status.phase === 'cancelled' ? '重新准备更新' : '准备更新' }}</VButton>
        <VButton v-if="updater.status.phase === 'ready'" class="primary" :disabled="!updater.canInstall" @click="updater.install">安装并重启</VButton>
        <VButton v-if="updater.canCancel || updater.pending === 'cancel'" :disabled="!updater.canCancel" @click="updater.cancel">{{ updater.pending === 'cancel' ? '正在取消…' : '取消更新' }}</VButton>
      </div>
      <p v-if="updater.status.candidate" class="update-note">准备更新时可继续使用软件。安装将先保存工作区并备份数据库，然后退出并重启；请先结束正在进行的本地任务。</p>
    </template>
    <details v-if="updater.status?.log_tail.length" class="update-log">
      <summary>查看更新日志</summary>
      <pre>{{ updater.status.log_tail.join('\n') }}</pre>
    </details>
  </VCard>
</template>

<style scoped>
.update-summary{display:flex;justify-content:space-between;align-items:start;gap:16px;flex-wrap:wrap}.update-version,.update-state{margin:0}.update-version{font-size:13px;color:var(--muted)}.update-version strong{color:var(--ink);font-size:16px;margin-left:8px}.update-state{font-size:14px;font-weight:600;margin-top:8px}.revision{font-family:var(--mono);font-size:11px;color:var(--muted);margin-left:8px}.candidate{display:flex;gap:8px;align-items:baseline;flex-wrap:wrap;font-size:12px}.candidate>span:first-child{color:var(--muted)}.candidate strong{font-size:16px}.update-error{color:var(--danger);font-size:13px;margin:12px 0;overflow-wrap:anywhere}.update-note,.setup p{font-size:12px;color:var(--muted);line-height:1.7;margin:10px 0;overflow-wrap:anywhere}.update-source,.update-log{margin-top:12px}summary{cursor:pointer;font-size:12px;min-height:32px;display:list-item;padding:8px 0;box-sizing:border-box}.source-path{font-family:var(--mono);font-size:12px;overflow-wrap:anywhere;margin:4px 0}.update-progress{margin-top:16px}.update-actions{display:flex;gap:8px;flex-wrap:wrap;margin-top:12px}.update-actions:empty{display:none}.update-log pre{max-height:200px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;padding:12px;background:var(--field);border:1px solid var(--line);border-radius:8px;font-size:11px;margin:0}#software-update{scroll-margin-top:16px}
</style>
