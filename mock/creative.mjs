// Synthetic creative protocol. No source-library data, credentials, or external calls.
const clone = (x) => JSON.parse(JSON.stringify(x));
const sizes = { portrait: [832, 1216], landscape: [1216, 832], square: [1024, 1024] };
const plans = new Map(), snapshots = new Map(), runs = new Map(), requests = new Map();
let sequence = 0;
export const creativeMetrics = { llm_calls: 0, image_calls: 0, character_selections: 0 };
const characters = (scenario) => scenario === "creative-empty-characters" ? [] : ["薇拉", "桃乃"];
export const references = Array.from({ length: 18 }, (_, i) => ({
  entry_id: `creative-fixture-${i}`, title: `${i + 1}. ${["前景与纵深", "动态视线", "室内光影"][i % 3]} · 合成参考`,
  path: ["合成精选", i % 2 ? "动态构图" : "空间层次"],
  tags: `original synthetic example ${i}, library interior, window lighting, layered foreground, diagonal composition`,
  negative: "flat composition",
  character_prompts: [{ label: "主体", prompt: "original character, reaching toward a bookshelf, expressive gesture", negative: "rigid pose" }],
  content_hash: String(i).padStart(64, "0"), decision: "include", rating: "safe", prose: i % 5 === 0,
}));
function fail(code, message) { throw { code, message }; }
const created = () => new Date().toISOString();
function eligible(req, scenario = "") {
  const scope = Array.isArray(req.scope) ? req.scope : [];
  const pool = scenario === "creative-over-former-budget"
    ? Array.from({ length: 188 }, (_, i) => {
      const r = references[i % references.length];
      return { ...r, entry_id: `creative-fixture-${i}`, title: `${i + 1}. 合成完整参考`,
        tags: `${r.tags.repeat(240)}, full-material-end-${i}`, content_hash: String(i).padStart(64, "0") };
    })
    : scenario === "creative-large-references"
      ? references.map((r) => ({ ...r, tags: r.tags.repeat(80) })) : references;
  return pool.filter((r) => (!scope.length || scope.some((s) => s.every((v, i) => r.path[i] === v))) &&
    (!req.search || `${r.title} ${r.tags} ${r.path.join("/")}`.includes(req.search)));
}
function plan(req, scenario) {
  const dimensions = sizes[req.orientation];
  if (!dimensions) fail("invalid_request", "只允许三个免费尺寸");
  const mode = req.reference_mode || "sample";
  if (!["sample", "all"].includes(mode)) fail("invalid_request", "参考模式无效");
  if (mode === "all" && scenario === "creative-legacy") fail("creative_full_reference_unsupported", "当前插件不支持全量参考，请更新并重载插件");
  const available = eligible({ ...req, search: "" }, scenario);
  const priority = Array.isArray(req.reference_ids) ? req.reference_ids : [];
  if (mode === "sample" && priority.some((id) => !available.some((r) => r.entry_id === id))) fail("invalid_request", "重点参考不在当前范围");
  if (!available.length) fail("empty_references", "当前范围没有已加入的 safe 图包");
  if (!req.provider_id) fail("invalid_request", "请选择提示词模型");
  let character = req.character;
  if (!character.trim()) {
    const pool = [...new Set(characters(scenario).map((name) => name.trim()))]
      .filter((name) => name && Buffer.byteLength(name, "utf8") <= 1000);
    if (!pool.length) fail("creative_character_pool_empty", "插件角色池为空，无法随机选角；请检查插件角色池或指定角色。");
    character = pool[Math.floor(Math.random() * pool.length)];
    creativeMetrics.character_selections++;
  }
  const ordered = [...available.filter((r) => priority.includes(r.entry_id)), ...available.filter((r) => !priority.includes(r.entry_id))];
  const picked = mode === "all" ? available : ordered.slice(0, Math.max(req.reference_count, priority.length));
  if (mode === "sample" && Buffer.byteLength(JSON.stringify(picked), "utf8") > 96000)
    fail("invalid_request", "参考原文超过96KB，不会截断");
  creativeMetrics.llm_calls++;
  const requirement = req.requirements || "在参考范围内自由构思";
  const person = `${character}, dancing, flowing skirt, `;
  const artist = req.artist_ratio > 0 ? "0.7::artist:synthetic_example::, colorful, thick outlines" : "";
  const tag = `${person}beach, dancing, wet sand, ocean foam framing the foreground, sunset rim light, wide shot, dynamic composition`;
  const next = {
    plan_id: `creative-plan-${++sequence}`, parent_plan_id: null, created_at: created(),
    character, requirements: req.requirements, provider_id: req.provider_id,
    skill_version: "builtin-1",
    concept: `${character}在湿沙上起舞，海浪在前景形成弧线，夕阳从侧后方勾勒动作。要求：${requirement}`,
    prompt: [artist, tag].filter(Boolean).join(", "), negative: "low quality, blurry, static pose",
    artist, artist_source: artist ? "pool" : "none", character_prompts: [],
    reference_ids: picked.map((r) => r.entry_id),
    ...(scenario === "creative-legacy" ? {} : { reference_mode: mode, used_reference_ids: picked.slice(0, 2).map((r) => r.entry_id) }),
    requirement_checks: req.requirements ? [{ requirement: req.requirements, evidence: "主提示词 beach, dancing；构图说明中的湿沙与舞蹈动作（模型检查，非成图保证）" }] : [],
    warnings: ["合成离线演示；这些结果未调用真实模型。"],
    model: "nai-diffusion-5-full", size: dimensions.join("x"), steps: 28,
    parameters: { scale: 5, cfg: 0, sampler: "k_euler_ancestral", noise_schedule: "karras", other: "0" },
    free_only: true, llm_raw: '{"synthetic":true,"novel_composition":true}',
  };
  if (scenario === "creative-long") next.concept += "构图说明与必须满足的内容。".repeat(300);
  plans.set(next.plan_id, clone(next));
  snapshots.set(next.plan_id, clone(picked));
  return next;
}
function advance(run) {
  if (run.status !== "running" && run.status !== "queued") return run;
  const elapsed = Date.now() - run._started;
  const completed = Math.min(run.images.length, Math.floor(elapsed / 650));
  for (let i = 0; i < completed; i++) {
    const img = run.images[i];
    if (img.status !== "done") creativeMetrics.image_calls++;
    img.status = "done"; img.file = `${run.run_id}-${i}.svg`; img.score = run._score && i % 2 === 0 ? 7.8 : null;
    img.notes = img.score === null ? ["未评分"] : ["合成评分，不是真实模型输出"];
  }
  if (completed === run.images.length) {
    run.status = "done"; run.finished_at = created(); run.anlas_spent = 0;
    run.stamina_after = 98 - completed * 0.02;
  } else { run.status = "running"; run.images[completed].status = "running"; }
  return run;
}
function wire(run) { const value = clone(run); delete value._started; delete value._score; return value; }
export async function answerCreative(command, args = {}, scenario = "") {
  if (scenario === "401") fail("plugin_rejected", "HTTP 401：模拟插件认证失败");
  if (scenario === "creative-unavailable" && command !== "creative_references" && command !== "creative_plans_list")
    fail("creative_unavailable", "当前插件尚未提供创作接口，请更新并重载插件，再刷新连接状态");
  if (scenario === "creative-loading") await new Promise((r) => setTimeout(r, 450));
  const req = args.req || {};
  switch (command) {
    case "creative_capabilities":
      return { version: 1, free_only: true,
        ...(scenario === "creative-legacy" ? {} : { full_reference_supported: true }),
        free_sizes: Object.entries(sizes).map(([id, [width, height]]) => ({ id, width, height })),
        providers: ["synthetic/prompt-model", "synthetic/second-model"], characters: characters(scenario),
        model: "nai-diffusion-5-full", steps: 28, available: true, reason: null,
        render_available: true, render_reason: null, min_stamina_percent: 5,
        free_limitations: ["逐张读取订阅与点数；同一出图通道上其他客户端的并发不受本应用控制。"] };
    case "creative_references": {
      if (scenario === "creative-malformed") return { items: {}, total: 12345 };
      if (scenario === "empty") return { items: [], total: 0 };
      const items = eligible(req, scenario);
      return clone({ items: items.slice(req.offset || 0, (req.offset || 0) + (req.limit || 100)), total: items.length });
    }
    case "creative_plan":
      if (scenario === "creative-invalid-plan") return {};
      return plan(req, scenario);
    case "creative_edit": {
      const prev = plans.get(req.plan_id);
      if (!prev) fail("not_found", "方案不存在");
      if (!req.prompt.trim()) fail("invalid_request", "提示词不能为空");
      if (prev.requirements && !/beach|沙滩/.test(req.prompt)) fail("requirement_failed", "编辑后缺少沙滩要求，请补全后再冻结");
      creativeMetrics.llm_calls++;
      const next = { ...clone(prev), plan_id: `creative-plan-${++sequence}`, parent_plan_id: prev.plan_id,
        created_at: created(), prompt: req.prompt, negative: req.negative,
        character_prompts: req.character_prompts || prev.character_prompts,
        ...(prev.reference_mode === undefined ? {} : { used_reference_ids: prev.reference_ids.slice(-1) }) };
      plans.set(next.plan_id, clone(next)); snapshots.set(next.plan_id, clone(snapshots.get(prev.plan_id))); return next;
    }
    case "creative_plans_list": return clone([...plans.values()].reverse().slice(0, args.limit || 30));
    case "creative_plan_references": return clone(snapshots.get(args.plan_id) || []);
    case "creative_start": {
      if (requests.has(req.request_id)) {
        const prior = requests.get(req.request_id);
        if (prior.body !== JSON.stringify(req)) fail("idempotency_conflict", "同一请求 ID 不能更换参数");
        return wire(advance(runs.get(prior.id)));
      }
      if (scenario === "creative-low-stamina") fail("free_only_unavailable", "免费点数低于 5% 保护余量；已停止，不转用 Anlas");
      const p = plans.get(req.plan_id);
      if (!p) fail("not_found", "方案不存在");
      if (!Number.isInteger(req.count) || req.count < 1 || req.count > 8) fail("invalid_request", "张数必须为 1–8");
      const run = { run_id: `creative-run-${++sequence}`, plan_id: p.plan_id, plan: clone(p), request_id: req.request_id,
        status: "queued", created_at: created(), finished_at: null,
        images: Array.from({ length: req.count }, (_, idx) => ({ idx, seed: req.seed === null ? 100000 + sequence * 31 + idx : req.seed + idx,
          status: "pending", file: null, error: null, score: null, notes: [], account_index: null,
          anlas_before: null, anlas_after: null, anlas_delta: null, stamina_before: null, stamina_after: null })),
        error: null, free_only: true, anlas_spent: null, stamina_before: 98, stamina_after: null,
        warnings: [], cancel_requested: false,
        _started: Date.now(), _score: req.score };
      runs.set(run.run_id, run); requests.set(req.request_id, { id: run.run_id, body: JSON.stringify(req) });
      return wire(run);
    }
    case "creative_status": {
      const run = runs.get(args.run_id); if (!run) fail("not_found", "任务不存在"); return wire(advance(run));
    }
    case "creative_list": return [...runs.values()].reverse().slice(0, args.limit || 30).map((r) => wire(advance(r)));
    case "creative_cancel": {
      const run = runs.get(args.run_id); if (!run) fail("not_found", "任务不存在");
      advance(run);
      if (["queued", "running"].includes(run.status)) {
        run.status = "cancelled"; run.finished_at = created(); run.cancel_requested = true;
        run.images.forEach((i) => { if (i.status !== "done") i.status = "cancelled"; });
        run.anlas_spent = run.images.some((i) => i.status === "done") ? 0 : null;
      }
      return wire(run);
    }
    case "creative_image": {
      const run = runs.get(args.run_id), item = run?.images[args.idx];
      if (!item || item.status !== "done") fail("not_found", "图片尚未完成");
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="416" height="608"><rect width="100%" height="100%" fill="#e7edf3"/><path d="M0 360 Q160 280 416 380V608H0Z" fill="#93b7c2"/><text x="32" y="280" font-size="22" fill="#29434d">Synthetic image · ${item.seed}</text></svg>`;
      return `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;
    }
    default: fail("not_found", `Mock 未登记创作命令 ${command}`);
  }
}
