import { invoke } from "./ipc";
import type * as T from "./types-creative";

export const creativeApi = {
  capabilities: () => invoke<T.CreativeCapabilities>("creative_capabilities"),
  references: (req: T.CreativeReferenceQuery) =>
    invoke<T.CreativeReferencePage>("creative_references", { req }),
  plan: (req: T.CreativePlanRequest) =>
    invoke<T.CreativePlan>("creative_plan", { req }),
  edit: (req: T.CreativeEditRequest) =>
    invoke<T.CreativePlan>("creative_edit", { req }),
  plans: (limit = 30) =>
    invoke<T.CreativePlan[]>("creative_plans_list", { limit }),
  planReferences: (plan_id: string) =>
    invoke<T.CreativeReference[]>("creative_plan_references", { plan_id }),
  start: (req: T.CreativeStartRequest) =>
    invoke<T.CreativeRun>("creative_start", { req }),
  status: (run_id: string) =>
    invoke<T.CreativeRun>("creative_status", { run_id }),
  list: (limit = 30) => invoke<T.CreativeRun[]>("creative_list", { limit }),
  cancel: (run_id: string) =>
    invoke<T.CreativeRun>("creative_cancel", { run_id }),
  image: (run_id: string, idx: number, full = false) =>
    invoke<string>("creative_image", { run_id, idx, full }),
};
