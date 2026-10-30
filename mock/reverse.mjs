// Synthetic reverse-evaluation fixtures for the /eval page (Vite mode=mock only).
// Three OpenAI-compatible direct endpoints (vision-model-a/b/c) and three finished batches.
// Every number below comes from a fixed seed; none of it is a measurement of a real model.
import * as desktop from "./desktop.mjs";
import { jpegFor } from "./plugin/images.mjs";

const clone = (v) => structuredClone(v);
const jpeg = (key) => `data:image/jpeg;base64,${jpegFor(key)}`;
const round = (v, digits = 4) => Number(v.toFixed(digits));
const mean = (v) => (v.length ? v.reduce((a, b) => a + b, 0) / v.length : null);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
function hash(text) {
  let h = 2166136261;
  for (const ch of text) h = Math.imul(h ^ ch.codePointAt(0), 16777619);
  return h >>> 0;
}
/** mulberry32 seeded by a string: the same key always yields the same sequence. */
function rng(key) {
  let a = hash(key);
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
function pick(pool, n, r) {
  const rest = [...pool], out = [];
  while (out.length < n && rest.length) out.push(rest.splice(Math.floor(r() * rest.length), 1)[0]);
  return out;
}

// Vocabulary: generic scene tags only.
const TRUTH_POOL = [
  "1girl", "solo", "smile", "open mouth", "looking at viewer", "upper body", "full body", "cowboy shot",
  "outdoors", "sky", "cloud", "sunset", "night", "rain", "umbrella", "school uniform", "hoodie", "scarf",
  "round glasses", "holding book", "bookshelf", "window", "curtains", "flower field", "cherry blossoms",
  "street", "city lights", "river", "bridge", "cat", "bicycle", "from side", "from above", "wind",
  "light particles", "depth of field", "backlighting", "soft lighting", "sitting", "standing", "walking",
  "hand up", "table", "cup", "library", "train interior",
];
const DISTRACTORS = [
  "blue sky", "day", "grass", "tree", "white shirt", "skirt", "building", "lamp", "bag", "ribbon",
  "jacket", "necktie", "ocean", "snow", "leaf", "chair", "wooden floor", "reflection", "shadow", "petals",
];
const APPEARANCE = ["brown hair", "black hair", "long hair", "short hair", "blue eyes", "green eyes", "red eyes"];
const QUALITY_NOTES = ["构图完整", "主体清晰", "光影自然", "背景略空", "手部细节一般", "色彩统一", "线条干净"];

/** Per-model behaviour of the synthetic generator (target precision / recall, latency range, failure rates). */
const MODELS = {
  "vision-model-a": { precision: 0.8, recall: 0.68, latency: [2600, 4300], refusal: 0, error: 0, leak: 0.1, banned: 0, contradiction: 0, quality: 7.6, similarity: 7.0 },
  "vision-model-b": { precision: 0.72, recall: 0.77, latency: [1400, 2500], refusal: 0, error: 0.07, leak: 0.25, banned: 0.05, contradiction: 0.05, quality: 7.2, similarity: 6.7 },
  "vision-model-c": { precision: 0.62, recall: 0.56, latency: [700, 1400], refusal: 0.09, error: 0, leak: 0.45, banned: 0.15, contradiction: 0.15, quality: 6.4, similarity: 5.9 },
};
const SEED_CREATED = "2026-10-18T09:00:00+09:00";
const TEMPLATE_V1 = 'Describe the image as comma-separated tags. Reply with JSON only: {"suitable": true, "tags": "tag, tag, ..."}';
const TEMPLATE_V2 = 'Describe the image as comma-separated tags ordered by visual importance. Reply with JSON only: {"suitable": true, "framing": "upper body", "tags": "tag, tag, ..."}';

function initialProviders() {
  const provider = (provider_id, model, base_url, has_key) => ({
    provider_id, name: model, base_url, model, max_tokens: 1024, enabled: true, has_key,
    created_at: SEED_CREATED, kind: "openai", plugin_provider_id: null,
  });
  return [
    provider(1, "vision-model-a", "https://gateway.example.com/v1", true),
    provider(2, "vision-model-b", "https://gateway.example.com/v1", true),
    provider(3, "vision-model-c", "http://127.0.0.1:8000/v1", true),
  ];
}
function initialTemplates() {
  return [
    { template_id: 1, name: "tags-json", version: 1, body: TEMPLATE_V1, parent_id: null, created_at: SEED_CREATED, note: "基线：只要求 JSON 标签" },
    { template_id: 2, name: "tags-json", version: 2, body: TEMPLATE_V2, parent_id: 1, created_at: "2026-10-23T18:20:00+09:00", note: "按画面重要性排序，并给出 framing" },
  ];
}
function initialRulesets() {
  return [{
    ruleset_id: 1, name: "default", version: 1, banned: ["watermark", "signature", "text"],
    appearance_regex: "\\b(hair|eyes)\\b", accessory_ok_regex: "\\b(hairclip|hair ribbon|glasses)\\b",
    refusal_regex: "(cannot|unable to) (help|describe)", always_drop: ["masterpiece", "best quality"],
    max_words: 6, max_len: 48, created_at: SEED_CREATED, note: "合成规则集：外貌词只作预期遗漏，不计入 P/R/F1",
  }];
}
const FIXED_QUESTIONS = Array.from({ length: 12 }, (_, i) => `fixture-nai5_community_pack-${i}#0`);
/** Batches 2 and 3 redraw with the same seeds and count; only the style-recipe source differs (pool vs retrieval). */
function initialBatches() {
  return [
    { batch_id: "rb-20261020-base", created_at: "2026-10-20T09:30:12+09:00", finished_at: "2026-10-20T09:41:55+09:00", status: "done",
      template_id: 1, ruleset_id: 1, provider_ids: [1, 2, 3], question_ids: FIXED_QUESTIONS, note: "基线", render: null },
    { batch_id: "rb-20261024-pool", created_at: "2026-10-24T14:15:40+09:00", finished_at: "2026-10-24T14:52:03+09:00", status: "done",
      template_id: 2, ruleset_id: 1, provider_ids: [1, 2, 3], question_ids: FIXED_QUESTIONS, note: "模板 v2 · 重画（风格池随机）",
      render: { mode: "pool", count: 2, seed: 410000, artist: null } },
    { batch_id: "rb-20261028-style", created_at: "2026-10-28T20:10:05+09:00", finished_at: "2026-10-28T20:47:31+09:00", status: "done",
      template_id: 2, ruleset_id: 1, provider_ids: [1, 2, 3], question_ids: FIXED_QUESTIONS, note: "模板 v2 · 重画（检索风格配方）",
      render: { mode: "style", count: 2, seed: 410000, artist: null } },
  ];
}

const state = { providers: [], templates: [], rulesets: [], batches: [], runs: new Map(), details: new Map(), judgments: new Map(), nextRun: 1, nextId: 100, sequence: 0 };
export function resetReverse() {
  Object.assign(state, { providers: initialProviders(), templates: initialTemplates(), rulesets: initialRulesets(), batches: initialBatches(),
    runs: new Map(), details: new Map(), judgments: new Map(), nextRun: 1, nextId: 100, sequence: 0 });
  for (const batch of state.batches) generate(batch);
  state.judgments.set(firstRun("rb-20261024-pool"), { score: 4, comment: "标签齐全，背景描述略少", at: "2026-10-24T15:03:10+09:00" });
}
function firstRun(batch_id) { return state.runs.get(batch_id)[0].run_id; }

// Questions
function question(question_id) {
  if (question_id.startsWith("local:")) {
    const path = question_id.slice("local:".length);
    return { question_id: null, entry_id: null, index: hash(path) % 1000, local_path: path, thumb_path: path, truth_kind: "none", truth_eval: [], expected_miss: [] };
  }
  const [entry_id, ordinal = "0"] = question_id.split("#");
  const index = Number(entry_id.match(/-(\d+)$/)?.[1] ?? hash(entry_id) % 1000);
  const r = rng(`truth|${entry_id}|${ordinal}`);
  const prose = index % 6 === 5;
  return { question_id, entry_id, index, thumb_path: jpeg(index), local_path: jpeg(index), truth_kind: prose ? "prose" : "tags",
    truth_eval: pick(TRUTH_POOL, 10 + Math.floor(r() * 5), r), expected_miss: pick(APPEARANCE, 2, r) };
}
const PROSE_TRUTH = "A girl reads by the library window while late afternoon light falls across the desk and a cat sleeps on the chair.";
function questionsFor(query = {}) {
  const books = query.codex_id ? [query.codex_id] : ["nai5_community_pack", "artist_nai5_personal"];
  const items = books.flatMap((codex_id) => desktop.list({ ...query, codex_id, limit: 1000, offset: 0 }).items);
  return items
    .filter((e) => !query.main_image_only || e.image_count > 0)
    .slice(query.offset || 0, (query.offset || 0) + (query.limit || 200))
    .map((e) => {
      const q = question(`${e.entry_id}#0`);
      const prose = q.truth_kind === "prose";
      return { question_id: q.question_id, entry_id: e.entry_id, ordinal: 0, title: e.title, rating: e.rating, thumb_path: e.thumb_path, local_path: e.image_path,
        truth_source: "tags", truth_preview: (prose ? PROSE_TRUTH : [...q.truth_eval, ...q.expected_miss].join(", ")).slice(0, 200), prose };
    });
}

// Runs
function templateBoost(template_id) {
  const version = state.templates.find((t) => t.template_id === template_id)?.version ?? 1;
  return version >= 2 ? { precision: 0.05, recall: 0.05 } : { precision: 0, recall: 0 };
}
const label = (list, idKey, id) => {
  const item = list.find((x) => x[idKey] === id);
  return item ? `${item.name} v${item.version}` : `#${id}`;
};
function emptyRender() {
  return { render_path: null, render_seed: null, render_prompt: null, quality_score: null, quality_notes: [], similarity_score: null, similarity_reason: null,
    render_error: null, render_artist: null, render_artist_source: null, render_size: null, render_size_source: null, plugin_dropped: [], plugin_would_reject: false,
    render_candidates: [], render_style_entry_id: null, render_style_title: null, render_style_score: null, render_style_decision: null, render_style_fallback: false };
}
/** One redraw: same seed/count per question across batches, judge scores depend on the model and the style-recipe source. */
function render(run, q, model, spec, key) {
  const r = rng(key), mode = spec.mode || "pool", m = MODELS[model] ?? MODELS["vision-model-b"];
  if (r() < 0.04) return { ...emptyRender(), render_error: "出图超时：生成后端 120 秒内未返回，已保留反推结果" };
  const seed = (spec.seed ?? 410000) + q.index * 7919;
  let artist = null, source = mode === "none" ? null : mode, style = {};
  if (mode === "pool") artist = `artist:sample_pool_${String(1 + Math.floor(r() * 9)).padStart(2, "0")}, soft colors`;
  if (mode === "given") artist = spec.artist || null;
  if (mode === "style") {
    const k = (q.index * 5 + 3) % 42, score = round(0.55 + r() * 0.27, 3), fallback = r() < 0.1;
    style = { render_style_entry_id: `fixture-artist_nai5_personal-${k}`, render_style_title: `合成示例 ${k + 1}`, render_style_score: fallback ? round(score - 0.2, 3) : score,
      render_style_decision: ["undecided", "include", "exclude"][k % 3], render_style_fallback: fallback };
    if (fallback) { source = "pool"; artist = `artist:sample_pool_${String(1 + Math.floor(r() * 9)).padStart(2, "0")}, soft colors`; }
    else artist = "artist:synthetic_example, luminous colors, soft lighting";
  }
  const lift = mode === "style" && !style.render_style_fallback ? 0.8 : 0;
  const candidates = Array.from({ length: clamp(spec.count || 2, 1, 4) }, (_, i) => {
    const quality = round(clamp(m.quality + (r() - 0.5) * 1.8, 0, 10), 1);
    return { file: `reverse/${run.run_id}_${i}.jpg`, seed: seed + i, quality_score: quality, quality_notes: pick(QUALITY_NOTES, 2, r), local_path: jpeg(`render-${run.run_id}-${i}`) };
  });
  const best = candidates.reduce((a, c) => (c.quality_score > a.quality_score ? c : a), candidates[0]);
  return { ...emptyRender(), ...style,
    render_path: best.local_path, render_seed: best.seed, render_prompt: [artist, ...run.cleaned_tags].filter(Boolean).join(", "),
    quality_score: best.quality_score, quality_notes: best.quality_notes,
    similarity_score: round(clamp(m.similarity + lift + (r() - 0.5) * 1.6, 0, 10), 1),
    similarity_reason: lift ? "线条与上色接近原图，背景层次略少" : "主体与构图一致，上色和笔触差异明显",
    render_artist: artist, render_artist_source: source, render_size: "832x1216", render_size_source: "ratio",
    plugin_dropped: r() < 0.2 ? ["from side"] : [], plugin_would_reject: run.cleaned_tags.length < 5, render_candidates: candidates };
}
function makeRun(batch, provider, q, run_id) {
  const m = MODELS[provider.model] ?? MODELS["vision-model-b"], boost = templateBoost(batch.template_id);
  // r: per-batch draws (latency, failures, jitter). t: per model × question draws shared by every batch,
  // so a template change shows up as a consistent shift instead of being drowned by sampling noise.
  const r = rng(`${batch.batch_id}|${provider.provider_id}|${q.question_id ?? q.local_path}`);
  const t = rng(`tags|${provider.model}|${q.question_id ?? q.local_path}`);
  const latency_ms = Math.round(m.latency[0] + r() * (m.latency[1] - m.latency[0]));
  const row = { run_id, question_id: q.question_id, provider_id: provider.provider_id, provider: provider.name, status: "ok", latency_ms, error: null,
    cleaned_tags: [], metric: null, thumb_path: q.thumb_path, truth_kind: q.truth_kind, leak_appearance: 0, leak_banned: 0, ...emptyRender() };
  const detail = { raw_response: null, parsed: null, pred_raw: [], pred: [], truth: [...q.truth_eval, ...q.expected_miss], truth_eval: q.truth_kind === "tags" ? q.truth_eval : [],
    expected_miss: q.truth_kind === "tags" ? q.expected_miss : [], hits: [], misses: [], extras: [], expected_hits: [], contradictions: [],
    request: { batch_id: batch.batch_id, question_id: q.question_id, provider_id: provider.provider_id, model: provider.model, max_tokens: provider.max_tokens,
      template: label(state.templates, "template_id", batch.template_id), ruleset: label(state.rulesets, "ruleset_id", batch.ruleset_id),
      render: batch.render ? { artist_mode: batch.render.mode, count: batch.render.count, seed: (batch.render.seed ?? 410000) + q.index * 7919, judge_provider_id: null } : null } };
  if (q.truth_kind === "prose") { detail.truth = []; detail.truth_eval = []; }
  if (r() < m.error) {
    Object.assign(row, { status: "error", error: "HTTP 429：请求过于频繁，本条记为失败", latency_ms: Math.round(180 + r() * 240) });
    return { row, detail };
  }
  if (r() < m.refusal) {
    const parsed = { suitable: false, reason: "unable to describe this image" };
    Object.assign(row, { status: "refused", error: `suitable=false: ${parsed.reason}` });
    Object.assign(detail, { raw_response: JSON.stringify(parsed), parsed });
    return { row, detail };
  }
  const recall = clamp(m.recall + boost.recall + (t() - 0.5) * 0.2 + (r() - 0.5) * 0.06, 0.2, 0.98);
  const precision = clamp(m.precision + boost.precision + (t() - 0.5) * 0.2 + (r() - 0.5) * 0.06, 0.25, 0.98);
  // Local images have no answer key: the model still returns tags, but no P/R/F1 is computed.
  const hits = (q.truth_kind === "none" ? pick(TRUTH_POOL, 12, t) : q.truth_eval).filter(() => t() < recall);
  const extraCount = Math.max(0, Math.round((hits.length * (1 - precision)) / precision + (t() - 0.5) * 2));
  const extras = pick(DISTRACTORS.filter((tag) => !q.truth_eval.includes(tag)), extraCount, t);
  const contradictions = q.truth_eval.includes("outdoors") && r() < m.contradiction ? ["indoors"] : [];
  extras.push(...contradictions);
  const pred = [...hits, ...extras].sort(() => r() - 0.5);
  const leaked = q.expected_miss.filter(() => r() < m.leak), banned = r() < m.banned ? ["signature"] : [], dropped = r() < 0.3 ? ["masterpiece"] : [];
  const pred_raw = [...dropped, ...pred.slice(0, 3), ...leaked, ...pred.slice(3), ...banned];
  const parsed = { suitable: true, framing: ["upper body", "cowboy shot", "full body"][q.index % 3], tags: pred_raw.join(", ") };
  Object.assign(detail, { raw_response: JSON.stringify(parsed), parsed, pred_raw, pred });
  Object.assign(row, { cleaned_tags: pred, leak_appearance: leaked.length, leak_banned: banned.length });
  if (q.truth_kind === "tags") {
    const p = pred.length ? hits.length / pred.length : 0, rc = hits.length / q.truth_eval.length;
    row.metric = { hit: hits.length, missed: q.truth_eval.length - hits.length, extra: extras.length, expected_hit: 0,
      precision: round(p), recall: round(rc), f1: round(p + rc > 0 ? (2 * p * rc) / (p + rc) : 0),
      appearance_leak: leaked.length, banned_leak: banned.length, ruleset_drop: pred_raw.length - pred.length, contradiction: contradictions.length };
    Object.assign(detail, { hits, misses: q.truth_eval.filter((t) => !hits.includes(t)), extras, contradictions });
  }
  if (batch.render) Object.assign(row, render(row, q, provider.model, batch.render, `render|${batch.batch_id}|${run_id}|${provider.provider_id}`));
  return { row, detail };
}
function generate(batch) {
  const rows = [];
  for (const question_id of batch.question_ids) {
    const q = question(question_id);
    for (const provider_id of batch.provider_ids) {
      const provider = state.providers.find((p) => p.provider_id === provider_id);
      if (!provider) continue;
      const { row, detail } = makeRun(batch, provider, q, state.nextRun++);
      rows.push(row);
      state.details.set(row.run_id, detail);
    }
  }
  state.runs.set(batch.batch_id, rows);
}
/** Batches started from the page finish on a timer (no Tauri Channel in mock mode; the store polls the batch list). */
function advance(batch) {
  if (batch.status !== "running") return batch;
  const total = state.runs.get(batch.batch_id).length;
  if (Date.now() - batch.started >= Math.min(4000, 400 * total)) Object.assign(batch, { status: "done", finished_at: new Date().toISOString() });
  return batch;
}
function visibleRuns(batch) {
  const rows = state.runs.get(batch.batch_id) ?? [];
  if (batch.status === "done") return rows;
  if (batch.status === "cancelled") return rows.slice(0, batch.kept ?? 0);
  return rows.slice(0, Math.floor((Date.now() - batch.started) / 400));
}
function summary(batch) {
  advance(batch);
  return { batch_id: batch.batch_id, created_at: batch.created_at, finished_at: batch.finished_at, status: batch.status,
    template: label(state.templates, "template_id", batch.template_id), ruleset: label(state.rulesets, "ruleset_id", batch.ruleset_id),
    providers: batch.provider_ids.map((id) => state.providers.find((p) => p.provider_id === id)?.name ?? `#${id}`),
    question_count: batch.question_ids.length, note: batch.note };
}
function batchById(batch_id) {
  const batch = state.batches.find((b) => b.batch_id === batch_id);
  if (!batch) throw { code: "not_found", message: `批次不存在：${batch_id}` };
  return batch;
}
/** Same aggregation as the desktop backend: macro means over tag questions, leak sums over every run. */
function results(batch) {
  const batchSummary = summary(batch), runs = visibleRuns(batch), byProvider = new Map();
  for (const run of runs) byProvider.set(run.provider_id, [...(byProvider.get(run.provider_id) ?? []), run]);
  const rows = [...byProvider.entries()].sort(([a], [b]) => a - b).map(([provider_id, list]) => {
    const tagMetrics = list.filter((r) => r.truth_kind === "tags" && r.metric).map((r) => r.metric);
    const styleUsed = list.filter((r) => r.render_style_entry_id && !r.render_style_fallback);
    const avg = (values, digits = 4) => { const v = mean(values); return v === null ? null : round(v, digits); };
    return { provider_id, provider: list[0].provider, runs: list.length,
      ok: list.filter((r) => r.status === "ok").length, refused: list.filter((r) => r.status === "refused").length, errors: list.filter((r) => r.status === "error").length,
      precision: avg(tagMetrics.map((m) => m.precision)), recall: avg(tagMetrics.map((m) => m.recall)), f1: avg(tagMetrics.map((m) => m.f1)),
      appearance_leak: list.reduce((a, r) => a + r.leak_appearance, 0), banned_leak: list.reduce((a, r) => a + r.leak_banned, 0),
      contradiction: list.reduce((a, r) => a + (r.metric?.contradiction ?? 0), 0),
      avg_latency_ms: avg(list.map((r) => r.latency_ms).filter((v) => v !== null), 1), tags_n: tagMetrics.length,
      render_n: list.filter((r) => r.render_path).length,
      quality_avg: avg(list.map((r) => r.quality_score).filter((v) => v !== null), 2),
      similarity_avg: avg(list.map((r) => r.similarity_score).filter((v) => v !== null), 2),
      style_hit_n: styleUsed.length, style_score_avg: avg(styleUsed.map((r) => r.render_style_score).filter((v) => v !== null), 3) };
  });
  return { batch: batchSummary, runs: clone(runs), summary: rows };
}
/** Mean F1 per question across providers; `run_*` is the first run of that question, as in the backend. */
function compare(batch_a, batch_b) {
  const a = batchById(batch_a), b = batchById(batch_b);
  const collect = (batch) => {
    const map = new Map();
    for (const run of visibleRuns(batch)) {
      if (!run.question_id) continue;
      const entry = map.get(run.question_id) ?? { f1: [], run_id: run.run_id };
      if (run.metric?.f1 != null) entry.f1.push(run.metric.f1);
      map.set(run.question_id, entry);
    }
    return map;
  };
  const ma = collect(a), mb = collect(b), all_a = [], all_b = [];
  const rows = [...new Set([...ma.keys(), ...mb.keys()])].sort().map((question_id) => {
    const x = ma.get(question_id), y = mb.get(question_id);
    const f1_a = x ? mean(x.f1) : null, f1_b = y ? mean(y.f1) : null;
    if (f1_a !== null) all_a.push(f1_a);
    if (f1_b !== null) all_b.push(f1_b);
    return { question_id, f1_a: f1_a === null ? null : round(f1_a), f1_b: f1_b === null ? null : round(f1_b),
      delta: f1_a !== null && f1_b !== null ? round(f1_b - f1_a) : null, run_a: x?.run_id ?? null, run_b: y?.run_id ?? null };
  });
  const avg = (v) => { const m = mean(v); return m === null ? null : round(m); };
  return { batch_a: summary(a), batch_b: summary(b), rows, avg_f1_a: avg(all_a), avg_f1_b: avg(all_b) };
}
function findRun(run_id) {
  for (const [batch_id, rows] of state.runs) {
    const index = rows.findIndex((r) => r.run_id === run_id);
    if (index >= 0) return { batch: batchById(batch_id), rows, index };
  }
  throw { code: "not_found", message: `run #${run_id} 不存在` };
}
function saveVersioned(list, idKey, input, fields) {
  if (!input?.name?.trim()) throw { code: "bad_request", message: "名称不能为空" };
  const version = Math.max(0, ...list.filter((x) => x.name === input.name).map((x) => x.version)) + 1;
  const saved = { [idKey]: ++state.nextId, name: input.name, version, ...fields, created_at: new Date().toISOString(), note: input.note ?? null };
  list.push(saved);
  return clone(saved);
}

export async function answerReverse(command, args = {}, scenario = "") {
  if (scenario === "empty" && command.endsWith("_list")) return [];
  switch (command) {
    case "reverse_providers_list": return clone(state.providers);
    case "reverse_templates_list": return clone(state.templates);
    case "reverse_rulesets_list": return clone(state.rulesets);
    case "reverse_template_save": {
      const { input } = args;
      return saveVersioned(state.templates, "template_id", input, { body: input.body, parent_id: input.parent_id ?? null });
    }
    case "reverse_ruleset_save": {
      const { input } = args, { name, note, ...rules } = input;
      return saveVersioned(state.rulesets, "ruleset_id", input, rules);
    }
    case "reverse_provider_save": {
      const { input } = args;
      if (!input?.name?.trim()) throw { code: "bad_request", message: "名称不能为空" };
      const current = state.providers.find((p) => p.provider_id === input.provider_id);
      const { provider_id, ...fields } = input;
      const value = { ...(current ?? { provider_id: ++state.nextId, has_key: false, created_at: new Date().toISOString() }), ...fields,
        plugin_provider_id: fields.kind === "plugin" ? fields.plugin_provider_id : null };
      if (current) Object.assign(current, value);
      else state.providers.push(value);
      return clone(value);
    }
    case "reverse_provider_delete":
      state.providers = state.providers.filter((p) => p.provider_id !== args.provider_id);
      return null;
    case "reverse_questions": return questionsFor(args.query);
    case "reverse_pick_local_image": return null;
    case "reverse_import_local_image": return args.path;
    case "reverse_batches_list":
      return [...state.batches].sort((a, b) => b.created_at.localeCompare(a.created_at)).map(summary);
    case "reverse_batch_results": return results(batchById(args.batch_id));
    case "reverse_compare": return compare(args.batch_a, args.batch_b);
    case "reverse_run_get": {
      const { rows, index } = findRun(args.run_id), run = rows[index];
      return clone({ run, ...state.details.get(run.run_id), judgment: state.judgments.get(run.run_id) ?? null });
    }
    case "reverse_source_image": {
      try {
        const { rows, index } = findRun(args.run_id), run = rows[index];
        if (!run.question_id) return { path: run.thumb_path, kind: "original" };
        return { path: question(run.question_id).local_path, kind: "display" };
      } catch {
        return { path: null, kind: "missing" };
      }
    }
    case "reverse_judge":
      findRun(args.run_id);
      state.judgments.set(args.run_id, { score: args.score, comment: args.comment, at: new Date().toISOString() });
      return null;
    case "reverse_render_run": {
      const { batch, rows, index } = findRun(args.run_id), run = rows[index];
      if (run.status !== "ok") throw { code: "bad_request", message: "只有解析成功的 run 才能重画" };
      const spec = { ...(batch.render ?? { count: 2, seed: 410000 }), mode: args.render_artist_mode || batch.render?.mode || "pool", artist: args.artist, count: args.render_count ?? batch.render?.count ?? 2 };
      const q = question(run.question_id ?? `local:${run.thumb_path}`);
      const next = { ...run, ...render(run, q, run.provider, spec, `rerender|${run.run_id}|${++state.sequence}`) };
      rows[index] = next;
      const detail = state.details.get(run.run_id);
      detail.request = { ...detail.request, last_render_request: { artist_mode: spec.mode, count: spec.count, judge_provider_id: args.judge_provider_id ?? null, style_model: spec.mode === "style" ? args.style_model ?? null : null } };
      return clone(next);
    }
    case "reverse_run_batch": {
      const req = args.req ?? {};
      if (!req.question_ids?.length) throw { code: "bad_request", message: "请先勾选题目" };
      if (!state.templates.some((t) => t.template_id === req.template_id)) throw { code: "bad_request", message: "模板不存在" };
      if (!state.rulesets.some((r) => r.ruleset_id === req.ruleset_id)) throw { code: "bad_request", message: "规则集不存在" };
      const provider_ids = (req.provider_ids ?? []).filter((id) => state.providers.some((p) => p.provider_id === id && p.enabled));
      if (!provider_ids.length) throw { code: "bad_request", message: "请至少选择一个已启用的模型" };
      const now = new Date();
      const batch = { batch_id: `rb-mock-${now.getTime().toString(36)}-${++state.sequence}`, created_at: now.toISOString(), finished_at: null, status: "running",
        template_id: req.template_id, ruleset_id: req.ruleset_id, provider_ids, question_ids: [...req.question_ids], note: req.note?.trim() || null,
        render: req.render ? { mode: req.render_artist_mode || "pool", count: req.render_count ?? 2, seed: req.render_seed ?? 410000, artist: req.artist ?? null } : null,
        started: Date.now() };
      state.batches.push(batch);
      generate(batch);
      return { batch_id: batch.batch_id };
    }
    case "reverse_cancel_batch": {
      const batch = advance(batchById(args.batch_id));
      if (batch.status === "running") Object.assign(batch, { status: "cancelled", finished_at: new Date().toISOString(), kept: visibleRuns(batch).length });
      return null;
    }
    default: throw { code: "not_found", message: `Mock 未登记反推命令 ${command}` };
  }
}

/** style_search over the synthetic style dictionary: descending cosine scores, blacklisted entries never returned. */
export function styleSearch(req = {}) {
  const codex_id = req.codex_ids?.[0] ?? "artist_nai5_personal";
  const page = desktop.list({ codex_id, decision: req.decisions?.length ? req.decisions : undefined, limit: 1000 });
  const r = rng(`style|${JSON.stringify(req.source ?? {})}|${req.model ?? ""}|${req.center}`);
  const hits = page.items
    .map((e) => ({ e, score: round(0.3 + r() * 0.55) }))
    .sort((a, b) => b.score - a.score)
    .slice(0, clamp(req.top_k ?? 10, 1, 50))
    .map(({ e, score }) => ({ entry_id: e.entry_id, codex_id: e.codex_id, title: e.title, path: e.path, decision: e.decision, blacklisted: false, score,
      style_prompt: e.tags_first_line, core: codex_id === "artist_nai5_personal" ? e.tags_first_line : null,
      pack_tags: codex_id === "artist_nai5_personal" ? null : e.tags_first_line, thumb_path: e.thumb_path, prose: false }));
  return { model: req.model ?? "megastyle-siglip-so400m", indexed: page.total, query_ms: Math.round(30 + r() * 60), hits };
}

resetReverse();
