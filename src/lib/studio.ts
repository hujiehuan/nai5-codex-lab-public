import type { StudioCapabilities, StudioPlan, StudioRun, StudioTab, StudioRow, StudioSkill, StudioPreparedInput, StudioReferenceContent, StudioPlanRequest } from "../types-studio";

export const STUDIO_MODES: { id: StudioTab; label: string; hint: string }[] = [
  { id: "reverse", label: "反推", hint: "同一原图，对照识图模板、模型与重画" },
  { id: "draw", label: "/画", hint: "精选构图参考，同一要求下生成不同构思" },
  { id: "review", label: "画风复审", hint: "同一组图片，对照视觉模型与本地编码器" },
];
export const STUDIO_PHASE: Record<StudioRow["phase"], string> = {
  draft: "待构思", planning: "构思中", planned: "方案已冻结", plan_error: "构思失败",
  starting: "正在提交", start_error: "提交未确认", running: "生成中", done: "已完成", error: "未全部完成", cancelled: "已停止",
};
export function studioTab(value: unknown): StudioTab {
  return STUDIO_MODES.some((v) => v.id === value) ? value as StudioTab : "draw";
}
export function studioTerminal(value: string): boolean { return ["done", "error", "cancelled"].includes(value); }
export function studioSeed(value: string): number | null {
  if (!value.trim()) return null;
  if (!/^\d+$/.test(value.trim())) throw new Error("种子须为 0–4294967295 的整数");
  const n = Number(value);
  if (!Number.isSafeInteger(n) || n < 0 || n > 4294967295) throw new Error("种子须为 0–4294967295 的整数");
  return n;
}
const record = (x: unknown): x is Record<string, unknown> => !!x && typeof x === "object" && !Array.isArray(x);
const strings = (x: unknown): x is string[] => Array.isArray(x) && x.every((v) => typeof v === "string");
export function validCapabilities(x: unknown): x is StudioCapabilities {
  return record(x) && x.version === 1 && strings(x.providers) && strings(x.characters) &&
    Array.isArray(x.free_sizes) && typeof x.available === "boolean" && typeof x.model === "string" &&
    typeof x.steps === "number" && record(x.defaults) && record(x.templates) &&
    (x.reference_images_supported === undefined || typeof x.reference_images_supported === "boolean") &&
    (x.reference_text_supported === undefined || typeof x.reference_text_supported === "boolean");
}
export function validPreparedInput(x: unknown, mode: string, content: StudioReferenceContent = "image_text"): x is StudioPreparedInput {
  if (!record(x) || typeof x.input_id !== "string" || !x.input_id || typeof x.character !== "string" ||
    !strings(x.characters) || !strings(x.reference_ids) || x.reference_ids.some((id) => !id) ||
    new Set(x.reference_ids).size !== x.reference_ids.length || x.reference_count !== x.reference_ids.length) return false;
  if (mode === "reverse") return x.reference_count === 0 && (x.reference_image_count === undefined || x.reference_image_count === 0);
  if (!x.reference_ids.length) return false;
  return content === "text" ? x.reference_content === "text" && x.reference_image_count === 0 :
    (x.reference_content === undefined || x.reference_content === "image_text") && x.reference_image_count === x.reference_count;
}
function validReferenceManifest(x: Record<string, unknown>): boolean {
  const settings = x.settings as Record<string, unknown>;
  if (settings.reference_content !== undefined && settings.reference_content !== "image_text" && settings.reference_content !== "text") return false;
  // Old frozen plans remain readable; they must never be labelled image+text.
  if (settings.reference_images === undefined && settings.reference_image_count === undefined) return settings.reference_content === undefined;
  if (!Array.isArray(settings.reference_images) || settings.reference_image_count !== settings.reference_images.length) return false;
  const images = settings.reference_images;
  if (settings.reference_content === "text") {
    if (images.length !== 0 || x.mode === "reverse") return false;
    const plans = x.plans as Record<string, unknown>[], ids = plans[0].reference_ids;
    return strings(ids) && ids.length > 0 && ids.every(Boolean) && new Set(ids).size === ids.length && plans.every((plan) =>
      strings(plan.reference_ids) && plan.reference_ids.length === ids.length && plan.reference_ids.every((id, i) => id === ids[i]));
  }
  if (!images.every((image) => record(image) && typeof image.entry_id === "string" && !!image.entry_id && image.ordinal === 0 &&
    ["image/png", "image/jpeg", "image/webp"].includes(String(image.mime)) && typeof image.sha256 === "string" && /^[a-f0-9]{64}$/.test(image.sha256) &&
    (image.raw_tag === null || typeof image.raw_tag === "string"))) return false;
  const ids = images.map((image) => image.entry_id);
  if (new Set(ids).size !== ids.length) return false;
  if (x.mode === "reverse") return images.length === 0;
  return images.length > 0 && (x.plans as Record<string, unknown>[]).every((plan) =>
    strings(plan.reference_ids) && plan.reference_ids.length === ids.length && plan.reference_ids.every((id, i) => id === ids[i]));
}
export function validSkill(x: unknown): x is StudioSkill {
  return record(x) && ["id", "name", "version", "sha256", "text", "source"].every((k) => typeof x[k] === "string") &&
    typeof x.builtin === "boolean" && Number.isInteger(x.bytes) && (x.bytes as number) >= 0;
}
export function validPlan(x: unknown): x is StudioPlan {
  return record(x) && typeof x.plan_id === "string" && x.plan_id.length > 0 &&
    ["draw", "reverse"].includes(String(x.mode)) && Array.isArray(x.plans) && x.plans.length > 0 &&
    x.plans.every((p) => record(p) && typeof p.prompt === "string" && typeof p.negative === "string") &&
    record(x.settings) && strings(x.warnings) && validReferenceManifest(x);
}
/** New plans must match their frozen request, independent of the current form. */
export function validPlanForRequest(plan: unknown, request: Pick<StudioPlanRequest, "mode" | "reference_content">): plan is StudioPlan {
  if (!validPlan(plan) || plan.mode !== request.mode) return false;
  if (request.mode === "reverse") return true;
  if (request.reference_content === "text") return plan.settings.reference_content === "text";
  if (plan.settings.reference_content === "text") return false;
  return Number.isInteger(plan.settings.reference_image_count) && Number(plan.settings.reference_image_count) > 0;
}
export function validRun(x: unknown): x is StudioRun {
  return record(x) && typeof x.run_id === "string" && typeof x.plan_id === "string" &&
    ["queued", "running", "done", "error", "cancelled"].includes(String(x.status)) &&
    Array.isArray(x.images) && x.images.every((v) => record(v) && Number.isInteger(v.idx) && typeof v.status === "string") &&
    strings(x.warnings) && (x.anlas_spent === null || typeof x.anlas_spent === "number");
}
export function pairKey(a: string, b: string): string { return JSON.stringify([a, b].sort()); }
export interface PairMetrics { labelled: number; evaluated: number; correct: number; accuracy: number | null; precision: number | null; recall: number | null }
/** Missing labels or missing predictions never become false negatives. */
export function pairMetrics(images: { id: string; group: string }[], predictions: { left_id: string; right_id: string; same: boolean }[]): PairMetrics {
  const groups = new Map(images.map((x) => [x.id, x.group.trim()]));
  const truth = new Map<string, boolean>();
  for (let i = 0; i < images.length; i++) for (let j = i + 1; j < images.length; j++) {
    const a = groups.get(images[i].id), b = groups.get(images[j].id);
    if (a && b) truth.set(pairKey(images[i].id, images[j].id), a === b);
  }
  let tp = 0, fp = 0, fn = 0, correct = 0, evaluated = 0;
  const seen = new Set<string>();
  for (const p of predictions) {
    const key = pairKey(p.left_id, p.right_id);
    if (!truth.has(key) || seen.has(key)) continue;
    seen.add(key); evaluated++;
    const actual = truth.get(key)!;
    if (actual === p.same) correct++;
    if (actual && p.same) tp++; else if (!actual && p.same) fp++; else if (actual && !p.same) fn++;
  }
  return { labelled: truth.size, evaluated, correct, accuracy: evaluated ? correct / evaluated : null,
    precision: tp + fp ? tp / (tp + fp) : null, recall: tp + fn ? tp / (tp + fn) : null };
}
export const percent = (n: number | null) => n === null ? "—" : `${(n * 100).toFixed(1)}%`;
export const bytesLabel = (n: number) => n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KiB` : `${(n / 1024 / 1024).toFixed(2)} MiB`;
