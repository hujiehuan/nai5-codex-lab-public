import { invoke, snapshot } from "./ipc";
import type * as T from "./types-studio";

// Register saves when requested, including queued writes from an unmounted lab.
// Installation must see a failed save, rather than restarting with stale state.
let workspaceSave: Promise<void> = Promise.resolve();
function saveWorkspace(state: unknown): Promise<void> {
  const frozen = snapshot(state);
  workspaceSave = workspaceSave.catch(() => {}).then(() => invoke<void>("studio_workspace_save", { state: frozen }));
  return workspaceSave;
}
export async function flushStudioWorkspace(): Promise<void> {
  let pending: Promise<void>;
  do { pending = workspaceSave; await pending; } while (pending !== workspaceSave);
}

export const studioApi = {
  capabilities: () => invoke<T.StudioCapabilities>("studio_capabilities"),
  prepare: (req: Omit<T.StudioPlanRequest, "provider_id" | "skill_ids">) => invoke<T.StudioPreparedInput>("studio_prepare", { req }),
  references: (req: T.StudioReferenceQuery) => invoke<T.StudioReferencePage>("studio_references", { req }),
  plan: (req: T.StudioPlanRequest) => invoke<T.StudioPlan>("studio_plan", { req }),
  getPlan: (plan_id: string) => invoke<T.StudioPlan>("studio_plan_get", { plan_id }),
  start: (req: { plan_id: string; request_id: string; seed: number | null }) => invoke<T.StudioRun>("studio_start", { req }),
  status: (run_id: string) => invoke<T.StudioRun>("studio_status", { run_id }),
  list: (limit = 50) => invoke<T.StudioRun[]>("studio_list", { limit }),
  cancel: (run_id: string) => invoke<T.StudioRun>("studio_cancel", { run_id }),
  image: (run_id: string, idx: number, format: "png" | "jpeg", quality = 88) =>
    invoke<T.StudioImageResult>("studio_image", { run_id, idx, format, quality }),
  skills: () => invoke<T.StudioSkill[]>("studio_skills_list"),
  syncSkills: (kind: T.StudioSkillKind) => invoke<T.StudioPluginSkill>("studio_skills_sync", { kind }),
  importSkill: (req: { path: string; name?: string }) => invoke<T.StudioSkill>("studio_skills_import", { req }),
  fetchSkill: (id: "npe") => invoke<T.StudioSkill>("studio_skills_fetch", { id }),
  deleteSkill: (id: string) => invoke<void>("studio_skills_delete", { id }),
  workspace: () => invoke<unknown>("studio_workspace_get"),
  saveWorkspace,
  compare: (req: { provider_id: string; prompt: string; images: T.StudioCompareImage[] }) =>
    invoke<T.StudioCompareResult>("studio_compare", { req }),
  compareLocal: (req: { images: T.StudioCompareImage[]; models: string[] }) =>
    invoke<T.StyleCompareResult>("style_compare", { req }),
  cancelLocalCompare: () => invoke<void>("style_compare_cancel"),
};
