<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NSelect } from "naive-ui";
import { api, errMessage } from "../api";
import { studioApi } from "../api-studio";
import { snapshot } from "../ipc";
import { usePluginModels } from "../stores/plugin-models";
import { usePluginSkills } from "../stores/plugin-skills";
import { PLUGIN_SKILL_ID, skillKind, followsPlugin, freezeSkillGroups, migrateSkillGroups } from "../lib/studio-skills";
import { STUDIO_MODES, STUDIO_PHASE, studioSeed, studioTab, studioTerminal, validCapabilities, validPlan, validPlanForRequest, validRun, validSkill, validPreparedInput } from "../lib/studio";
import type { StudioCapabilities, StudioMode, StudioPlanRequest, StudioReferencePage, StudioRow, StudioRun, StudioSkill, StudioSkillGroup } from "../types-studio";
import type { TreeNode } from "../types";
import VCard from "../ui/VCard.vue";
import VCost from "../ui/VCost.vue";
import VButton from "../ui/VButton.vue";
import StudioSkills from "../components/studio/StudioSkills.vue";
import StudioReview from "../components/studio/StudioReview.vue";
import StudioResults from "../components/studio/StudioResults.vue";
import StudioLegacyHistory from "../components/studio/StudioLegacyHistory.vue";
import StudioSourceImage from "../components/studio/StudioSourceImage.vue";

const route = useRoute(), router = useRouter();
const pluginModels = usePluginModels();
const pluginSkills = usePluginSkills();
const tab = computed(() => studioTab(route.query.mode));
const mode = computed<StudioMode>(() => tab.value === "review" ? "draw" : tab.value);
const capabilities = ref<StudioCapabilities | null>(null), capabilityError = ref<string | null>(null), capabilityLoading = ref(true);
const skills = ref<StudioSkill[]>([]), skillError = ref<string | null>(null);
const groups = ref<StudioSkillGroup[]>([{ id: "default", name: "默认资料", skill_ids: [PLUGIN_SKILL_ID] }]);
const pluginSkillKind = computed(() => skillKind(mode.value));
const pluginSkillState = computed(() => pluginSkills.states[pluginSkillKind.value]);
async function refreshPluginSkill() { if (tab.value !== "review") await pluginSkills.refresh(pluginSkillKind.value).catch(() => {}); }
function emptyForm() { return {
  providers: [] as string[], characters: [] as string[], requirements: "", count: 3,
  orientation: "portrait" as StudioPlanRequest["orientation"], reference_mode: "all" as "sample" | "all",
  reference_content: "image_text" as "image_text" | "text",
  decisions: "include" as "include" | "all", scope: [] as string[], reference_count: 4,
  style_name: "", artist_ratio: 50, brief: "", multi_brief: "", score_provider_ids: [] as string[],
  seed: "", jpeg: false, jpeg_quality: 88, image_path: "", reverse_prompt: "", reverse_separate_prompt: "",
  reverse_separate: false, style_mode: "none" as "none" | "pool" | "precise", style_review_provider_id: "", style_top_k: 5,
}; }
const forms = reactive({ draw: emptyForm(), reverse: emptyForm() });
const form = computed(() => forms[mode.value]);
const referenceChoice = computed({
  get: () => form.value.reference_content === "text" ? `text_${form.value.reference_mode}` : form.value.reference_mode,
  set: (value: string) => {
    if (!["all", "sample", "text_all", "text_sample"].includes(value)) return;
    form.value.reference_content = value.startsWith("text_") ? "text" : "image_text";
    form.value.reference_mode = value.endsWith("sample") ? "sample" : "all";
  },
});
const initialized = new Set<StudioMode>();
const references = ref<StudioReferencePage | null>(null), referenceError = ref<string | null>(null), referenceLoading = ref(false), referenceSearch = ref("");
const directories = ref<{ label: string; value: string }[]>([]), directoryError = ref<string | null>(null);
const rows = ref<StudioRow[]>([]), selectedId = ref<string | null>(null), activeId = ref<string | null>(null);
const selectedRow = computed(() => rows.value.find((r) => r.id === selectedId.value) ?? null);
const activeRow = computed(() => rows.value.find((r) => r.id === activeId.value) ?? null);
const history = ref<StudioRun[] | null>(null), historyLoading = ref(false), historyError = ref<string | null>(null), historySelection = ref<StudioRun | null>(null);
const showingHistory = ref(false), legacyOpen = ref(false);
const actionError = ref<string | null>(null), saveError = ref<string | null>(null), notice = ref<string | null>(null);
const planning = ref(false), preparing = ref(false), starting = ref(false), cancelling = ref(false), stopPlanning = ref(false);
const renderQueue = ref<string[]>([]), stoppingQueue = ref(false), pollError = ref<string | null>(null);
const workspaceReadError = ref<string | null>(null), restoringWorkspace = ref(false), pendingStartId = ref<string | null>(null);
const reviewState = ref<unknown>(null), initialReview = ref<unknown>(null);
let mounted = true, hydrated = false, referenceSequence = 0;
let saveTimer: ReturnType<typeof setTimeout> | undefined, referenceTimer: ReturnType<typeof setTimeout> | undefined, pollTimer: ReturnType<typeof setTimeout> | undefined;
let polling = false, pollEpoch = 0;
const importingImage = ref(false);
const experimentBusy = computed(() => planning.value || preparing.value || starting.value || !!activeId.value);
const busy = computed(() => experimentBusy.value || importingImage.value);
const availableModels = computed(() => pluginModels.ids.filter((p) => capabilities.value?.providers.includes(p)));
function referenceIssue(content: StudioPlanRequest["reference_content"], currentMode: string): string | null {
  if (currentMode === "reverse" || currentMode === "review" || !capabilities.value) return null;
  if (content === "text") return capabilities.value.reference_text_supported === true ? null :
    "当前插件不支持显式纯文字参考，请更新并在 AstrBot 重载薇拉插件，再刷新连接。没有替换你的参考方式。";
  return capabilities.value.reference_images_supported === true ? null :
    "当前插件不支持图文参考，请更新并在 AstrBot 重载薇拉插件，再刷新连接。未退回纯文字构思。";
}
const referenceCapabilityIssue = computed(() => referenceIssue(form.value.reference_content, tab.value));
const modelOptions = computed(() => availableModels.value.map((p) => ({ value: p, label: p })));
const formModelIssue = computed(() => pluginModels.issue([...form.value.providers, ...form.value.score_provider_ids, ...(mode.value === "reverse" && form.value.style_mode === "precise" && form.value.style_review_provider_id ? [form.value.style_review_provider_id] : [])]));
function requestModelIds(req: StudioPlanRequest) { return [req.provider_id, ...req.score_provider_ids, ...(req.reverse?.style_mode === "precise" && req.reverse.style_review_provider_id ? [req.reverse.style_review_provider_id] : [])]; }
const matrixCount = computed(() => form.value.providers.length * groups.value.length);
const selectedPlan = computed(() => showingHistory.value ? historySelection.value?.plan ?? null : selectedRow.value?.plan ?? null);
const selectedRun = computed(() => showingHistory.value ? historySelection.value : selectedRow.value?.run ?? null);
const selectedJpeg = computed(() => showingHistory.value ? false : !!selectedRow.value && Boolean((selectedRow.value as StudioRow & { jpeg_enabled?: boolean }).jpeg_enabled));
const visibleRows = computed(() => rows.value.filter((r) => r.request.mode === mode.value));

function workspace() { return snapshot({ version: 1, skill_follow_version: 1, forms, groups: groups.value, initialized: [...initialized], review: reviewState.value,
  selected_id: selectedId.value, active_id: activeId.value,
  rows: rows.value.map(({ plan, run, ...row }) => ({ ...row, plan_id: plan?.plan_id ?? row.plan_id ?? null, run_id: run?.run_id ?? row.run_id ?? null, restoring: false })),
}); }
async function persist() {
  if (!hydrated) return;
  if (workspaceReadError.value || restoringWorkspace.value) throw new Error("请先恢复读取工作区，避免覆盖已有记录。");
  const data = workspace();
  const pending = studioApi.saveWorkspace(data);
  try { await pending; saveError.value = null; } catch (e) { saveError.value = errMessage(e); throw e; }
}
function queueSave() { if (!hydrated || workspaceReadError.value || restoringWorkspace.value) return; clearTimeout(saveTimer); saveTimer = setTimeout(() => void persist().catch(() => {}), 250); }
watch([forms, groups, rows, selectedId, reviewState, activeId], queueSave, { deep: true });
function defaultsForModes() {
  const cap = capabilities.value; if (!cap) return;
  for (const m of ["draw", "reverse"] as StudioMode[]) {
    if (initialized.has(m)) continue;
    const defaultProvider = cap.defaults[`${m}_provider_id`];
    if (typeof defaultProvider === "string" && defaultProvider) forms[m].providers = [defaultProvider];
    forms[m].brief = cap.templates[`${m}_brief`] ?? "";
    forms[m].multi_brief = cap.templates.multi_brief ?? "";
    forms[m].reverse_prompt = cap.templates.reverse ?? "";
    forms[m].reverse_separate_prompt = cap.templates.reverse_separate ?? "";
    const score = cap.defaults.score_provider_ids;
    forms[m].score_provider_ids = Array.isArray(score) ? score.filter((s): s is string => typeof s === "string" && !!s) : [];
    const rerank = cap.defaults.style_review_provider_id;
    forms[m].style_review_provider_id = typeof rerank === "string" ? rerank : "";
    initialized.add(m);
  }
}
let capabilityGeneration = 0;
async function loadCapabilities() {
  const generation = ++capabilityGeneration;
  capabilityLoading.value = true; capabilityError.value = null;
  try { const [value] = await Promise.all([studioApi.capabilities(), pluginModels.refresh()]); if (generation !== capabilityGeneration || !mounted) return; if (!validCapabilities(value)) throw new Error("总实验室能力响应不完整，请更新并重载插件。"); capabilities.value = value; defaultsForModes(); }
  catch (e) { if (generation === capabilityGeneration && mounted) capabilityError.value = errMessage(e); } finally { if (generation === capabilityGeneration && mounted) capabilityLoading.value = false; }
}
async function loadSkills() {
  skillError.value = null;
  try { const values = await studioApi.skills(); if (!Array.isArray(values) || !values.every(validSkill)) throw new Error("Skill 资料列表结构错误"); skills.value = values;
  } catch (e) { skillError.value = errMessage(e); }
}
async function loadDirectories() {
  try {
    const books = await api.listBooks(); if (!Array.isArray(books)) throw new Error("目录列表结构错误");
    const map = new Map<string, string>();
    const walk = (nodes: TreeNode[], prefix: string[]) => { for (const node of nodes) { const path = [...prefix, node.name]; map.set(JSON.stringify(path), path.join(" / ")); walk(Array.isArray(node.children) ? node.children : [], path); } };
    for (const book of books) if (["nai5_community_pack", "local_pack"].includes(book.codex_id)) walk(book.tree, []);
    directories.value = [...map].map(([value, label]) => ({ value, label }));
  } catch (e) { directoryError.value = errMessage(e); }
}
async function loadReferences() {
  if (tab.value === "reverse" || tab.value === "review") return;
  const seq = ++referenceSequence; referenceLoading.value = true; referenceError.value = null;
  try {
    const value = await studioApi.references({ mode: "draw", decisions: form.value.decisions, scope: form.value.scope.map((s) => JSON.parse(s)), search: referenceSearch.value, offset: 0, limit: 60 });
    if (!value || !Array.isArray(value.items) || !Number.isInteger(value.total) || value.total < 0) throw new Error("参考图包列表结构错误");
    if (seq === referenceSequence) references.value = value;
  } catch (e) { if (seq === referenceSequence) referenceError.value = errMessage(e); }
  finally { if (seq === referenceSequence) referenceLoading.value = false; }
}
watch([tab, () => form.value.decisions, () => form.value.scope, referenceSearch], () => { clearTimeout(referenceTimer); referenceTimer = setTimeout(() => void loadReferences(), 250); }, { deep: true });
watch(tab, (value) => { void loadCapabilities(); void refreshPluginSkill(); if (value !== "review") { selectedId.value = rows.value.find((r) => r.request.mode === value)?.id ?? null; showingHistory.value = false; } });
function request(): StudioPlanRequest {
  const f = form.value;
  if (referenceCapabilityIssue.value) throw new Error(referenceCapabilityIssue.value);
  if (formModelIssue.value) throw new Error(formModelIssue.value);
  if (!["image_text", "text"].includes(f.reference_content) || !["all", "sample"].includes(f.reference_mode)) throw new Error("参考方式无效，请重新选择。");
  if (!Number.isInteger(f.count) || f.count < 1 || f.count > 8) throw new Error("每组张数须为 1–8 的整数");
  if (mode.value === "reverse" && !f.image_path) throw new Error("请先选择反推原图");
  if (!f.providers.length || !groups.value.length) throw new Error("请选择模型和至少一个 skill 组合");
  if (matrixCount.value > 32) throw new Error("单次对照最多 32 个模型 × skill 组合");
  const available = new Set([PLUGIN_SKILL_ID, ...skills.value.map((s) => s.id)]);
  if (groups.value.some((g) => g.skill_ids.some((id) => !available.has(id)))) throw new Error("组合包含已移除的 skill，请重新选择");
  return { mode: mode.value, provider_id: "", skill_ids: [], character: f.characters.length === 1 ? f.characters[0] : "", characters: f.characters.length > 1 ? [...f.characters] : [], requirements: f.requirements,
    count: f.count, orientation: f.orientation, reference_mode: f.reference_mode, ...(mode.value === "reverse" ? {} : { reference_content: f.reference_content }), reference_decisions: f.decisions,
    scope: f.scope.map((s) => JSON.parse(s)), reference_ids: [], reference_count: f.reference_count, style_name: f.style_name, artist_ratio: f.artist_ratio / 100,
    brief: f.brief, multi_brief: f.multi_brief, score_provider_ids: [...f.score_provider_ids], jpeg: { quality: f.jpeg_quality },
    reverse: mode.value !== "reverse" ? null : { image_path: f.image_path, use_separate: f.reverse_separate, prompt: f.reverse_prompt, separate_prompt: f.reverse_separate_prompt,
      style_mode: f.style_mode, style_review_provider_id: f.style_mode === "precise" ? f.style_review_provider_id : "", style_top_k: f.style_top_k },
  };
}
async function composeRow(row: StudioRow) {
  const at = performance.now(); row.phase = "planning"; row.error = null;
  try { await pluginModels.refresh(); if (stopPlanning.value || !mounted) { row.phase = "draft"; return; } const issue = pluginModels.issue(requestModelIds(row.request)) || referenceIssue(row.request.reference_content, row.request.mode); if (issue) throw new Error(issue); const plan = await studioApi.plan(row.request); if (!validPlanForRequest(plan, row.request)) throw new Error("构思返回的参考方式、图片数量或来源与冻结输入不符，未开始出图"); row.plan = plan; row.phase = "planned"; }
  catch (e) { row.error = errMessage(e); row.phase = "plan_error"; }
  finally { row.elapsed_ms = Math.round(performance.now() - at); await persist().catch(() => {}); }
}
async function buildMatrix() {
  if (busy.value) return;
  actionError.value = null; notice.value = null; stopPlanning.value = false;
  try {
    preparing.value = true; await pluginModels.refresh();
    if (stopPlanning.value || !mounted) return;
    const base = request(), seed = studioSeed(form.value.seed) ?? crypto.getRandomValues(new Uint32Array(1))[0];
    const chosenProviders = [...form.value.providers], configuredGroups = snapshot(groups.value), jpeg = form.value.jpeg;
    const currentSkill = followsPlugin(configuredGroups) ? await pluginSkills.refresh(skillKind(base.mode)) : null;
    if (stopPlanning.value || !mounted) return;
    const chosenGroups = freezeSkillGroups(configuredGroups, currentSkill, skillKind(base.mode));
    const frozenSkills = [...skills.value, ...(currentSkill?.skill ? [currentSkill.skill] : [])];
    const { provider_id: _, skill_ids: __, ...common } = base;
    preparing.value = true;
    const input = await studioApi.prepare(common);
    if (!validPreparedInput(input, base.mode, base.reference_content)) throw new Error("冻结输入的参考方式、图片数量或来源不完整，未开始构思，请更新软件与插件后重试。");
    if (stopPlanning.value) return;
    const next = chosenProviders.flatMap((provider_id) => chosenGroups.map((group): StudioRow & { jpeg_enabled: boolean } => ({
      id: crypto.randomUUID(), label: `${provider_id} × ${group.name || '无名称组合'}`, request: { ...base, input_id: input.input_id, provider_id, skill_ids: [...group.skill_ids], character: input.character, characters: input.characters },
      phase: "draft", plan: null, run: null, error: null, elapsed_ms: null, request_id: crypto.randomUUID(), seed, jpeg_enabled: jpeg,
      skill_versions: group.skill_ids.map((id) => { const s = frozenSkills.find((v) => v.id === id); return s ? `${s.name} · ${s.version}` : id; }),
    })));
    rows.value = [...next, ...rows.value].slice(0, 100); selectedId.value = next[0].id; showingHistory.value = false;
    const materials = base.mode === "reverse" ? "同一份反推原图" : base.reference_content === "text" ? `${input.reference_count} 条完整文字（纯文字参考，0 张参考图）` : `${input.reference_count} 条完整文字＋${input.reference_image_count} 张主参考图`;
    notice.value = `已冻结角色 ${input.characters.join('、') || input.character}、${materials}和种子 ${seed}。${next.length} 组使用相同输入。`;
    await persist(); preparing.value = false; planning.value = true;
    for (const item of next) { if (stopPlanning.value || !mounted) break; const row = rows.value.find((r) => r.id === item.id)!; await composeRow(row); }
  } catch (e) { actionError.value = errMessage(e); }
  finally { preparing.value = false; planning.value = false; }
}
async function retryPlan(row: StudioRow) {
  if (busy.value) return; planning.value = true; stopPlanning.value = false;
  try { await composeRow(row); } finally { planning.value = false; }
}
async function startRow(row: StudioRow) {
  if (starting.value || activeId.value || !row.plan) return;
  starting.value = true; pendingStartId.value = row.id; pollError.value = null; row.error = null; row.phase = "starting"; selectedId.value = row.id; showingHistory.value = false;
  try {
    // Persist the exact idempotency key before the potentially billable request.
    await persist();
    const value = await studioApi.start({ plan_id: row.plan.plan_id, request_id: row.request_id, seed: row.seed });
    if (!validRun(value)) throw new Error("开始请求响应不完整；重试会保留相同请求 ID。");
    row.run = value; row.run_id = value.run_id; row.phase = studioTerminal(value.status) ? value.status as StudioRow["phase"] : "running";
    if (!studioTerminal(value.status)) { activeId.value = row.id; if (row.stop_requested) await cancelRow(row); else schedulePoll(); }
    else { await loadHistory(); }
  } catch (e) { row.error = errMessage(e); row.phase = "start_error"; renderQueue.value = []; }
  finally { starting.value = false; pendingStartId.value = null; queueSave(); if (row.phase === "done") void advanceQueue(); }
}
function schedulePoll() { clearTimeout(pollTimer); if (mounted && activeId.value) pollTimer = setTimeout(() => void pollActive(), 2000); }
async function pollActive() {
  const row = activeRow.value; if (!row?.run || polling) return;
  polling = true; const epoch = pollEpoch;
  const id = row.id, runId = row.run.run_id;
  try {
    const value = await studioApi.status(runId); if (!validRun(value)) throw new Error("运行状态响应不完整");
    if (!mounted || activeId.value !== id || epoch !== pollEpoch) return;
    row.run = value; pollError.value = null;
    if (studioTerminal(value.status)) { row.phase = value.status as StudioRow["phase"]; activeId.value = null; cancelling.value = false; queueSave(); await loadHistory(); if (value.status === "done") void advanceQueue(); else renderQueue.value = []; }
  } catch (e) { if (activeId.value === id) pollError.value = errMessage(e); }
  finally { polling = false; schedulePoll(); }
}
async function advanceQueue() {
  if (stoppingQueue.value || activeId.value || starting.value) return;
  const id = renderQueue.value.shift(); if (!id) return;
  const row = rows.value.find((r) => r.id === id); if (row?.plan) await startRow(row);
}
async function startAll() {
  stoppingQueue.value = false; renderQueue.value = visibleRows.value.filter((r) => r.phase === "planned").map((r) => r.id); await advanceQueue();
}
async function stop() {
  stopPlanning.value = true; stoppingQueue.value = true; renderQueue.value = [];
  const row = activeRow.value ?? rows.value.find((r) => r.id === pendingStartId.value);
  if (!row) return;
  row.stop_requested = true; queueSave();
  if (row.run) await cancelRow(row);
  else notice.value = "已记住停止请求，收到运行编号后立即停止；响应未确认时请重试原请求以确认并停止。";
}
async function cancelRow(row: StudioRow) {
  if (!row.run) return;
  pollEpoch++; cancelling.value = true; actionError.value = null;
  try { const value = await studioApi.cancel(row.run.run_id); if (!validRun(value)) throw new Error("停止响应不完整"); row.run = value; if (studioTerminal(value.status)) { row.phase = value.status as StudioRow["phase"]; if (activeId.value === row.id) activeId.value = null; cancelling.value = false; } schedulePoll(); }
  catch (e) { actionError.value = errMessage(e); cancelling.value = false; schedulePoll(); }
}
async function loadHistory() {
  historyLoading.value = true; historyError.value = null;
  try { const value = await studioApi.list(60); if (!Array.isArray(value) || !value.every(validRun)) throw new Error("实验历史返回结构错误"); history.value = value; }
  catch (e) { historyError.value = errMessage(e); } finally { historyLoading.value = false; }
}
async function selectHistory(run: StudioRun) {
  showingHistory.value = true; historySelection.value = run;
  try { const value = await studioApi.status(run.run_id); if (!validRun(value)) throw new Error("历史详情返回结构错误"); if (historySelection.value?.run_id === run.run_id) historySelection.value = value; }
  catch (e) { historyError.value = errMessage(e); }
}
async function restoreRow(row: StudioRow) {
  if (row.restoring) return;
  row.restoring = true; row.error = null;
  try {
    if (!row.plan && row.plan_id) { const p = await studioApi.getPlan(row.plan_id); if (!validPlan(p)) throw new Error("已保存方案结构错误"); row.plan = p; }
    if (row.run_id) { const r = await studioApi.status(row.run_id); if (!validRun(r)) throw new Error("已保存运行结构错误"); row.run = r; row.phase = studioTerminal(r.status) ? r.status as StudioRow["phase"] : "running";
      if (!studioTerminal(r.status) && (!activeId.value || activeId.value === row.id)) { activeId.value = row.id; if (row.stop_requested) await cancelRow(row); else schedulePoll(); }
    }
  } catch (e) { row.error = errMessage(e); }
  finally { row.restoring = false; queueSave(); }
}
async function restore() {
  if (restoringWorkspace.value) return;
  restoringWorkspace.value = true;
  try {
    const raw = await studioApi.workspace();
    if (raw === null) { workspaceReadError.value = null; return; }
    if (!raw || typeof raw !== "object" || Array.isArray(raw) || (raw as Record<string, unknown>).version !== 1) throw new Error("工作区格式无法识别，原记录已保留。");
    const value = raw as Record<string, unknown>;
    rows.value = []; activeId.value = null;
    if (value.forms && typeof value.forms === "object") for (const m of ["draw", "reverse"] as StudioMode[]) {
      const f = (value.forms as Record<string, unknown>)[m];
      if (f && typeof f === "object") { const existing = forms[m]; for (const k of Object.keys(existing) as (keyof typeof existing)[]) { const v = (f as Record<string, unknown>)[k]; if (Array.isArray(existing[k]) ? Array.isArray(v) && v.every((s) => typeof s === "string") : typeof v === typeof existing[k]) Object.assign(existing, { [k]: v }); } }
    }
    if (Array.isArray(value.groups)) { const g = value.groups.filter((x) => x && typeof x.id === "string" && typeof x.name === "string" && Array.isArray(x.skill_ids) && x.skill_ids.every((s: unknown) => typeof s === "string")); if (g.length) groups.value = migrateSkillGroups(g, value.skill_follow_version); }
    if (Array.isArray(value.initialized)) for (const m of value.initialized) if (["draw", "reverse"].includes(m)) initialized.add(m);
    initialReview.value = value.review ?? null; reviewState.value = initialReview.value;
    if (Array.isArray(value.rows)) for (const candidate of value.rows.slice(0, 100)) {
      if (!candidate || typeof candidate.id !== "string" || !candidate.request || typeof candidate.request.provider_id !== "string" || !["draw", "reverse"].includes(candidate.request.mode)) continue;
      const row: StudioRow = { ...candidate, plan: null, run: null, restoring: false, phase: ["planning", "starting"].includes(candidate.phase) ? (candidate.phase === "planning" ? "plan_error" : "start_error") : candidate.phase };
      rows.value.push(row);
      if (row.phase === "running" && row.run_id && !activeId.value) activeId.value = row.id;
      const live = rows.value[rows.value.length - 1]; await restoreRow(live);
      if (candidate.phase === "planning" && !live.error) live.error = "上次构思未确认完成。重试会再次调用模型。";
    }
    if (typeof value.selected_id === "string") selectedId.value = value.selected_id;
    workspaceReadError.value = null;
  } catch (e) { workspaceReadError.value = errMessage(e); }
  finally { restoringWorkspace.value = false; }
}
function onFocus() { void loadCapabilities(); if (!busy.value) void refreshPluginSkill(); }
function onKey(e: KeyboardEvent) { if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && tab.value !== "review" && !busy.value) { e.preventDefault(); void buildMatrix(); } }
onMounted(async () => {
  await restore(); await Promise.all([loadCapabilities(), loadSkills(), refreshPluginSkill(), loadDirectories(), loadHistory()]); hydrated = true; queueSave(); void loadReferences(); schedulePoll(); window.addEventListener("keydown", onKey); window.addEventListener("focus", onFocus);
});
onBeforeUnmount(() => { ++capabilityGeneration; mounted = false; stopPlanning.value = true; renderQueue.value = []; clearTimeout(saveTimer); clearTimeout(referenceTimer); clearTimeout(pollTimer); window.removeEventListener("keydown", onKey); window.removeEventListener("focus", onFocus); void persist().catch(() => {}); });
</script>

<template>
  <div class="workspace-page studio" data-testid="studio">
    <header class="page-heading"><div><p class="eyebrow">NAI5 · EXPERIMENT STUDIO</p><h1>总实验室</h1><p>冻结输入，对照模型与资料，保留每次实验的真实结果。</p></div><div class="cost-strip"><VCost kind="free" label="出图使用免费点数" /><VCost kind="model" label="构思 / 评分另计 API 费用" /></div></header>
    <nav class="mode-tabs" aria-label="实验模式"><button v-for="m in STUDIO_MODES" :key="m.id" :disabled="busy" :class="{ selected: tab === m.id }" :aria-current="tab === m.id ? 'page' : undefined" @click="router.replace({ path: '/studio', query: { mode: m.id } })"><strong>{{ m.label }}</strong><small>{{ m.hint }}</small></button></nav>
    <div v-if="capabilityLoading" class="status-banner" role="status">正在读取插件实验能力…</div>
    <div v-else-if="capabilityError" class="status-banner error" role="alert">{{ capabilityError }}<VButton @click="loadCapabilities">重试连接</VButton></div>
    <div v-else-if="capabilities" class="status-banner" :class="{ error: !capabilities.available }"><span>{{ capabilities.available ? '插件已连接' : '当前无法生成' }} · {{ capabilities.model }} · {{ capabilities.steps }} 步</span><span>{{ capabilities.reason || '插件现有配置保持不变，所有参数仅用于本次实验。' }}</span><VButton @click="loadCapabilities">刷新连接</VButton></div>
    <div v-if="referenceCapabilityIssue && !capabilityLoading && !capabilityError" class="status-banner error" role="alert" data-testid="studio-reference-capability">{{ referenceCapabilityIssue }}</div>
    <div v-if="workspaceReadError" class="status-banner error" role="alert">工作区读取失败：{{ workspaceReadError }}<VButton :disabled="restoringWorkspace" @click="restore">重试读取工作区</VButton></div>
    <div v-if="saveError" class="status-banner error" role="alert">工作区尚未保存：{{ saveError }}<VButton @click="persist().catch(() => {})">重试保存</VButton></div>
    <div class="status-banner" :class="{ error: !!pluginModels.error || !!pluginModels.selection?.enabled && !pluginModels.ids.length }"><span>{{ pluginModels.error || pluginModels.description }}<template v-if="pluginModels.selection?.enabled && !pluginModels.ids.length">。名单中的模型均已失效，请在插件总览调整。</template></span><VButton :disabled="pluginModels.loading" @click="loadCapabilities">刷新模型名单</VButton></div>
    <StudioReview v-show="tab === 'review'" :providers="availableModels" :initial="initialReview" @state="reviewState = $event" />
    <div v-show="tab !== 'review'" class="studio-main">
      <div class="configuration">
        <VCard title="01 · 共同输入" :subtitle="mode === 'reverse' ? '所有组合使用同一份原图副本' : '同一组角色、要求和参考图包贯穿所有对照组合'">
          <template v-if="mode === 'reverse'"><StudioSourceImage v-model="forms.reverse.image_path" :disabled="experimentBusy || restoringWorkspace || !!workspaceReadError" @importing="importingImage = $event" /><label class="check"><input v-model="form.reverse_separate" type="checkbox" :disabled="busy" />分别反推（多角色模板）</label></template>
          <label>角色，可多选<NSelect v-model:value="form.characters" multiple filterable tag :disabled="busy" :options="(capabilities?.characters ?? []).map((c) => ({ value: c, label: c }))" placeholder="留空从插件角色池选择一次" aria-label="实验角色" /></label>
          <label>必须满足的内容，可空<textarea v-model="form.requirements" rows="4" :disabled="busy" placeholder="例如：在沙滩上跳舞，全身远景，夕阳逆光" aria-label="必须满足的内容" /></label>
          <p class="hint">角色留空时只随机一次并冻结；没有内容要求时，在当前资料范围自由构思。</p>
          <template v-if="mode !== 'reverse'">
            <div class="field-pair"><label>图包决策范围<select v-model="form.decisions" :disabled="busy"><option value="include">仅加入</option><option value="all">全部三态</option></select></label><label>参考方式<select v-model="referenceChoice" data-testid="studio-reference-mode" :disabled="busy"><option value="all">全部图文一起传</option><option value="sample">抽样图文参考</option><option value="text_all">全部文一起传</option><option value="text_sample">抽样文参考</option></select></label></div>
            <label>目录范围<NSelect v-model:value="form.scope" multiple filterable clearable :disabled="busy" :options="directories" placeholder="全部目录" aria-label="参考目录范围" /></label>
            <p v-if="directoryError" class="error">{{ directoryError }}</p>
            <label v-if="form.reference_mode === 'sample'" data-testid="studio-reference-sample">抽样条数<input v-model.number="form.reference_count" type="number" min="1" max="8" :disabled="busy" /></label>
            <div data-testid="studio-reference-description"><p class="hint">/画只使用 safe 图包，排除已移除和黑名单。{{ form.reference_mode === 'all' ? '同一次构思发送当前范围的全部合格图包。' : '构思前按条数抽样一次，所有组合共用同一份参考。' }}</p>
            <p v-if="form.reference_content === 'text'" class="hint">只传所选图包的完整主提示词、负面词、角色提示词和来源，不读取或上传参考图片；缺图不影响构思，不要求模型具备视觉能力。全文仍可能增加模型 API 用量，超出模型实际容量会明确报错。</p>
            <p v-else class="hint">每条发送主参考图＋完整提示词，需使用支持看图的模型。主图取已下载的展示图，缺失时使用同一张原图，不使用缩略图；缺图会提示先补齐。图文构思增加模型 API 用量，超出实际容量会报错，不删图或改成纯文字。</p></div>
            <details class="reference-preview"><summary>查看参考图包<template v-if="references && !referenceError && !referenceLoading"> · {{ references.total }} 条匹配</template></summary><input v-model="referenceSearch" placeholder="搜索仅用于浏览，不改变实验范围" aria-label="浏览参考图包" /><p v-if="referenceLoading" role="status">读取图包…</p><p v-else-if="referenceError" class="error">{{ referenceError }} <VButton @click="loadReferences">重试图包</VButton></p><p v-else-if="references?.total === 0" class="hint">此范围没有合格图包，构思会明确拒绝。</p><div v-else class="reference-list"><details v-for="r in references?.items ?? []" :key="r.entry_id"><summary>{{ r.title }} · {{ r.decision === 'include' ? '加入' : r.decision === 'exclude' ? '不加入' : '未定' }}</summary><pre>{{ r.tags }}</pre></details><p v-if="references && references.total > references.items.length" class="hint">这里展示前 {{ references.items.length }} 条；全量实验仍读取整个范围。</p></div></details>
          </template>
        </VCard>
        <VCard title="02 · 模型与 skill 对照" :subtitle="`${matrixCount} 个组合 · 每组 ${form.count} 张${mode === 'reverse' ? '候选图' : '独立构思'}`">
          <label>提示词 / 识图模型，可多选<NSelect v-model:value="form.providers" multiple filterable :disabled="busy || !pluginModels.ready" :options="modelOptions" placeholder="明确选择实验模型" aria-label="实验模型" /></label><p v-if="formModelIssue" class="error" role="alert">{{ formModelIssue }}</p><p v-if="skillError" class="error">{{ skillError }} <VButton @click="loadSkills">重试资料库</VButton></p><StudioSkills v-model:groups="groups" :skills="skills" :plugin-state="pluginSkillState" :kind="pluginSkillKind" :disabled="busy" @refresh="loadSkills" @sync="refreshPluginSkill" />
        </VCard>
        <VCard title="03 · 生成与评估">
          <div class="field-pair"><label>{{ mode === 'reverse' ? '每组候选张数' : '每组不同构思数' }}<input v-model.number="form.count" type="number" min="1" max="8" :disabled="busy" /></label><label>免费尺寸<select v-model="form.orientation" :disabled="busy"><option value="portrait">竖图 832 × 1216</option><option value="landscape">横图 1216 × 832</option><option value="square">方图 1024 × 1024</option><option value="auto">自动选择三档免费尺寸</option></select></label></div>
          <label>固定种子，可空<input v-model="form.seed" :disabled="busy" placeholder="留空时随机一次，所有组合共用" inputmode="numeric" /></label>
          <label>评分模型，可多选<NSelect v-model:value="form.score_provider_ids" multiple filterable :disabled="busy || !pluginModels.ready" :options="modelOptions" placeholder="不评分" aria-label="评分模型" /></label>
          <details><summary>{{ mode === 'reverse' ? '反推模板与画风复审' : '创作简报与画风' }}</summary>
            <template v-if="mode === 'reverse'"><label>普通反推模板<textarea v-model="form.reverse_prompt" rows="6" :disabled="busy" /></label><label>分别反推模板<textarea v-model="form.reverse_separate_prompt" rows="6" :disabled="busy" /></label><label>画风来源<select v-model="form.style_mode" :disabled="busy"><option value="none">不带画风</option><option value="pool">插件画风池</option><option value="precise">精准检索并可复审</option></select></label><template v-if="form.style_mode === 'precise'"><label>画风复审模型<NSelect v-model:value="form.style_review_provider_id" clearable filterable :disabled="busy || !pluginModels.ready" :options="modelOptions" placeholder="不使用视觉复审" aria-label="画风复审模型" @clear="form.style_review_provider_id = ''" /></label><label>画风候选数<input v-model.number="form.style_top_k" type="number" min="2" max="8" :disabled="busy" /></label></template></template>
            <template v-else><label>创作简报<textarea v-model="form.brief" rows="6" :disabled="busy" /></label><label>多人简报<textarea v-model="form.multi_brief" rows="6" :disabled="busy" /></label><label>指定画风名称，可空<input v-model="form.style_name" :disabled="busy" placeholder="插件中的风格名" /></label><label>画风池概率 {{ form.artist_ratio }}%<input v-model.number="form.artist_ratio" type="range" min="0" max="100" :disabled="busy" /></label></template>
          </details>
          <label class="check"><input v-model="form.jpeg" type="checkbox" :disabled="busy" />生成 JPEG 衍生副本，保留 PNG 原图</label><label v-if="form.jpeg">JPEG 质量 {{ form.jpeg_quality }}<input v-model.number="form.jpeg_quality" type="range" min="1" max="100" :disabled="busy" /></label><p class="hint">JPEG 为有损存储副本（4:4:4），不改变生图参数；不保证一定更小。</p>
          <div class="notice">仅 V5、最多 28 步和三档免费尺寸，逐张核验免费资格；不降级使用 Anlas。构思、评分及视觉复审可能产生模型 API 费用。</div>
        </VCard>
      </div>
      <div class="outcomes">
        <VCard title="对照矩阵" subtitle="先构思并检查冻结方案，再明确出图；重试提交保持同一请求 ID">
          <div v-if="notice" class="notice" data-testid="studio-reference-notice">{{ notice }}</div><p v-if="!visibleRows.length" class="empty">配置输入与组合后，点击“构建对照方案”。</p>
          <div v-else class="table-scroll"><table class="matrix"><thead><tr><th>模型 × skill</th><th>状态 / 耗时</th><th>操作</th></tr></thead><tbody><tr v-for="row in visibleRows" :key="row.id" :class="{ selected: !showingHistory && selectedId === row.id }"><td><button class="row-select" @click="selectedId = row.id; showingHistory = false">{{ row.label }}</button><small>seed {{ row.seed }} · {{ row.request.count }} 张</small><small v-if="Array.isArray(row.skill_versions)" data-testid="studio-row-skills">已冻结 skill：{{ row.skill_versions.length ? row.skill_versions.join("；") : "无 skill" }}</small></td><td>{{ STUDIO_PHASE[row.phase] }}<small v-if="row.elapsed_ms !== null">构思 {{ (row.elapsed_ms / 1000).toFixed(1) }} 秒</small><p v-if="row.error" class="error">{{ row.error }}</p></td><td><VButton v-if="(row.plan_id && !row.plan) || (row.run_id && !row.run)" :disabled="row.restoring" @click="restoreRow(row)">{{ row.restoring ? '读取记录中…' : '重试读取记录' }}</VButton><VButton v-if="['draft', 'plan_error'].includes(row.phase)" :disabled="busy" @click="retryPlan(row)">{{ row.phase === 'draft' ? '构思' : '重试构思（API）' }}</VButton><VButton v-if="row.phase === 'planned'" :disabled="busy || !row.plan" class="primary" @click="startRow(row)">出图</VButton><VButton v-if="row.phase === 'start_error' && row.plan" :disabled="busy" @click="startRow(row)">{{ row.stop_requested ? '确认并停止原请求' : '重试原请求' }}</VButton><span v-if="['done', 'error', 'cancelled'].includes(row.phase)" class="hint">{{ row.run?.images.filter((i) => i.status === 'done').length ?? 0 }}/{{ row.request.count }} 已生成</span></td></tr></tbody></table></div>
          <div class="inline"><VButton :disabled="busy || !visibleRows.some((r) => r.phase === 'planned')" @click="startAll">按顺序出图已冻结方案</VButton><span v-if="renderQueue.length" class="hint">后续 {{ renderQueue.length }} 组待执行</span></div>
        </VCard>
        <VCard :title="showingHistory ? '历史结果' : '方案与本次结果'"><StudioResults :plan="selectedPlan" :run="selectedRun" :active="selectedRun?.run_id === activeRow?.run?.run_id" :jpeg="selectedJpeg" :quality="selectedRow?.request.jpeg.quality ?? 88" /></VCard>
        <VCard title="实验历史"><template #actions><VButton :disabled="historyLoading" @click="loadHistory">刷新历史</VButton></template><p v-if="historyLoading" role="status">读取历史…</p><p v-if="historyError" class="error" role="alert">{{ historyError }}</p><p v-else-if="history?.length === 0" class="hint">尚无总实验室出图记录。</p><div class="history-list"><VButton v-for="run in history ?? []" :key="run.run_id" @click="selectHistory(run)">{{ run.created_at }} · {{ run.mode ?? run.plan?.mode }} · {{ run.status }} · {{ run.run_id.slice(0, 10) }}</VButton></div><VButton @click="legacyOpen = !legacyOpen">{{ legacyOpen ? '收起旧版本记录' : '查看旧版本实验记录' }}</VButton><StudioLegacyHistory v-if="legacyOpen" /></VCard>
      </div>
    </div>
    <footer v-if="tab !== 'review'" class="action-bar"><div><strong>{{ activeRow ? `正在生成：${activeRow.label}` : importingImage ? '正在读取反推原图' : planning ? '正在逐组构思' : preparing ? '正在冻结共同输入' : `${matrixCount} 个组合 · ${matrixCount * form.count} 张计划图像` }}</strong><p v-if="pollError" class="error">状态读取失败：{{ pollError }} <VButton @click="pollActive">重试状态</VButton></p><p v-else class="hint">{{ activeId ? '查看历史不会改变停止目标。远端已接收的请求可能继续完成。' : '⌘ / Ctrl + Enter 构建方案；只有点出图才提交图像请求。' }}</p><p v-if="actionError" class="error" role="alert">{{ actionError }}</p></div><div class="inline"><VButton v-if="experimentBusy" :disabled="cancelling || stopPlanning && planning" @click="stop">{{ cancelling ? '停止中…' : activeId || starting ? '停止生成' : '停止后续构思' }}</VButton><VButton class="primary" :disabled="busy || !!workspaceReadError || capabilityLoading || !!capabilityError || !!referenceCapabilityIssue || !!formModelIssue || !capabilities?.available || !matrixCount" @click="buildMatrix">{{ importingImage ? '读取原图中…' : preparing ? '冻结输入中…' : planning ? '构思中…' : '构建对照方案' }}</VButton></div></footer>
  </div>
</template>

<style scoped>
.studio :deep(.n-base-selection-placeholder__inner){color:var(--secondary)}
.studio{max-width:1900px;gap:16px;padding-bottom:0}.cost-strip,.inline{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.mode-tabs{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:8px}.mode-tabs button{display:flex;flex-direction:column;gap:5px;min-width:0;border:1px solid var(--line);border-radius:var(--radius);background:var(--panel);color:var(--ink);padding:12px 14px;text-align:left;cursor:pointer}.mode-tabs small{font-size:11px;color:var(--muted);line-height:1.5}.mode-tabs .selected{border-color:var(--accent);background:var(--accent-soft)}.mode-tabs .selected strong{color:var(--accent)}.status-banner{display:flex;gap:10px;align-items:center;justify-content:space-between;flex-wrap:wrap;padding:11px 14px;border:1px solid var(--line);border-radius:var(--radius);font-size:12px;color:var(--green);background:var(--panel);overflow-wrap:anywhere}.studio-main{display:grid;grid-template-columns:minmax(330px,.86fr) minmax(0,1.4fr);gap:16px;align-items:start}.configuration,.outcomes{display:flex;flex-direction:column;gap:16px;min-width:0}label{display:flex;flex-direction:column;gap:6px;margin:0 0 14px;font-size:13px;min-width:0}input,select,textarea{width:100%;min-width:0}textarea{resize:vertical}.field-pair{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}.check{flex-direction:row;align-items:center;margin:14px 0}.check input{width:24px;height:24px;flex:none}.hint{font-size:12px;color:var(--muted);margin:8px 0;overflow-wrap:anywhere}.error{color:var(--danger);overflow-wrap:anywhere}.notice{font-size:12px;background:var(--paid-soft);color:var(--paid);padding:11px;border-radius:var(--radius);margin:10px 0;overflow-wrap:anywhere}summary{cursor:pointer;padding:10px 0;font-size:13px}.reference-list{max-height:250px;overflow:auto}.reference-list details{border-bottom:1px solid var(--line)}pre{font-size:11px;white-space:pre-wrap;max-height:250px;overflow:auto;overflow-wrap:anywhere;margin:6px 0;padding:10px;background:var(--field)}.table-scroll{max-width:100%;overflow-x:auto;margin:10px 0}.matrix{width:100%;border-collapse:collapse;font-size:12px;min-width:390px}.matrix td,.matrix th{padding:10px 8px;text-align:left;border-bottom:1px solid var(--line);vertical-align:top}.matrix td:first-child{max-width:230px}.matrix small{display:block;color:var(--muted);margin-top:6px}.matrix .selected{background:var(--accent-soft)}.row-select{border:0;padding:4px 0;background:transparent;color:var(--accent);font-size:12px;font-weight:600;text-align:left;cursor:pointer;overflow-wrap:anywhere;min-height:28px}.matrix .error{font-size:11px;max-width:230px}.empty{padding:28px 0;color:var(--muted)}.history-list{display:flex;flex-direction:column;gap:6px;margin-bottom:12px;max-height:270px;overflow:auto}.history-list :deep(.n-button){height:auto;min-height:32px;padding:8px;justify-content:start}.history-list :deep(.n-button__content){white-space:normal;overflow-wrap:anywhere;text-align:left}.action-bar{position:sticky;bottom:0;z-index:10;background:var(--panel);border:1px solid var(--line);border-radius:var(--radius) var(--radius) 0 0;box-shadow:var(--shadow);padding:14px 16px;display:flex;gap:16px;justify-content:space-between;align-items:center}.action-bar>div:first-child{min-width:0}.action-bar strong{font-size:13px;overflow-wrap:anywhere}.action-bar .hint{margin:4px 0 0}.action-bar .error{font-size:12px}.inline input{flex:1}@media(max-width:1150px){.studio-main{grid-template-columns:minmax(0,1fr)}.configuration{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));align-items:start}.configuration>:last-child{grid-column:1 / -1}.mode-tabs small{display:none}}@media(max-width:700px){.configuration{display:flex}.mode-tabs{gap:5px}.mode-tabs button{padding:10px 8px}.mode-tabs strong{font-size:12px}.action-bar{flex-direction:column;align-items:stretch}.action-bar>.inline{justify-content:flex-end}.field-pair{gap:8px}}
</style>
