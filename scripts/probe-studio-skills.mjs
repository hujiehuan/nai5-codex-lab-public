// Offline actual-UI regression for plugin-selected material snapshots. No live model or bot calls.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, studioMetrics } from "../mock/studio.mjs";
import { setStudioPluginSkillState, studioPluginSkillsRoute } from "../mock/plugin/studio-skills.mjs";
import { executablePath } from "./chrome.mjs";

const out = process.env.STUDIO_SKILLS_OUT || "/tmp/nai5-studio-skills-probe";
await mkdir(out, { recursive: true });
const server = await createServer({ mode: "mock", cacheDir: join(out, "vite-cache"), optimizeDeps: { include: ["@tauri-apps/plugin-dialog"] }, server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), calls = [], errors = [], blocked = [], measurements = [], cases = [];
let scenario = "", afterPlan = null, holdKind = null, releaseSync = null;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
page.on("pageerror", (error) => errors.push(error.message));
await page.evaluateOnNewDocument(() => { window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, convertFileSrc: (path) => path, invoke: async () => 1 }; });
await page.setRequestInterception(true);
page.on("request", async (request) => {
  const url = new URL(request.url());
  if (![base, "null"].includes(url.origin)) { blocked.push(url.origin); return request.abort(); }
  if (url.pathname !== "/__mock__/invoke") return request.continue();
  const body = JSON.parse(request.postData());
  if (!(body.command.startsWith("studio_") || body.command === "reverse_pick_local_image")) return request.continue();
  const call = { ...body, scenario }; calls.push(call);
  try {
    const value = body.command === "reverse_pick_local_image"
      ? "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='100'%20height='140'%3E%3Crect%20width='100'%20height='140'%20fill='plum'/%3E%3C/svg%3E"
      : await answerStudio(body.command, body.args, scenario);
    call.result = value;
    if (body.command === "studio_skills_sync" && body.args.kind === holdKind) await new Promise((resolve) => { releaseSync = resolve; });
    await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
  } catch (error) {
    call.error = error;
    await request.respond({ status: 400, contentType: "application/json", body: JSON.stringify(error) });
  } finally { if (body.command === "studio_plan" && afterPlan) { const effect = afterPlan; afterPlan = null; effect(); } }
});
const since = (command, offset = 0) => calls.slice(offset).filter((call) => call.command === command);
const region = '[data-testid="studio-plugin-skill"]';
async function waitText(text) { await page.waitForFunction((text) => document.body.textContent.includes(text), {}, text); }
async function waitSkill(text) { await page.waitForFunction(({ region, text }) => document.querySelector(region)?.textContent.includes(text) && document.querySelector(region)?.getAttribute("aria-busy") !== "true", {}, { region, text }); }
async function button(text) {
  for (const item of await page.$$("button")) if (await item.evaluate((e, text) => e.checkVisibility() && e.textContent.trim() === text, text)) {
    await item.evaluate((e) => e.scrollIntoView({ block: "center" }));
    assert.equal(await item.evaluate((e) => e.disabled), false, `Button disabled: ${text}`);
    await item.click(); return;
  }
  throw Error(`Visible button missing: ${text}`);
}
async function select(label, text) {
  const selector = `.n-select[aria-label="${label}"]`;
  await page.$eval(selector, (e) => e.scrollIntoView({ block: "center" })); await page.click(selector);
  await page.waitForSelector(".n-base-select-option"); let found = false;
  for (const option of await page.$$(".n-base-select-option")) if ((await option.evaluate((e) => e.textContent)).includes(text)) { await option.click(); found = true; break; }
  await page.keyboard.press("Escape"); assert(found, `Missing skill option ${text}`);
}
async function go(mode = "draw", state = "", workspace = null) {
  await page.goto("about:blank"); await delay(300); resetStudio(); scenario = state;
  if (workspace) await answerStudio("studio_workspace_save", { state: workspace });
  const offset = calls.length;
  await page.goto(`${base}/?skill-probe=${Date.now()}#/studio?mode=${mode}`, { waitUntil: "networkidle0" });
  await page.waitForSelector(region);
  await page.waitForFunction((region) => document.querySelector(region)?.getAttribute("aria-busy") !== "true" && !document.body.textContent.includes("正在读取插件实验能力"), {}, region);
  return offset;
}
async function finishPlans(count) {
  await page.waitForFunction((count) => document.querySelectorAll(".matrix tbody tr").length === count && [...document.querySelectorAll('.action-bar button')].some((e) => !e.disabled && e.textContent.trim() === "构建对照方案"), {}, count);
}
async function savedWorkspace() { await delay(350); return await answerStudio("studio_workspace_get"); }
async function measure(name) {
  for (const width of [1280, 390]) for (const theme of ["light", "dark"]) {
    await page.setViewport({ width, height: 1000 }); await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
    await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, {}, theme); await delay(750);
    const value = await page.evaluate(() => {
      const root = document.querySelector('.skill-editor');
      const visible = (e) => e.checkVisibility() && e.getBoundingClientRect().width > 0 && getComputedStyle(e).visibility !== "hidden";
      const targets = [...root.querySelectorAll('button,input,select,summary,[role="combobox"]')].filter((e) => visible(e) && !e.disabled && !e.closest('.n-base-selection-input-tag')).map((e) => { const r = e.getBoundingClientRect(); return { text: e.getAttribute("aria-label") || e.textContent.trim().slice(0, 50), width: r.width, height: r.height }; });
      const color = (s) => (s.match(/[\d.]+/g) || []).map(Number);
      const blend = (a, b) => [0, 1, 2].map((i) => (a[i] || 0) * (a[3] ?? 1) + b[i] * (1 - (a[3] ?? 1)));
      const lum = (c) => c.slice(0, 3).map((v) => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
      const contrast = [], walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT); let node;
      while ((node = walker.nextNode())) {
        const e = node.parentElement; if (!e || !node.textContent.trim() || !visible(e) || ["STYLE", "SCRIPT", "OPTION"].includes(e.tagName) || e.closest('[disabled],.n-button--disabled')) continue;
        const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p);
        if (parents.some((p) => +getComputedStyle(p).opacity < .99)) continue;
        let bg = [255, 255, 255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg);
        const s = getComputedStyle(e), a = lum(blend(color(s.color), bg)), b = lum(bg), large = parseFloat(s.fontSize) >= 24 || (parseFloat(s.fontSize) >= 18.66 && parseInt(s.fontWeight) >= 700);
        contrast.push({ text: node.textContent.trim().slice(0, 100), ratio: (Math.max(a, b) + .05) / (Math.min(a, b) + .05), minimum: large ? 3 : 4.5, foreground: s.color, background: bg, className: e.className });
      }
      return { overflow: document.documentElement.scrollWidth - innerWidth, targets, contrast };
    });
    measurements.push({ name, width, theme, ...value });
    assert(value.contrast.length > 0);
    assert.equal(value.overflow, 0, `Page overflow: ${name}/${width}/${theme}`);
    assert.deepEqual(value.targets.filter((e) => e.width < 24 || e.height < 24), [], `Small skill control: ${JSON.stringify(value.targets)}`);
    await page.screenshot({ path: join(out, `${name}-${width}-${theme}.png`), fullPage: true });
  }
}

try {
  await page.setViewport({ width: 1280, height: 1000 });
  const first = await go(); await waitSkill("fixture-v1");
  assert(since("studio_skills_sync", first).every((call) => call.args.kind === "draw"));
  assert((await page.$eval('.skill-group', (e) => e.textContent)).includes("跟随插件"));
  await measure("current");
  await button("查看已同步全文"); await waitText("版本 1：保留用户要求"); await button("关闭预览");
  await button("＋ 添加 skill 组合"); await select("组合 2 的 skill", "跟随插件");
  setStudioPluginSkillState("draw", { version: 2 }); scenario = "studio-plan-error";
  const round = calls.length;
  afterPlan = () => setStudioPluginSkillState("draw", { version: 3 });
  await button("构建对照方案"); await finishPlans(2); await waitText("模拟构思失败");
  const sync = since("studio_skills_sync", round), prepare = since("studio_prepare", round), plans = since("studio_plan", round);
  assert.equal(sync.length, 1, "New round forces exactly one fresh selection for all groups");
  assert.equal(sync[0].result.skill.version, "fixture-v2"); assert.equal(prepare.length, 1); assert.equal(plans.length, 2);
  assert(calls.indexOf(sync[0]) < calls.indexOf(prepare[0]), "Sync must precede input freeze");
  const frozenId = sync[0].result.skill.id;
  for (const call of plans) { assert.deepEqual(call.args.req.skill_ids, [frozenId]); assert.equal(call.args.req.input_id, prepare[0].result.input_id); }
  assert.equal((await answerStudio("studio_skills_list")).length, 1, "Historical snapshots must not fill the ordinary library");
  const saved = await savedWorkspace(); assert.equal(saved.skill_follow_version, 1); assert.deepEqual(saved.groups[0].skill_ids, ["plugin-active"]);
  scenario = ""; setStudioPluginSkillState("draw", { version: 4 });
  await page.reload({ waitUntil: "networkidle0" }); await waitSkill("fixture-v4"); await waitText("重试构思（API）");
  const retry = calls.length; await button("重试构思（API）"); await waitText("方案已冻结");
  assert.equal(since("studio_skills_sync", retry).length, 0, "Retry must not replace its frozen material with the current plugin selection");
  assert.equal(since("studio_prepare", retry).length, 0); assert.equal(since("studio_plan", retry).length, 1);
  assert.deepEqual(since("studio_plan", retry)[0].args.req.skill_ids, [frozenId]);
  assert.equal(since("studio_plan", retry)[0].result.settings.skill_bundles[0].version, "fixture-v2");
  const next = calls.length; await button("构建对照方案"); await finishPlans(4);
  const newId = since("studio_skills_sync", next)[0].result.skill.id; assert.notEqual(newId, frozenId);
  assert(since("studio_plan", next).every((call) => call.args.req.skill_ids[0] === newId));
  cases.push("fresh round v2 shared across two groups; v3 selection during planning ignored; restored retry remains v2; new round follows v4");

  setStudioPluginSkillState("draw", { version: 5 }); const focus = calls.length;
  await page.evaluate(() => window.dispatchEvent(new Event("focus"))); await waitSkill("fixture-v5");
  assert.equal(since("studio_prepare", focus).length, 0); assert.equal(since("studio_plan", focus).length, 0);
  cases.push("focus refreshes display without planning or generation");
  const longVersion = `2026-09-21-${"long-revision-".repeat(8)}abcdef`;
  setStudioPluginSkillState("draw", { version: longVersion }); await button("同步插件 skill"); await waitSkill(longVersion);
  await measure("long-version"); cases.push("long plugin version wraps without page overflow");

  await go("reverse", "studio-skills-disabled"); await waitSkill("未启用反推 skill"); await measure("disabled");
  await button("选择反推原图"); const disabled = calls.length; await button("构建对照方案"); await finishPlans(1);
  assert.equal(since("studio_skills_sync", disabled)[0].args.kind, "reverse");
  assert.deepEqual(since("studio_plan", disabled)[0].args.req.skill_ids, []);
  assert.deepEqual(since("studio_plan", disabled)[0].result.settings.skill_bundles, []);
  cases.push("reverse disabled means zero materials, no builtin fallback");

  for (const error of ["error", "missing", "tampered", "malformed", "legacy"]) {
    await go(); await waitSkill("fixture-v1"); scenario = `studio-skills-${error}`;
    const offset = calls.length; await button("构建对照方案"); await waitSkill("同步失败");
    assert.equal(since("studio_prepare", offset).length, 0, `${error} must fail before preparing input`);
    assert.equal(since("studio_plan", offset).length, 0, `${error} must not call a model or use cached material`);
    if (error === "legacy") await measure("sync-error");
    cases.push(`${error}: no stale success, input preparation or model call`);
  }
  await go("draw", "studio-skills-legacy", { version: 1, skill_follow_version: 1, groups: [{ id: "fixed", name: "固定资料对照", skill_ids: ["builtin-guide"] }] });
  await waitSkill("同步失败"); const fixed = calls.length; await button("构建对照方案"); await finishPlans(1);
  assert.equal(since("studio_skills_sync", fixed).length, 0); assert.deepEqual(since("studio_plan", fixed)[0].args.req.skill_ids, ["builtin-guide"]);
  cases.push("explicit fixed material remains usable with an old plugin; no automatic fallback");

  await go(); await waitSkill("fixture-v1"); holdKind = "reverse";
  await page.evaluate(() => { location.hash = "#/studio?mode=reverse"; });
  for (let i = 0; i < 100 && !releaseSync; i++) await delay(25);
  assert(releaseSync, "Reverse synchronization must start on mode change");
  setStudioPluginSkillState("draw", { version: 6 }); await page.evaluate(() => { location.hash = "#/studio?mode=draw"; });
  await waitSkill("fixture-v6"); holdKind = null; releaseSync(); releaseSync = null; await delay(150);
  assert((await page.$eval(region, (e) => e.textContent)).includes("插件创作资料"));
  assert(!(await page.$eval(region, (e) => e.textContent)).includes("插件反推资料"));
  cases.push("late reverse response cannot overwrite draw display");

  // Legacy migration is deliberately narrow; user-created and explicit no-skill controls survive.
  for (const [name, workspace, expected] of [
    ["untouched-default", { version: 1, groups: [{ id: "default", name: "默认资料", skill_ids: ["nai5-prompting"] }] }, ["plugin-active"]],
    ["custom-name", { version: 1, groups: [{ id: "default", name: "自定义固定组合", skill_ids: ["builtin-guide"] }] }, ["builtin-guide"]],
    ["explicit-empty", { version: 1, groups: [{ id: "default", name: "默认资料", skill_ids: [] }] }, []],
    ["already-migrated-fixed", { version: 1, skill_follow_version: 1, groups: [{ id: "default", name: "默认资料", skill_ids: ["builtin-guide"] }] }, ["builtin-guide"]],
  ]) {
    await go("draw", "", workspace); await waitSkill("fixture-v1"); await button("构建对照方案"); await finishPlans(1);
    const persisted = await savedWorkspace(); assert.deepEqual(persisted.groups[0].skill_ids, expected, name);
    assert.equal(persisted.skill_follow_version, 1, name); cases.push(`workspace: ${name}`);
  }
  assert.equal(studioPluginSkillsRoute({ features: "studio_v1", kind: "draw" }).status, "ok");
  assert.equal(studioMetrics.generations, 0); assert.deepEqual(errors, []); assert.deepEqual(blocked, []);
  assert.deepEqual(measurements.flatMap((m) => m.contrast.filter((e) => e.ratio + .01 < e.minimum).map((e) => ({ name: m.name, width: m.width, theme: m.theme, ...e }))), [], "Skill text must meet AA in every measured state");
  await writeFile(join(out, "measurements.json"), JSON.stringify({ cases, measurements, calls: calls.map(({ command, args, error }) => ({ command, args, error })) }, null, 2));
  console.log(`PASS Studio skill synchronization: ${cases.length} scenarios, ${measurements.length} viewport/theme checks. ${out}`);
} catch (error) {
  await writeFile(join(out, "failure.json"), JSON.stringify({ error: String(error), cases, measurements, recentCalls: calls.slice(-15) }, null, 2));
  await page.screenshot({ path: join(out, "failure.png"), fullPage: true }).catch(() => {});
  throw error;
} finally { if (releaseSync) releaseSync(); await browser.close(); await server.close(); }
