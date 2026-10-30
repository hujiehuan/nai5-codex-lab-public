/** Dedicated creative draw protocol. All rendering is stamina-only. */
export type CreativeReferenceMode = "sample" | "all";
export type CreativeOrientation = "portrait" | "landscape" | "square";
export type CreativeSize = "832x1216" | "1216x832" | "1024x1024";
export interface CreativeCapabilities {
  version: number;
  full_reference_supported?: boolean;
  free_only: boolean;
  free_sizes: { id: CreativeOrientation; width: number; height: number }[];
  providers: string[];
  characters: string[];
  model: string;
  steps: number;
  available: boolean;
  reason: string | null;
  render_available: boolean;
  render_reason: string | null;
  min_stamina_percent: number;
  free_limitations: string[];
}
export interface CreativeReference {
  entry_id: string;
  title: string;
  path: string[];
  tags: string;
  negative: string | null;
  character_prompts: { label: string; prompt: string; negative?: string }[];
  content_hash: string;
  decision: "include";
  rating: "safe";
  prose: boolean;
}
export interface CreativeReferenceQuery {
  scope: string[][];
  search?: string;
  offset?: number;
  limit?: number;
}
export interface CreativeReferencePage {
  items: CreativeReference[];
  total: number;
}
export interface CreativePlanRequest {
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
}
export interface CreativeCharacterPrompt {
  prompt: string;
  negative_prompt: string;
  position: string;
}
export interface CreativePlan {
  plan_id: string;
  parent_plan_id: string | null;
  created_at: string;
  character: string;
  requirements: string;
  provider_id: string;
  skill_version: string;
  concept: string;
  prompt: string;
  negative: string;
  artist: string;
  artist_source: string;
  character_prompts: CreativeCharacterPrompt[];
  reference_ids: string[];
  reference_mode?: CreativeReferenceMode;
  used_reference_ids?: string[];
  requirement_checks: { requirement: string; evidence: string }[];
  warnings: string[];
  model: string;
  size: CreativeSize;
  steps: number;
  parameters: {
    scale: number;
    cfg: number;
    sampler: string;
    noise_schedule: string;
    other: string;
  };
  free_only: boolean;
  llm_raw: string;
}
export interface CreativeEditRequest {
  plan_id: string;
  prompt: string;
  negative: string;
  character_prompts?: CreativeCharacterPrompt[];
}
export interface CreativeStartRequest {
  plan_id: string;
  request_id: string;
  count: number;
  seed: number | null;
  score: boolean;
}
export type CreativeRunStatus =
  | "queued"
  | "running"
  | "done"
  | "error"
  | "cancelled";
export interface CreativeImage {
  idx: number;
  seed: number;
  status: "pending" | "running" | "done" | "error" | "cancelled";
  file: string | null;
  error: string | null;
  score: number | null;
  notes: string[];
  account_index: number | null;
  anlas_before: number | null;
  anlas_after: number | null;
  anlas_delta: number | null;
  stamina_before: number | null;
  stamina_after: number | null;
}
export interface CreativeRun {
  run_id: string;
  plan_id: string;
  plan: CreativePlan;
  request_id: string;
  status: CreativeRunStatus;
  created_at: string;
  finished_at: string | null;
  images: CreativeImage[];
  error: string | null;
  free_only: boolean;
  anlas_spent: number | null;
  warnings: string[];
  cancel_requested: boolean;
  stamina_before: number | null;
  stamina_after: number | null;
}
