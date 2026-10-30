// Synthetic offline data only. No bot, user library or paid services.
import { resetStudioPluginSkills, syncStudioPluginSkills } from "./plugin/studio-skills.mjs";
const image = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="832" height="1216"><rect width="832" height="1216" fill="#e8dcf4"/><circle cx="416" cy="385" r="150" fill="#b68cc9"/><path d="M150 1100 416 540 682 1100" fill="#744486"/><text x="416" y="1170" text-anchor="middle" font-size="32">SYNTHETIC STUDIO RESULT</text></svg>')}`;
const data = { workspace: null, plans: new Map(), runs: new Map(), inputs: new Map(), pluginSkills: new Map(), counter: 0 };
export const studioMetrics = { prepare: 0, plans: 0, start: 0, generations: 0, cancel: 0, compare: 0, local: 0, skillSync: 0 };
const skills = [{ id: "builtin-guide", name: "NAI5 提示词写法（合成）", version: "builtin-1", sha256: "a".repeat(64), text: "Synthetic fixed skill material. Compose a new scene; preserve user requirements.", source: "synthetic://builtin-guide", builtin: true, bytes: 88 }];
export const providers = ["示例/视觉模型-A", "示例/视觉模型-B", "示例/超长模型名称-用于检查多模型对照配置的横向溢出与键盘可达性"];
const capabilities = { version: 1, reference_images_supported: true, reference_text_supported: true, providers, characters: ["薇拉", "阿莉亚", "露米"], free_sizes: [{ id: "portrait", width: 832, height: 1216 }, { id: "landscape", width: 1216, height: 832 }, { id: "square", width: 1024, height: 1024 }], model: "nai-diffusion-5-full", steps: 28, available: true, free_only: true, reason: null,
  defaults: { draw_provider_id: providers[0], reverse_provider_id: providers[0], score_provider_ids: [providers[1]], style_review_provider_id: providers[1] },
  templates: { reverse: "Synthetic ordinary reverse template", reverse_separate: "Synthetic separate multi-character template", draw_brief: "设计新场景，完整满足用户要求。", multi_brief: "保持每个角色的身份与相对位置。" } };
function next(prefix) { return `${prefix}-${String(++data.counter).padStart(6, "0")}`; }
function clone(v) { return structuredClone(v); }
function plan(req) {
  const size = req.orientation === "landscape" ? "1216x832" : req.orientation === "square" ? "1024x1024" : "832x1216";
  const plans = Array.from({ length: req.mode === "reverse" ? 1 : req.count }, (_, i) => ({ plan_id: next("concept"), parent_plan_id: null, created_at: new Date().toISOString(), character: req.character, characters: req.characters,
    requirements: req.requirements, provider_id: req.provider_id, skill_version: req.skill_ids.join("+"), concept: `构思 ${i + 1}：${["海边舞台，夕阳逆光", "浅滩漫步，俯视远景", "礁石花园，旋转舞姿"][i % 3]}`,
    prompt: `1girl, ${req.character}, beach, dancing, scene_${i}, ${"synthetic detailed composition, ".repeat(35)}`, negative: "low quality", artist: "artist:synthetic", artist_source: "pool", character_prompts: [{ prompt: req.character, negative_prompt: "", position: "center" }], reference_ids: req.reference_ids ?? [], reference_mode: req.reference_mode, used_reference_ids: (req.reference_ids ?? []).slice(0, 2), requirement_checks: req.requirements ? [{ requirement: req.requirements, evidence: "此处为合成模型要求自检，成图仍需人工确认" }] : [], warnings: [], model: capabilities.model, size, steps: 28,
    parameters: { scale: 5, cfg: 0, sampler: "k_euler_ancestral", noise_schedule: "native", other: "" }, free_only: true, llm_raw: "Synthetic model output only." }));
  return { plan_id: next("studio-plan"), created_at: new Date().toISOString(), mode: req.mode, plans, settings: { ...req, skill_bundles: req.skill_ids.map((id) => {
    const skill = data.pluginSkills.get(id) ?? skills.find((item) => item.id === id);
    if (!skill) throw { code: "not_found", message: `合成资料快照不存在：${id}` };
    return { id, version: skill.version, sha256: skill.sha256 };
  }) }, warnings: [] };
}
function resultImage(idx, seed) { return { idx, plan_id: "fixture-plan", seed: seed + idx, status: "pending", file: null, error: null, score: null, notes: [], account_index: null, anlas_before: null, anlas_after: null, anlas_delta: null, stamina_before: null, stamina_after: null }; }
function publicRun(run) { const { polls, ...value } = run; return clone(value); }
const pairs = (images, map) => images.flatMap((left, i) => images.slice(i + 1).map((right, at) => map(left, right, i + at)));
export function resetStudio() { data.workspace = null; for (const map of [data.plans, data.runs, data.inputs, data.pluginSkills]) map.clear(); resetStudioPluginSkills(); for (const key of Object.keys(studioMetrics)) studioMetrics[key] = 0; }
export async function answerStudio(command, args = {}, scenario = "") {
  const req = args.req;
  if (scenario === "studio-401") throw { code: "plugin_rejected", message: "HTTP 401：模拟插件认证失败" };
  switch (command) {
    case "studio_capabilities": {
      if (scenario === "studio-malformed") return { providers: {} };
      const value = clone(capabilities);
      if (scenario === "studio-no-reference-images") delete value.reference_images_supported;
      if (scenario === "studio-no-reference-text" || scenario === "studio-legacy-reference-images") delete value.reference_text_supported;
      return value;
    }
    case "studio_workspace_get": if (scenario === "studio-workspace-error") throw { code: "io", message: "模拟工作区读取失败" }; return clone(data.workspace);
    case "studio_workspace_save": data.workspace = clone(args.state); return null;
    case "studio_skills_list": return clone(skills);
    case "studio_skills_sync": {
      studioMetrics.skillSync++;
      const value = syncStudioPluginSkills(args.kind, scenario);
      if (value.skill) data.pluginSkills.set(value.skill.id, clone(value.skill));
      return clone(value);
    }
    case "studio_skills_fetch":
    case "studio_skills_import": { const value = { ...skills[0], id: next("skill"), name: command === "studio_skills_fetch" ? "NPE 固定版（合成）" : "导入资料（合成）", builtin: false }; skills.push(value); return clone(value); }
    case "studio_skills_delete": { const index = skills.findIndex((s) => s.id === args.id && !s.builtin); if (index >= 0) skills.splice(index, 1); return null; }
    case "studio_references": {
      if (scenario === "studio-reference-error") throw { code: "db", message: "合成资料读取失败，不应显示为空池" };
      const total = scenario === "studio-reference-images-large" ? 1367 : req.decisions === "all" ? 12513 : 188;
      return { total, items: Array.from({ length: Math.min(req.limit ?? 60, total) }, (_, i) => ({ entry_id: `reference-${i}`, title: `构图参考 ${String(i + 1).padStart(3, "0")}`, path: ["社区整理", "常规"], tags: `Synthetic reference ${i}: light, composition, background`, negative: "", character_prompts: [], content_hash: "b".repeat(64), decision: req.decisions === "all" ? ["include", "exclude", "undecided"][i % 3] : "include", rating: "safe", prose: false })) };
    }
    case "studio_prepare": {
      studioMetrics.prepare++;
      const content = req.reference_content ?? "image_text";
      if (!["image_text", "text"].includes(content)) throw { code: "bad_request", message: "未知参考方式" };
      if (req.mode !== "reverse" && content === "image_text" && scenario === "studio-reference-image-missing") throw { code: "bad_request", message: "参考图包 reference-42 缺少主参考图，请在图库下载展示图后重试；未减少图包或退回纯文字。" };
      const id = next("input"), characters = req.characters.length > 1 ? req.characters : [], character = characters.length ? "" : (req.character || capabilities.characters[0]);
      const reference_ids = Array.from({ length: req.mode === "reverse" ? 0 : req.reference_mode === "all" ? (scenario === "studio-reference-images-large" ? 1367 : req.reference_decisions === "all" ? 12513 : 188) : req.reference_count }, (_, i) => `reference-${i}`);
      const reference_images = content === "text" ? [] : reference_ids.map((entry_id, i) => ({ entry_id, ordinal: 0, mime: "image/png", sha256: (i + 1).toString(16).padStart(64, "0"), raw_tag: `Synthetic image ${i}: composition, light` }));
      data.inputs.set(id, { ...clone(req), character, characters, reference_ids, ...(req.mode === "reverse" ? {} : { reference_content: content, reference_images, reference_image_count: reference_images.length }) });
      return { input_id: id, character, characters, reference_count: reference_ids.length, reference_ids,
        ...(req.mode === "reverse" ? {} : { reference_content: content }),
        reference_image_count: scenario === "studio-reference-image-count-mismatch" ? Math.max(0, reference_ids.length - 1) : scenario === "studio-reference-text-count-mismatch" ? 1 : reference_images.length };
    }
    case "studio_plan": {
      studioMetrics.plans++;
      if (scenario === "studio-reference-image-limit" && req.reference_content !== "text") throw { code: "plugin_rejected", message: "模型服务拒绝本次图片数量或请求容量；未移除参考图片或改成纯文字重试。" };
      if (scenario === "studio-plan-error") throw { code: "model", message: "模拟构思失败，未产生图像" };
      const input = data.inputs.get(req.input_id); if (!input) throw { code: "not_found", message: "冻结输入不存在" };
      if (req.mode !== "reverse" && (req.reference_content ?? "image_text") !== input.reference_content) throw { code: "bad_request", message: "参考方式与冻结输入不符" };
      const value = plan({ ...input, ...req, reference_ids: input.reference_ids });
      if (scenario === "studio-legacy-reference-images") delete value.settings.reference_content;
      if (scenario === "studio-reference-text-plan-mismatch") { value.settings.reference_content = "image_text"; value.settings.reference_image_count = 1; }
      data.plans.set(value.plan_id, value); return clone(value);
    }
    case "studio_plan_get": {
      const saved = data.plans.get(args.plan_id); if (!saved) throw { code: "not_found", message: "方案不存在" };
      const value = clone(saved);
      if (scenario === "studio-legacy-text-history") for (const key of ["reference_content", "reference_images", "reference_image_count"]) delete value.settings[key];
      return value;
    }
    case "studio_start": {
      studioMetrics.start++; const previous = [...data.runs.values()].find((r) => r.request_id === req.request_id); if (previous) return publicRun(previous);
      const group = data.plans.get(req.plan_id); if (!group) throw { code: "not_found", message: "方案不存在" };
      studioMetrics.generations++;
      const run = { run_id: next("studio-run"), plan_id: group.plan_id, request_id: req.request_id, mode: group.mode, plan: clone(group), status: "running", created_at: new Date().toISOString(), finished_at: null, images: Array.from({ length: group.settings.count }, (_, i) => resultImage(i, req.seed ?? 12345)), error: null, free_only: true, anlas_spent: null, warnings: [], cancel_requested: false, task_id: null, polls: 0 };
      data.runs.set(run.run_id, run);
      if (scenario === "studio-start-lost") throw { code: "network", message: "模拟：服务端已接收但响应丢失，请重试原请求" };
      return publicRun(run);
    }
    case "studio_status": {
      const run = data.runs.get(args.run_id); if (!run) throw { code: "not_found", message: "运行不存在" };
      if (scenario === "studio-status-error") throw { code: "network", message: "模拟状态读取失败" };
      if (run.status === "running") {
        run.polls++; const index = Math.min(run.polls - 1, run.images.length - 1), item = run.images[index]; item.status = "done"; item.file = `/synthetic/${run.run_id}/${index}.png`; item.score = index ? null : 7.5; item.anlas_delta = 0; item.stamina_before = 90; item.stamina_after = 88;
        if (run.polls >= run.images.length) { run.status = "done"; run.finished_at = new Date().toISOString(); run.anlas_spent = 0; }
      }
      return publicRun(run);
    }
    case "studio_cancel": { studioMetrics.cancel++; const run = data.runs.get(args.run_id); if (!run) throw { code: "not_found", message: "运行不存在" }; run.cancel_requested = true; run.status = "cancelled"; run.finished_at = new Date().toISOString(); for (const i of run.images) if (i.status !== "done") i.status = "cancelled"; return publicRun(run); }
    case "studio_list": if (scenario === "studio-history-error") throw { code: "network", message: "模拟历史读取失败" }; return [...data.runs.values()].reverse().map(publicRun);
    case "studio_image": {
      const run = data.runs.get(args.run_id); if (!run?.images[args.idx] || run.images[args.idx].status !== "done" || scenario === "studio-image-pending") throw { code: "studio_image_pending", message: "图片尚未准备好" };
      return { path: image, original_path: image, format: args.format, original_bytes: 1500000, encoded_bytes: args.format === "jpeg" ? 334455 : 1500000, quality: args.format === "jpeg" ? args.quality : null };
    }
    case "studio_compare": studioMetrics.compare++; return { provider_id: req.provider_id, text: "合成视觉模型：以图 1 为基准完成候选排序。", latency_ms: 1234, reference_id: req.images[0].id, ranking: req.images.slice(1, 4).map((item, i) => ({ rank: i + 1, image_id: item.id, similarity: 92 - i * 8, reason: `第 ${i + 1} 名的线条、上色与光影接近基准图` })), pairs: [] };
    case "style_compare": studioMetrics.local++; return { images: req.images.map((i) => ({ ...i, width: 832, height: 1216, sha256: "c".repeat(64) })), results: req.models.map((model, i) => ({ model, label: model, model_version: "synthetic-fixed-revision", preprocessing: model.startsWith("mega") ? "RGB 全图 384" : "中心裁剪 224", device: "cpu", fallback_reason: null, load_ms: 12, encode_ms: 150, total_ms: 162, pairs: pairs(req.images, (a, b, at) => ({ left_id: a.id, right_id: b.id, cosine: at % 2 === 0 ? .88 - i * .04 : .32 })), error: null })), elapsed_ms: 486, centered: false };
    case "style_compare_cancel": return null;
    default: throw { code: "not_found", message: `未登记实验接口 ${command}` };
  }
}
