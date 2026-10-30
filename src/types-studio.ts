import type { CreativePlan, CreativeImage, CreativeOrientation } from "./types-creative";
import type { Decision, AppError } from "./types";

export type StudioMode = "reverse" | "draw";
export type StudioTab = StudioMode | "review";
export type StudioReferenceContent = "image_text" | "text";
export interface StudioSkill {
  id: string; name: string; version: string; sha256: string; text: string;
  source: string; builtin: boolean; bytes: number;
}
export type StudioSkillKind = "draw" | "reverse";
export interface StudioPluginSkill {
  version: 1; kind: StudioSkillKind; enabled: boolean; selected_id: string;
  skill: StudioSkill | null;
}
export interface StudioReference {
  entry_id: string; title: string; path: string[]; tags: string;
  negative: string | null; character_prompts: { label: string; prompt: string; negative?: string }[];
  content_hash: string; decision: Decision; rating: "safe"; prose: boolean;
}
export interface StudioReferenceQuery {
  mode: "draw"; decisions: "include" | "all"; search?: string;
  scope?: string[][]; offset?: number; limit?: number;
}
export interface StudioReferencePage { items: StudioReference[]; total: number }
export interface StudioJpeg { quality: number }
export interface StudioReverse {
  image_path: string; use_separate: boolean; prompt: string; separate_prompt: string;
  style_mode: "none" | "pool" | "precise"; style_review_provider_id: string; style_top_k: number;
}
export interface StudioPlanRequest {
  input_id?: string;
  mode: StudioMode; provider_id: string; character: string; characters: string[];
  requirements: string; count: number; orientation: CreativeOrientation | "auto";
  reference_mode: "sample" | "all"; reference_content?: StudioReferenceContent; reference_decisions: "include" | "all";
  scope: string[][]; reference_ids: string[]; reference_count: number;
  style_name: string; artist_ratio: number; skill_ids: string[]; brief: string;
  multi_brief: string; reverse: StudioReverse | null; score_provider_ids: string[];
  jpeg: StudioJpeg;
}
export interface StudioPreparedInput { input_id: string; character: string; characters: string[]; reference_count: number; reference_ids: string[]; reference_image_count: number; reference_content?: StudioReferenceContent }
export interface StudioReferenceImage {
  entry_id: string; ordinal: 0; mime: "image/png" | "image/jpeg" | "image/webp";
  sha256: string; raw_tag: string | null;
}
export interface StudioCapabilities {
  version: number; providers: string[]; characters: string[];
  reference_images_supported?: boolean;
  reference_text_supported?: boolean;
  free_sizes: { id: CreativeOrientation; width: number; height: number }[];
  model: string; steps: number; available: boolean; reason?: string | null;
  defaults: Record<string, unknown>; templates: Record<string, string>;
}
export interface StudioPlan {
  plan_id: string; created_at: string; mode: StudioMode; plans: CreativePlan[];
  settings: Record<string, unknown>; warnings: string[];
}
export interface StudioRun {
  run_id: string; plan_id: string; request_id: string; status: "queued" | "running" | "done" | "error" | "cancelled";
  created_at: string; finished_at: string | null; images: (CreativeImage & { plan_id?: string; judges?: { provider_id: string; score: number | null; error: string | null; latency_ms: number }[] })[];
  error: string | null; warnings: string[]; cancel_requested: boolean;
  anlas_spent: number | null; stamina_before?: number | null; stamina_after?: number | null;
  plan?: StudioPlan; mode?: StudioMode;
}
export interface StudioImageResult {
  path: string; original_path: string; format: "png" | "jpeg";
  original_bytes: number; encoded_bytes: number; quality?: number | null;
}
export interface StudioCompareImage { id: string; path: string }
export interface StudioComparePair {
  left_id: string; right_id: string; same_style: boolean | null; reason?: string;
}
export interface StudioCompareRank {
  rank: number; image_id: string; similarity: number; reason: string;
}
export interface StudioCompareResult {
  provider_id: string; text: string; latency_ms: number;
  reference_id?: string; ranking?: StudioCompareRank[];
  /** Pairwise judgments only exist on history created before the ranking protocol. */
  pairs?: StudioComparePair[];
}
export interface StyleCompareResult {
  images: { id: string; path: string; width: number; height: number; sha256: string }[];
  results: {
    model: string; label: string; model_version: string; preprocessing: string;
    device: "cpu" | "metal" | null; fallback_reason: string | null;
    load_ms: number; encode_ms: number; total_ms: number;
    pairs: { left_id: string; right_id: string; cosine: number }[];
    error: AppError | null;
  }[];
  elapsed_ms: number; centered: false;
}
export interface StudioSkillGroup { id: string; name: string; skill_ids: string[] }
export interface StudioRow {
  id: string; label: string; request: StudioPlanRequest; seed: number | null;
  phase: "draft" | "planning" | "planned" | "plan_error" | "starting" | "start_error" | "running" | "done" | "error" | "cancelled";
  plan: StudioPlan | null; run: StudioRun | null; request_id: string;
  plan_id?: string | null; run_id?: string | null; stop_requested?: boolean; restoring?: boolean;
  error: string | null; elapsed_ms: number | null;
  skill_versions?: string[];
}
