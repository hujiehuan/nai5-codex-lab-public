import type {
  CreativeCapabilities,
  CreativeOrientation,
  CreativePlan,
  CreativePlanRequest,
  CreativeReference,
  CreativeReferenceMode,
  CreativeRun,
  CreativeSize,
  CreativeStartRequest,
} from "../types-creative";
import { LAB_MAX_SEED, parseSeed } from "./lab";

export const CREATIVE_SIZES: {
  id: CreativeOrientation;
  label: string;
  size: CreativeSize;
  width: number;
  height: number;
}[] = [
  { id: "portrait", label: "竖图", size: "832x1216", width: 832, height: 1216 },
  {
    id: "landscape",
    label: "横图",
    size: "1216x832",
    width: 1216,
    height: 832,
  },
  { id: "square", label: "方图", size: "1024x1024", width: 1024, height: 1024 },
];
export interface CreativeForm {
  character: string;
  requirements: string;
  provider_id: string;
  orientation: CreativeOrientation;
  scope: string[][];
  reference_ids: string[];
  reference_count: number;
  reference_mode: CreativeReferenceMode;
  style_name: string;
  artist_ratio: number;
  count: number;
  seed: string;
  score: boolean;
}
export function defaultCreativeForm(): CreativeForm {
  return {
    character: "",
    requirements: "",
    provider_id: "",
    orientation: "portrait",
    scope: [],
    reference_ids: [],
    reference_count: 4,
    reference_mode: "sample",
    style_name: "",
    artist_ratio: 50,
    count: 1,
    seed: "",
    score: true,
  };
}
function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function strings(value: unknown): value is string[] {
  return (
    Array.isArray(value) && value.every((item) => typeof item === "string")
  );
}
export function isCreativeCapabilities(
  value: unknown,
): value is CreativeCapabilities {
  if (
    !record(value) ||
    !Array.isArray(value.free_sizes) ||
    !value.free_sizes.every(
      (size) =>
        record(size) &&
        typeof size.id === "string" &&
        typeof size.width === "number" &&
        typeof size.height === "number",
    )
  )
    return false;
  return (
    typeof value.version === "number" &&
    (value.full_reference_supported === undefined ||
      typeof value.full_reference_supported === "boolean") &&
    typeof value.free_only === "boolean" &&
    strings(value.providers) &&
    strings(value.characters) &&
    typeof value.model === "string" &&
    typeof value.steps === "number" &&
    typeof value.available === "boolean" &&
    typeof value.render_available === "boolean" &&
    ["reason", "render_reason"].every(
      (key) => value[key] === null || typeof value[key] === "string",
    ) &&
    typeof value.min_stamina_percent === "number" &&
    Number.isFinite(value.min_stamina_percent) &&
    strings(value.free_limitations)
  );
}
export function isCreativeReference(
  value: unknown,
): value is CreativeReference {
  if (
    !record(value) ||
    !["entry_id", "title", "tags", "content_hash"].every(
      (key) => typeof value[key] === "string",
    ) ||
    !strings(value.path)
  )
    return false;
  return (
    value.decision === "include" &&
    value.rating === "safe" &&
    typeof value.prose === "boolean" &&
    (value.negative === null || typeof value.negative === "string") &&
    Array.isArray(value.character_prompts) &&
    value.character_prompts.every(
      (item) =>
        record(item) &&
        typeof item.label === "string" &&
        typeof item.prompt === "string",
    )
  );
}
/** Validate persisted/plugin data before selecting it into editable or runnable UI state. */
export function isCreativePlan(value: unknown): value is CreativePlan {
  if (!record(value)) return false;
  const textFields = [
    "plan_id",
    "created_at",
    "character",
    "requirements",
    "provider_id",
    "skill_version",
    "concept",
    "prompt",
    "negative",
    "artist",
    "artist_source",
    "model",
    "size",
    "llm_raw",
  ];
  if (
    !textFields.every((key) => typeof value[key] === "string") ||
    !value.plan_id
  )
    return false;
  if (value.parent_plan_id !== null && typeof value.parent_plan_id !== "string")
    return false;
  if (
    typeof value.free_only !== "boolean" ||
    typeof value.steps !== "number" ||
    !Number.isFinite(value.steps)
  )
    return false;
  if (!strings(value.reference_ids) || !strings(value.warnings)) return false;
  if (
    value.reference_mode !== undefined &&
    value.reference_mode !== "sample" &&
    value.reference_mode !== "all"
  )
    return false;
  if (
    value.used_reference_ids !== undefined &&
    (!strings(value.used_reference_ids) ||
      value.used_reference_ids.some(
        (id) => !(value.reference_ids as string[]).includes(id),
      ) ||
      new Set(value.used_reference_ids).size !==
        value.used_reference_ids.length)
  )
    return false;
  if (
    !Array.isArray(value.character_prompts) ||
    !value.character_prompts.every(
      (item) =>
        record(item) &&
        ["prompt", "negative_prompt", "position"].every(
          (key) => typeof item[key] === "string",
        ),
    )
  )
    return false;
  if (
    !Array.isArray(value.requirement_checks) ||
    !value.requirement_checks.every(
      (item) =>
        record(item) &&
        typeof item.requirement === "string" &&
        typeof item.evidence === "string",
    )
  )
    return false;
  const params = value.parameters;
  return (
    record(params) &&
    ["scale", "cfg"].every(
      (key) => typeof params[key] === "number" && Number.isFinite(params[key]),
    ) &&
    ["sampler", "noise_schedule", "other"].every(
      (key) => typeof params[key] === "string",
    )
  );
}
export function isCreativeRun(value: unknown): value is CreativeRun {
  if (
    !record(value) ||
    !isCreativePlan(value.plan) ||
    typeof value.run_id !== "string" ||
    typeof value.plan_id !== "string" ||
    typeof value.created_at !== "string" ||
    typeof value.request_id !== "string"
  )
    return false;
  if (
    !["queued", "running", "done", "error", "cancelled"].includes(
      String(value.status),
    )
  )
    return false;
  if (
    typeof value.free_only !== "boolean" ||
    (value.anlas_spent !== null &&
      (typeof value.anlas_spent !== "number" ||
        !Number.isFinite(value.anlas_spent)))
  )
    return false;
  if (!strings(value.warnings) || typeof value.cancel_requested !== "boolean")
    return false;
  if (
    !["stamina_before", "stamina_after"].every((key) =>
      nullableNumber(value[key]),
    ) ||
    !["finished_at", "error"].every(
      (key) => value[key] === null || typeof value[key] === "string",
    )
  )
    return false;
  return (
    Array.isArray(value.images) &&
    value.images.every(
      (item) =>
        record(item) &&
        typeof item.idx === "number" &&
        Number.isInteger(item.idx) &&
        typeof item.seed === "number" &&
        Number.isInteger(item.seed) &&
        ["pending", "running", "done", "error", "cancelled"].includes(
          String(item.status),
        ) &&
        strings(item.notes) &&
        [
          "score",
          "account_index",
          "anlas_before",
          "anlas_after",
          "anlas_delta",
          "stamina_before",
          "stamina_after",
        ].every((key) => nullableNumber(item[key])) &&
        ["file", "error"].every(
          (key) => item[key] === null || typeof item[key] === "string",
        ),
    )
  );
}
function nullableNumber(value: unknown): boolean {
  return (
    value === null || (typeof value === "number" && Number.isFinite(value))
  );
}
export function creativeCapabilitiesIssue(
  cap: CreativeCapabilities | null,
): string | null {
  if (!cap) return "创作接口尚未就绪。请连接支持创作模式的插件。";
  if (cap.version !== 1 || cap.free_only !== true)
    return "插件未确认免费出图专用协议，暂不能创作或出图。";
  if (
    !Array.isArray(cap.free_sizes) ||
    !CREATIVE_SIZES.every((size) =>
      cap.free_sizes.some(
        (s) =>
          s.id === size.id &&
          s.width === size.width &&
          s.height === size.height,
      ),
    )
  )
    return "插件未支持约定的三种免费尺寸。";
  if (!cap.available) return cap.reason || "插件当前未开放创作接口。";
  return null;
}
export function creativeReferenceIssue(
  cap: CreativeCapabilities | null,
  mode: CreativeReferenceMode,
): string | null {
  return mode === "all" && cap && cap.full_reference_supported !== true
    ? "当前插件不支持全量参考，请更新并重载薇拉插件，再刷新连接状态；不会改为抽样。"
    : null;
}
export function loadCreativeReferenceMode(): CreativeReferenceMode {
  try {
    return localStorage.getItem("codexlab.creativeReferenceMode") === "all"
      ? "all"
      : "sample";
  } catch {
    return "sample";
  }
}
export function saveCreativeReferenceMode(mode: CreativeReferenceMode): void {
  try {
    localStorage.setItem("codexlab.creativeReferenceMode", mode);
  } catch {
    /* Storage may be unavailable. */
  }
}
export function creativeRenderIssue(
  cap: CreativeCapabilities | null,
): string | null {
  const issue = creativeCapabilitiesIssue(cap);
  if (issue || !cap) return issue;
  if (cap.render_available !== true)
    return cap.render_reason || "当前不满足免费出图条件。";
  if (cap.min_stamina_percent < 5) return "插件点数保护阈值低于约定的 5%。";
  if (
    !Number.isInteger(cap.steps) ||
    cap.steps < 1 ||
    cap.steps > 28 ||
    !["nai-diffusion-5-full", "nai-diffusion-5-curated"].includes(cap.model)
  )
    return "当前模型或步数不符合约定的免费出图规格。";
  return null;
}
export function creativePlanIssue(plan: CreativePlan): string | null {
  if (!isCreativePlan(plan)) return "方案记录格式无效，请重新读取或生成方案。";
  if (
    plan.free_only !== true ||
    !CREATIVE_SIZES.some((size) => size.size === plan.size)
  )
    return "方案没有使用约定的免费尺寸，不能出图。";
  if (!plan.prompt.trim()) return "方案缺少实际提示词，不能出图。";
  if (
    !Number.isInteger(plan.steps) ||
    plan.steps < 1 ||
    plan.steps > 28 ||
    !["nai-diffusion-5-full", "nai-diffusion-5-curated"].includes(plan.model)
  )
    return "方案模型或步数不符合免费出图规格。";
  return null;
}
export function creativePlanRequest(form: CreativeForm): CreativePlanRequest {
  if (!["sample", "all"].includes(form.reference_mode))
    throw new Error("参考模式无效。");
  if (!CREATIVE_SIZES.some((size) => size.id === form.orientation))
    throw new Error("请选择免费尺寸。");
  if (!form.provider_id.trim()) throw new Error("请选择提示词模型。");
  if (
    form.reference_mode === "sample" &&
    (!Number.isInteger(form.reference_count) ||
      form.reference_count < 3 ||
      form.reference_count > 6)
  )
    throw new Error("参考数量应为 3–6 条。");
  if (form.reference_mode === "sample" && form.reference_ids.length > 6)
    throw new Error("最多选择 6 条重点参考。");
  if (
    !Number.isFinite(form.artist_ratio) ||
    form.artist_ratio < 0 ||
    form.artist_ratio > 100
  )
    throw new Error("画风池概率应在 0–100% 之间。");
  return {
    character: form.character.trim(),
    requirements: form.requirements,
    provider_id: form.provider_id,
    orientation: form.orientation,
    scope: form.scope.map((path) => [...path]),
    reference_ids: [...new Set(form.reference_ids)],
    reference_count: form.reference_count,
    reference_mode: form.reference_mode,
    style_name: form.style_name.trim(),
    artist_ratio: form.artist_ratio / 100,
  };
}
/** Rendering takes only the frozen plan id; no prompt generation request is rebuilt here. */
export function creativeStartRequest(
  plan: CreativePlan,
  form: Pick<CreativeForm, "count" | "seed" | "score">,
  requestId: string,
  newSeed = false,
): CreativeStartRequest {
  const issue = creativePlanIssue(plan);
  if (issue) throw new Error(issue);
  const count = newSeed ? 1 : form.count;
  if (!Number.isInteger(count) || count < 1 || count > 8)
    throw new Error("张数应为 1–8 的整数。");
  const seed = newSeed ? null : parseSeed(form.seed);
  if (seed === undefined || (seed !== null && seed + count - 1 > LAB_MAX_SEED))
    throw new Error("种子无效，或连续种子超出允许范围。");
  return {
    plan_id: plan.plan_id,
    request_id: requestId,
    count,
    seed,
    score: form.score,
  };
}
export function referenceInScope(
  reference: Pick<CreativeReference, "path">,
  scope: string[][],
): boolean {
  return (
    !scope.length ||
    scope.some((prefix) =>
      prefix.every((part, index) => reference.path[index] === part),
    )
  );
}
export function creativeTerminal(status: string): boolean {
  return ["done", "error", "cancelled"].includes(status);
}
export const CREATIVE_STATUS: Record<string, string> = {
  queued: "排队中",
  running: "生成中",
  pending: "等待生成",
  done: "已完成",
  error: "失败",
  cancelled: "已停止",
};
