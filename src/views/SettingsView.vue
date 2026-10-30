<script setup lang="ts">
import AsyncState from "../ui/AsyncState.vue";
import VCard from "../ui/VCard.vue";
import VCost from "../ui/VCost.vue";
import VButton from "../ui/VButton.vue";
import { computed, onMounted, reactive, ref, watch } from "vue";
import { api, errMessage } from "../api";
import { useSettings } from "../stores/settings";
import { useStyle } from "../stores/style";
import { useSync } from "../stores/sync";
import { useToast } from "../stores/toast";
import ProgressBar from "../components/ProgressBar.vue";
import SoftwareUpdate from "../components/SoftwareUpdate.vue";
import IndexTiming from "../components/IndexTiming.vue";
import { fmtBytes, fmtDateTime } from "../lib/format";
import {
  codexLabel,
  fmtFullCounts,
  fmtPoolCounts,
  orderedCodexIds,
  sumFullCounts,
} from "../lib/pool";
import {
  downloadLabel,
  downloadRatio,
  embedLabel,
  embedRatio,
  indexRatio,
} from "../lib/style";
import type {
  AppEvent,
  BlacklistItem,
  ExportResult,
  FullCounts,
  PluginLibraryStatus,
  PoolStatus,
  PushFullResult,
  PushResult,
  StyleDevicePreference,
} from "../types";

const settingsStore = useSettings();
const style = useStyle();
const sync = useSync();
const toast = useToast();

const form = reactive({
  plugin_base_url: "",
  webui_username: "",
  download_concurrency: 2,
});

function fill() {
  const s = settingsStore.settings;
  if (!s) return;
  form.plugin_base_url = s.plugin_base_url;
  form.webui_username = s.webui_username;
  form.download_concurrency = s.download_concurrency;
}
// 独立保存画风设备时，保留基本设置表单里尚未保存的编辑。
watch(
  [
    () => settingsStore.settings?.plugin_base_url,
    () => settingsStore.settings?.webui_username,
    () => settingsStore.settings?.download_concurrency,
  ],
  fill,
);

const devicePreference = ref<StyleDevicePreference>("auto");
watch(
  () => settingsStore.settings?.style_device,
  (value) => {
    devicePreference.value = value ?? "auto";
  },
  { immediate: true },
);
const deviceBusy = computed(
  () =>
    style.busy ||
    style.searching ||
    sync.running ||
    sync.starting ||
    settingsStore.saving ||
    !settingsStore.settings,
);
const modelBusy = computed(
  () => style.busy || style.searching || sync.running || sync.starting,
);
const modelLabel = (id: string | null) =>
  [...style.models, ...style.classifiers].find((m) => m.model === id)?.label ??
  id ??
  "画风模型";
async function selectActiveModel(event: Event) {
  const select = event.target as HTMLSelectElement;
  if (!modelBusy.value) await style.selectModel(select.value);
  select.value = style.status?.model ?? "";
}
const activeDeviceLabel = computed(() =>
  style.status?.active_device === "metal"
    ? "Metal GPU"
    : style.status?.active_device === "cpu"
      ? "CPU"
      : "尚未加载",
);
async function saveDevicePreference() {
  if (
    !deviceBusy.value &&
    (await settingsStore.save({ style_device: devicePreference.value }))
  ) {
    toast.success("计算设备已保存，下次任务生效");
    await style.refreshStatus();
  }
  // 保存失败时回到后端已保存的选择，避免界面看似切换成功。
  devicePreference.value = settingsStore.settings?.style_device ?? "auto";
}

async function save() {
  const ok = await settingsStore.save({
    plugin_base_url: form.plugin_base_url.trim().replace(/\/+$/, ""),
    webui_username: form.webui_username.trim(),
    download_concurrency: Math.max(
      1,
      Math.min(4, Math.round(form.download_concurrency) || 1),
    ),
  });
  if (ok) toast.success("设置已保存");
}

// ---- 钥匙串 ----
const webuiPassword = ref("");
const libraryToken = ref("");
async function setSecret(account: string, value: string, clear: () => void) {
  if (!value) return;
  if (await settingsStore.setSecret(account, value)) {
    clear();
    toast.success("已写入钥匙串");
  }
}

// ---- 导出 / 推送（筛选池，CONTRACT §3）----
const exporting = ref(false);
const exportResult = ref<ExportResult | null>(null);
const pushing = ref(false);
const pushResult = ref<PushResult | null>(null);
async function doExport() {
  exporting.value = true;
  try {
    exportResult.value = await api.exportLibraries({});
    toast.success(`已导出到 ${exportResult.value.dir}`);
  } catch (e) {
    toast.error(e, "导出失败");
  } finally {
    exporting.value = false;
  }
}
async function doPush() {
  pushing.value = true;
  pushResult.value = null;
  try {
    pushResult.value = await api.pushToPlugin();
    if (pushResult.value.ok) {
      toast.success("已推送到插件");
      void loadPluginStatus();
    } else toast.push(`插件返回：${pushResult.value.message}`, "error", 8000);
  } catch (e) {
    toast.error(e, "推送失败");
  } finally {
    pushing.value = false;
  }
}
function pretty(v: unknown): string {
  try {
    return typeof v === "string" ? v : JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

// ---- 完整池（CONTRACT §8）----
const exportingFull = ref(false);
const exportFullResult = ref<ExportResult | null>(null);
const pushingFull = ref(false);
const pushFullResult = ref<PushFullResult | null>(null);
async function doExportFull() {
  exportingFull.value = true;
  try {
    exportFullResult.value = await api.exportFullPool({});
    toast.success(`完整池已导出到 ${exportFullResult.value.dir}`);
  } catch (e) {
    toast.error(e, "导出完整池失败");
  } finally {
    exportingFull.value = false;
  }
}
async function doPushFull() {
  pushingFull.value = true;
  pushFullResult.value = null;
  try {
    const r = await api.pushFullToPlugin();
    pushFullResult.value = r;
    if (r.ok) {
      toast.success(
        `完整池已推送到插件：${fmtFullCounts(sumFullCounts(r.counts))}`,
      );
      void loadPluginStatus();
    } else toast.push(`插件返回：${r.message}`, "error", 8000);
  } catch (e) {
    toast.error(e, "推送完整池失败");
  } finally {
    pushingFull.value = false;
  }
}
/** 推送结果表的行：按 §8.1 顺序的各 codex，多于一本时末尾加合计。 */
const pushFullRows = computed<
  { key: string; label: string; counts: FullCounts }[]
>(() => {
  const r = pushFullResult.value;
  if (!r) return [];
  const rows = orderedCodexIds(Object.keys(r.counts)).map((id) => ({
    key: id,
    label: codexLabel(id),
    counts: r.counts[id],
  }));
  if (rows.length > 1)
    rows.push({ key: "__sum", label: "合计", counts: sumFullCounts(r.counts) });
  return rows;
});

// ---- 插件当前持有（CONTRACT §8.2 plugin_library_status）----
const pluginStatus = ref<PluginLibraryStatus | null>(null);
const statusLoading = ref(false);
const statusError = ref<string | null>(null); // 真正抛错（plugin_rejected 等）时的文本；reachable=false 不算错
const statusAt = ref<string | null>(null); // 上次成功查询的本地时间
async function loadPluginStatus() {
  statusLoading.value = true;
  try {
    pluginStatus.value = await api.pluginLibraryStatus();
    statusError.value = null;
    statusAt.value = new Date().toISOString();
  } catch (e) {
    // 连不上 / 超时 / 未配置密码或 token 由后端包成 reachable=false 返回，不会到这里；
    // 到这里的是 401/403（plugin_rejected）之类，面板内显示并 toast。
    pluginStatus.value = null;
    statusError.value = errMessage(e);
    toast.error(e, "读取插件状态失败");
  } finally {
    statusLoading.value = false;
  }
}
const poolCards = computed<
  {
    key: "filtered" | "full";
    title: string;
    hint: string;
    status: PoolStatus | null;
  }[]
>(() => {
  const s = pluginStatus.value;
  return [
    {
      key: "filtered",
      title: "筛选池",
      hint: "/画 /反推 抽画师串用",
      status: s?.filtered ?? null,
    },
    {
      key: "full",
      title: "完整池",
      hint: "全量条目带决策标记，供将来画风检索",
      status: s?.full ?? null,
    },
  ];
});

// ---- 画风模型（CONTRACT §12.2 / §12.5）：状态、下载 / 重建索引进度都在 stores/style.ts，这里只算进度条的值与文字 ----
const indexPct = computed(() => {
  const r = style.status ? indexRatio(style.status) : null;
  return r === null ? null : Math.round(r * 100);
});
const downloadValue = computed<number | null>(() =>
  style.downloading
    ? downloadRatio(style.download)
    : style.download?.error
      ? 0
      : 1,
);
const rebuildValue = computed<number | null>(() =>
  style.rebuilding
    ? embedRatio(style.rebuild)
    : style.rebuild?.phase === "error"
      ? 0
      : 1,
);
const rebuildLabel = computed(() => {
  const r = style.rebuild;
  if (!r) return "启动中…";
  if (r.phase === "error") return `失败：${r.message}`;
  return `${embedLabel(r)}${r.message ? ` · ${r.message}` : ""}`;
});

// ---- 黑名单 ----
const blacklist = ref<BlacklistItem[]>([]);
const blacklistError = ref("");
const blacklistLoading = ref(false);
const newName = ref("");
const newReason = ref("");
async function loadBlacklist() {
  blacklistLoading.value = true;
  blacklistError.value = "";
  try {
    const data = await api.blacklistList();
    if (!Array.isArray(data)) throw Error("黑名单格式错误");
    blacklist.value = data;
  } catch (e) {
    blacklistError.value = errMessage(e);
    toast.error(e, "读取黑名单失败");
  } finally {
    blacklistLoading.value = false;
  }
}
async function addBlacklist() {
  const name = newName.value.trim();
  if (!name) return;
  try {
    await api.blacklistAdd(name, newReason.value.trim());
    newName.value = "";
    newReason.value = "";
    await loadBlacklist();
  } catch (e) {
    toast.error(e, "添加黑名单失败");
  }
}
async function removeBlacklist(name: string) {
  try {
    await api.blacklistRemove(name);
    await loadBlacklist();
  } catch (e) {
    toast.error(e, "删除黑名单失败");
  }
}

// ---- 事件 ----
const events = ref<AppEvent[]>([]);
const eventsError = ref("");
const eventsLoading = ref(false);
async function loadEvents() {
  eventsLoading.value = true;
  eventsError.value = "";
  try {
    const data = await api.eventsRecent(50);
    if (!Array.isArray(data)) throw Error("事件列表格式错误");
    events.value = data;
  } catch (e) {
    eventsError.value = errMessage(e);
    toast.error(e, "读取事件失败");
  } finally {
    eventsLoading.value = false;
  }
}

onMounted(async () => {
  await settingsStore.ensureLoaded();
  fill();
  await Promise.all([
    settingsStore.refreshSecret("webui_password"),
    settingsStore.refreshSecret("library_token"),
    loadBlacklist(),
    loadEvents(),
    loadPluginStatus(),
    style.refreshStatus(),
    sync.refresh(),
  ]);
});
</script>

<template>
  <div class="page workspace-page">
    <header class="page-heading">
      <div>
        <p class="eyebrow">NAI5 · WORKSPACE</p>
        <h1>桌面设置</h1>
        <p>管理软件更新、插件连接、钥匙串、本地模型和导出。</p>
      </div>
      <VCost kind="local" />
    </header>
    <SoftwareUpdate />
    <VCard flush class="card">
      <div class="sec-head">基本设置</div>
      <AsyncState
        :loading="settingsStore.loading"
        :error="settingsStore.readError"
        @retry="settingsStore.reload"
      >
        <div class="form">
          <label
            >插件地址
            <input
              v-model="form.plugin_base_url"
              placeholder="http://127.0.0.1:6185"
          /></label>
          <label>WebUI 用户名 <input v-model="form.webui_username" /></label>
          <label
            >下载并发（1–4）
            <input
              v-model.number="form.download_concurrency"
              type="number"
              min="1"
              max="4"
              style="width: 80px"
          /></label>
          <div>
            <VButton
              class="primary"
              :disabled="settingsStore.saving"
              @click="save"
              >保存设置</VButton
            >
          </div>
        </div>
      </AsyncState></VCard
    >

    <VCard flush class="card">
      <div class="sec-head">钥匙串</div>
      <div class="form">
        <div class="secret">
          <span class="label">WebUI 密码</span>
          <input
            v-model="webuiPassword"
            type="password"
            autocomplete="off"
            placeholder="输入新值"
          />
          <VButton
            @click="
              setSecret(
                'webui_password',
                webuiPassword,
                () => (webuiPassword = ''),
              )
            "
            >保存</VButton
          >
          <span
            class="state"
            :class="{ ok: settingsStore.secrets.webui_password }"
            >{{
              settingsStore.secrets.webui_password === undefined
                ? "未读取"
                : settingsStore.secrets.webui_password
                  ? "已设置"
                  : "未设置"
            }}</span
          >
          <VButton
            v-if="settingsStore.secrets.webui_password"
            class="mini"
            @click="settingsStore.deleteSecret('webui_password')"
            >删除</VButton
          >
        </div>
        <div class="secret">
          <span class="label">library_token</span>
          <input
            v-model="libraryToken"
            type="password"
            autocomplete="off"
            placeholder="输入新值"
          />
          <VButton
            @click="
              setSecret(
                'library_token',
                libraryToken,
                () => (libraryToken = ''),
              )
            "
            >保存</VButton
          >
          <span
            class="state"
            :class="{ ok: settingsStore.secrets.library_token }"
            >{{
              settingsStore.secrets.library_token === undefined
                ? "未读取"
                : settingsStore.secrets.library_token
                  ? "已设置"
                  : "未设置"
            }}</span
          >
          <VButton
            v-if="settingsStore.secrets.library_token"
            class="mini"
            @click="settingsStore.deleteSecret('library_token')"
            >删除</VButton
          >
        </div>
      </div>
    </VCard>

    <VCard flush class="card">
      <div class="sec-head">导出与推送</div>
      <div class="pool-row">
        <span class="label">筛选池</span>
        <VButton :disabled="exporting" @click="doExport">{{
          exporting ? "导出中…" : "导出到本地"
        }}</VButton>
        <VButton class="primary" :disabled="pushing" @click="doPush">{{
          pushing ? "推送中…" : "推送到插件"
        }}</VButton>
        <span class="dim"
          >只含已加入且过黑名单的条目（只有 safe 图包）；插件的 /画 /反推 用它</span
        >
      </div>
      <div class="pool-row">
        <span class="label">完整池</span>
        <VButton :disabled="exportingFull" @click="doExportFull">{{
          exportingFull ? "导出中…" : "导出完整池"
        }}</VButton>
        <VButton class="primary" :disabled="pushingFull" @click="doPushFull">{{
          pushingFull ? "推送中…" : "推送完整池"
        }}</VButton>
        <span class="dim"
          >两本词典全部条目，每条带决策与黑名单标记；插件只接收落盘，不改变现有指令</span
        >
      </div>
      <div v-if="exportResult" class="result">
        <div class="dim">
          筛选池 · 目录 <span class="mono">{{ exportResult.dir }}</span
          ><template v-if="exportResult.skipped">
            · 跳过 {{ exportResult.skipped }} 条</template
          >
        </div>
        <div class="table-scroll">
          <table class="tbl">
            <tbody>
              <tr v-for="f in exportResult.files" :key="f.name">
                <td class="mono">{{ f.name }}</td>
                <td>{{ f.count }} 条</td>
                <td class="mono dim">{{ f.path }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
      <div v-if="pushResult" class="result" :class="{ bad: !pushResult.ok }">
        <div>
          <b>{{ pushResult.ok ? "筛选池推送成功" : "筛选池推送失败" }}</b> ·
          nai5 {{ pushResult.nai5 }} · 图包 {{ pushResult.pack }} · 黑名单剔除
          {{ pushResult.dropped_blacklist }}
        </div>
        <div class="dim">{{ pushResult.message }}</div>
        <div v-if="pushResult.bak.length" class="dim">
          备份：<span v-for="b in pushResult.bak" :key="b" class="mono"
            >{{ b }}
          </span>
        </div>
        <details
          v-if="
            pushResult.plugin_response !== null &&
            pushResult.plugin_response !== undefined
          "
        >
          <summary>插件响应</summary>
          <pre class="mono">{{ pretty(pushResult.plugin_response) }}</pre>
        </details>
      </div>
      <div v-if="exportFullResult" class="result">
        <div class="dim">
          完整池 · 目录 <span class="mono">{{ exportFullResult.dir }}</span
          ><template v-if="exportFullResult.skipped">
            · 跳过 {{ exportFullResult.skipped }} 条</template
          >
        </div>
        <div class="table-scroll">
          <table class="tbl">
            <tbody>
              <tr v-for="f in exportFullResult.files" :key="f.name">
                <td class="mono">{{ f.name }}</td>
                <td>{{ f.count }} 条</td>
                <td class="mono dim">{{ f.path }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
      <div
        v-if="pushFullResult"
        class="result"
        :class="{ bad: !pushFullResult.ok }"
      >
        <div>
          <b>{{ pushFullResult.ok ? "完整池推送成功" : "完整池推送失败" }}</b> ·
          共 {{ pushFullResult.total }} 条
        </div>
        <div class="dim">{{ pushFullResult.message }}</div>
        <div class="table-scroll" v-if="pushFullRows.length">
          <table class="tbl">
            <thead>
              <tr>
                <th>词典</th>
                <th>总数</th>
                <th>加入</th>
                <th>不加入</th>
                <th>未定</th>
                <th>黑名单</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="r in pushFullRows"
                :key="r.key"
                :class="{ sum: r.key === '__sum' }"
              >
                <td>{{ r.label }}</td>
                <td>{{ r.counts.total }}</td>
                <td>{{ r.counts.include }}</td>
                <td>{{ r.counts.exclude }}</td>
                <td>{{ r.counts.undecided }}</td>
                <td>{{ r.counts.blacklisted }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="pushFullResult.bak.length" class="dim">
          备份：<span v-for="b in pushFullResult.bak" :key="b" class="mono"
            >{{ b }}
          </span>
        </div>
        <details
          v-if="
            pushFullResult.plugin_response !== null &&
            pushFullResult.plugin_response !== undefined
          "
        >
          <summary>插件响应</summary>
          <pre class="mono">{{ pretty(pushFullResult.plugin_response) }}</pre>
        </details>
      </div>
    </VCard>

    <VCard flush class="card">
      <div class="sec-head">
        插件当前持有
        <VButton
          class="mini"
          :disabled="statusLoading"
          @click="loadPluginStatus"
          >{{ statusLoading ? "刷新中…" : "刷新" }}</VButton
        >
        <span v-if="pluginStatus?.reachable" class="dim"
          >插件 {{ pluginStatus.plugin_version ?? "未知版本"
          }}<template v-if="statusAt">
            · {{ fmtDateTime(statusAt) }} 查询</template
          ></span
        >
      </div>
      <div v-if="statusError" class="dim bad-text">{{ statusError }}</div>
      <div v-else-if="!pluginStatus" class="dim">
        {{ statusLoading ? "查询中…" : "尚未查询" }}
      </div>
      <div v-else-if="!pluginStatus.reachable" class="dim">
        插件不可达：{{ pluginStatus.message }}
      </div>
      <div v-else class="pools">
        <div v-for="p in poolCards" :key="p.key" class="pool">
          <div class="pool-head">
            {{ p.title }} <span class="dim">{{ p.hint }}</span>
          </div>
          <div class="table-scroll" v-if="p.status">
            <table class="tbl kv">
              <tbody>
                <tr>
                  <td class="dim">导入时间</td>
                  <td>{{ fmtDateTime(p.status.imported_at) }}</td>
                </tr>
                <tr>
                  <td class="dim">release</td>
                  <td class="mono">{{ p.status.release_id ?? "—" }}</td>
                </tr>
                <tr>
                  <td class="dim">导出时间</td>
                  <td>{{ fmtDateTime(p.status.exported_at) }}</td>
                </tr>
                <tr>
                  <td class="dim">桌面端版本</td>
                  <td>{{ p.status.app_version ?? "—" }}</td>
                </tr>
                <tr>
                  <td class="dim">计数</td>
                  <td>{{ fmtPoolCounts(p.status.counts) }}</td>
                </tr>
                <tr>
                  <td class="dim">文件</td>
                  <td>
                    <span class="mono">{{ p.status.file }}</span> ·
                    {{ fmtBytes(p.status.size_bytes) }}
                  </td>
                </tr>
                <tr>
                  <td class="dim">备份</td>
                  <td>{{ p.status.backups.length }} 个</td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-else class="dim">插件尚未收到该池</div>
        </div>
      </div>
    </VCard>

    <VCard flush class="card">
      <div class="sec-head">
        画风模型
        <VButton
          class="mini"
          :disabled="style.statusLoading"
          @click="style.refreshStatus"
          >{{ style.statusLoading ? "刷新中…" : "刷新" }}</VButton
        >
        <span class="dim"
          >CLIP / MegaStyle / CSD 独立保存权重与索引；索引法典与 NAX V5 画师词典，含未定 /
          不加入</span
        >
      </div>
      <div v-if="style.statusError" class="dim bad-text">
        {{ style.statusError }}
      </div>
      <div v-else-if="!style.status" class="dim">
        {{ style.statusLoading ? "查询中…" : "尚未查询" }}
      </div>
      <template v-else>
        <div class="table-scroll">
          <table class="tbl kv">
            <tbody>
              <tr>
                <td class="dim">当前模型</td>
                <td>
                  <select
                    :value="style.status.model"
                    :disabled="modelBusy"
                    aria-label="当前画风模型"
                    @change="selectActiveModel"
                  >
                    <option
                      v-for="m in style.models"
                      :key="m.model"
                      :value="m.model"
                    >
                      {{ m.label }}
                    </option>
                  </select>
                  <div class="dim device-hint">
                    同步时自动补齐此模型索引；精准反推可沿用它或单独指定。切换会保留其他模型的权重与索引。
                  </div>
                </td>
              </tr>
              <tr>
                <td class="dim">目录</td>
                <td class="mono">{{ style.status.dir }}</td>
              </tr>
              <tr>
                <td class="dim">权重</td>
                <td>
                  <span class="state" :class="{ ok: style.status.present }">{{
                    style.status.present
                      ? `已下载 · ${fmtBytes(style.status.size_bytes)}`
                      : `未下载（约 ${fmtBytes(style.status.download_size_bytes)}）`
                  }}</span>
                </td>
              </tr>
              <tr>
                <td class="dim">计算设备</td>
                <td>
                  <div class="device-choice">
                    <select
                      v-model="devicePreference"
                      aria-label="画风计算设备"
                      :disabled="deviceBusy"
                      @change="saveDevicePreference"
                    >
                      <option value="auto">自动（优先 Metal）</option>
                      <option
                        value="metal"
                        :disabled="!style.status.metal_available"
                      >
                        Metal GPU
                      </option>
                      <option value="cpu">CPU</option>
                    </select>
                    <span
                      >实际使用：<span
                        :class="{
                          'ok-text': style.status.active_device === 'metal',
                        }"
                        >{{ activeDeviceLabel }}</span
                      ></span
                    >
                  </div>
                  <div class="dim device-hint">
                    用于向量模型的画风索引与查询；选择后自动保存，下次任务生效，已算索引保留。下方画师分类器使用独立
                    CPU 运行时。
                  </div>
                  <div
                    v-if="style.status.device_message"
                    class="dim device-hint"
                  >
                    {{ style.status.device_message }}
                  </div>
                </td>
              </tr>
              <tr>
                <td class="dim">索引</td>
                <td>
                  已索引 {{ style.status.indexed }} / 应索引
                  {{ style.status.total
                  }}<template v-if="indexPct !== null"
                    >（{{ indexPct }}%）</template
                  >
                  · 均值{{ style.status.mean_ready ? "就绪" : "未算" }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div class="model-grid">
          <article v-for="m in style.models" :key="m.model" class="pool">
            <div class="pool-head">
              {{ m.label }}
              <span v-if="m.model === style.status.model" class="ok-text"
                >当前模型</span
              >
            </div>
            <div class="dim">{{ m.description }}</div>
            <div class="model-facts">
              {{ m.dim }} 维 ·
              {{
                m.present
                  ? `已下载 ${fmtBytes(m.size_bytes)}`
                  : `待下载约 ${fmtBytes(m.download_size_bytes)}`
              }}
            </div>
            <div class="model-facts">
              已索引 {{ m.indexed }} / {{ m.total }} · 均值{{
                m.mean_ready ? "就绪" : "未算"
              }}
            </div>
            <div v-if="m.active_device" class="dim">
              已加载设备：{{
                m.active_device === "metal" ? "Metal GPU" : "CPU"
              }}
            </div>
            <div v-if="m.device_message" class="dim">
              {{ m.device_message }}
            </div>
            <div class="pool-row model-actions">
              <VButton
                :disabled="modelBusy || m.present"
                @click="style.startDownload(m.model)"
                >{{
                  style.downloading && style.downloadModel === m.model
                    ? "下载中…"
                    : m.present
                      ? "已下载"
                      : "下载模型"
                }}</VButton
              >
              <VButton
                :disabled="modelBusy || !m.present"
                title="只计算该模型缺失或图片变更的向量"
                @click="style.startRebuild(false, m.model)"
                >{{
                  style.rebuilding && style.rebuildModel === m.model
                    ? "索引中…"
                    : "补齐索引"
                }}</VButton
              >
              <VButton
                :disabled="modelBusy || !m.present"
                title="仅清除并重算此模型的向量"
                @click="style.startRebuild(true, m.model)"
                >全部重算</VButton
              >
            </div>
          </article>
        </div>
        <div class="pool-row">
          <VButton
            class="primary"
            :disabled="modelBusy || style.statusLoading"
            title="依次下载尚未下载的向量模型和画师分类器，失败或取消时停止队列"
            @click="style.downloadAll()"
            >{{
              style.downloadingAll
                ? `下载全部：${style.downloadPosition} / ${style.downloadQueue.length}`
                : "下载全部模型"
            }}</VButton
          >
          <VButton
            v-if="style.downloading || style.downloadingAll"
            @click="style.cancelDownload"
            >取消下载</VButton
          >
          <VButton v-if="style.rebuilding" @click="style.cancelRebuild"
            >取消索引</VButton
          >
          <span class="dim"
            >首次需分别为向量模型计算词典索引，可能耗时较久；完成后即可同图对比，后续仅补齐新增或变更图片。</span
          >
        </div>
        <ProgressBar
          v-if="style.downloading || style.download"
          :value="downloadValue"
          :danger="!!style.download?.error"
          :label="`${modelLabel(style.downloadModel)} · ${downloadLabel(style.download)}`"
        />
        <ProgressBar
          v-if="style.rebuilding || style.rebuild"
          :value="rebuildValue"
          :danger="style.rebuild?.phase === 'error'"
          :label="`${modelLabel(style.rebuildModel)} · ${rebuildLabel}`"
        />
        <IndexTiming :timing="style.rebuildTiming" />
      </template>
    </VCard>

    <VCard flush class="card">
      <div class="sec-head">
        画师识别模型
        <span class="dim"
          >Kaloscope / Camie · 直接识别原图中的画师标签，无需建索引</span
        >
      </div>
      <div class="dim">
        分类器使用 CPU
        运行时，结果显示在反推实验室的独立对照组中。识别标签不代表原作者，预测分不作为
        NovelAI 权重。
      </div>
      <div v-if="!style.classifiers.length" class="dim">
        {{ style.statusLoading ? "读取模型…" : "尚未读取模型目录" }}
      </div>
      <div class="model-grid">
        <article v-for="m in style.classifiers" :key="m.model" class="pool">
          <div class="pool-head">{{ m.label }}</div>
          <div class="dim">{{ m.description }}</div>
          <div class="model-facts">
            {{
              m.present
                ? "权重已下载"
                : `待下载约 ${fmtBytes(m.download_size_bytes)}`
            }}
            · {{ m.runtime_ready ? "CPU 运行时就绪" : "CPU 运行时未就绪" }}
          </div>
          <div class="dim mono model-dir">{{ m.dir }}</div>
          <div class="model-actions">
            <VButton
              :disabled="modelBusy || (m.present && m.runtime_ready)"
              @click="style.startClassifierDownload(m.model)"
              >{{
                style.downloading && style.downloadModel === m.model
                  ? "下载中…"
                  : !m.present
                    ? "下载识别模型"
                    : !m.runtime_ready
                      ? "补齐运行时"
                      : "已就绪"
              }}</VButton
            >
          </div>
        </article>
      </div>
    </VCard>

    <VCard flush class="card">
      <div class="sec-head">画师黑名单（导出时剔除）</div>
      <div class="inline gap">
        <input
          v-model="newName"
          placeholder="画师名（自动小写、空格→_）"
          @keydown.enter="addBlacklist"
        />
        <input
          v-model="newReason"
          placeholder="原因（可空）"
          @keydown.enter="addBlacklist"
        />
        <VButton @click="addBlacklist">添加</VButton>
      </div>
      <AsyncState
        :loading="blacklistLoading"
        :error="blacklistError"
        @retry="loadBlacklist"
        ><div class="table-scroll" v-if="blacklist.length">
          <table class="tbl">
            <thead>
              <tr>
                <th>画师</th>
                <th>原因</th>
                <th>加入时间</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="b in blacklist" :key="b.name">
                <td class="mono">{{ b.name }}</td>
                <td>{{ b.reason ?? "" }}</td>
                <td class="dim">{{ fmtDateTime(b.added_at) }}</td>
                <td>
                  <VButton class="mini" @click="removeBlacklist(b.name)"
                    >删除</VButton
                  >
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-else class="dim">黑名单为空</div></AsyncState
      >
    </VCard>

    <VCard flush class="card">
      <div class="sec-head">
        最近事件 <VButton class="mini" @click="loadEvents">刷新</VButton>
      </div>
      <AsyncState
        :loading="eventsLoading"
        :error="eventsError"
        @retry="loadEvents"
        ><div class="table-scroll" v-if="events.length">
          <table class="tbl">
            <tbody>
              <tr v-for="e in events" :key="e.id" :class="e.level">
                <td class="dim nowrap">{{ fmtDateTime(e.at) }}</td>
                <td class="nowrap">{{ e.level }}</td>
                <td class="nowrap">{{ e.area }}</td>
                <td>{{ e.message }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-else class="dim">暂无事件</div></AsyncState
      >
    </VCard>
  </div>
</template>

<style scoped>
.page {
  max-width: 1400px;
}
.card {
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 18px;
  box-shadow: var(--shadow);
}
.sec-head {
  font-weight: 600;
  margin-bottom: 10px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.model-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 10px;
  margin: 12px 0;
}
.model-facts {
  margin-top: 6px;
  font-size: 12px;
}
.model-actions {
  margin-top: 10px;
  margin-bottom: 0;
  gap: 6px;
}
.model-dir {
  overflow-wrap: anywhere;
  margin-top: 6px;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  font-size: 13px;
}
.form > label {
  display: flex;
  align-items: center;
  gap: 10px;
}
.form > label input:not([type="checkbox"]) {
  flex: 1;
  max-width: 420px;
}
.inline {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  flex-wrap: wrap;
}
.gap {
  gap: 12px;
  margin-bottom: 10px;
}
.secret {
  display: flex;
  align-items: center;
  gap: 8px;
}
.secret .label {
  width: 110px;
}
.secret input {
  width: 240px;
}
.state {
  color: var(--muted);
  font-size: 12px;
}
.state.ok {
  color: var(--green);
}
.device-choice {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.device-hint {
  margin-top: 5px;
}
.ok-text {
  color: var(--green);
}
.pool-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 10px;
  font-size: 13px;
  flex-wrap: wrap;
}
.pool-row .label {
  width: 56px;
  font-weight: 500;
}
.pool-row + .pb,
.pb + .pb {
  margin-top: 8px;
}
.result {
  margin-top: 10px;
  padding: 10px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  font-size: 13px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.result.bad {
  border-color: var(--danger);
}
.pools {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.pool {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 10px;
  font-size: 13px;
}
.pool-head {
  font-weight: 500;
  margin-bottom: 6px;
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex-wrap: wrap;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 4px 6px;
  border-bottom: 1px solid var(--line);
  vertical-align: top;
}
.tbl th {
  color: var(--muted);
  font-weight: 500;
}
.tbl tr.error td {
  color: var(--danger);
}
.tbl tr.warn td {
  color: var(--paid);
}
.tbl tr.sum td {
  font-weight: 600;
}
.tbl.kv td {
  word-break: break-all;
}
.tbl.kv td:first-child {
  white-space: nowrap;
  width: 84px;
}
.nowrap {
  white-space: nowrap;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
pre.mono {
  white-space: pre-wrap;
  word-break: break-all;
  background: var(--bg);
  padding: 8px;
  border-radius: var(--radius);
  max-height: 300px;
  overflow: auto;
}
.mini {
  padding: 1px 8px;
  font-size: 11px;
}
.dim {
  color: var(--muted);
  font-size: 12px;
}
.bad-text {
  color: var(--danger);
}
</style>

<style scoped>
.actions,
.releases,
.banner-links,
.secret,
.inline {
  flex-wrap: wrap;
}
.card {
  min-width: 0;
}
.sec-head {
  font-size: 15px;
}
.tbl td {
  max-width: 600px;
  overflow-wrap: anywhere;
}
.pool {
  min-width: 0;
}
.form input {
  max-width: 100%;
}
.releases {
  overflow-wrap: anywhere;
}
.pool-row {
  min-width: 0;
}
</style>
