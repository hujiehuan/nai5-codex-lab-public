// Offline UI execution: no live bot, user data, LLM or NovelAI calls.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, studioMetrics } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";

const out = process.env.STUDIO_OUT || "/tmp/nai5-studio-probe";
await mkdir(out, { recursive: true });
const server = await createServer({ mode: "mock", cacheDir: join(out, "vite-cache"), optimizeDeps: { include: ["@tauri-apps/plugin-dialog"] }, server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen(); const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), calls = [], errors = [], measurements = [];
let scenario = "", picked = 0, holdStart = false, releaseStart, partialVisual = false;
page.on("pageerror", (e) => errors.push(e.message));
await page.evaluateOnNewDocument(() => { window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, convertFileSrc: (p) => p, invoke: async () => 1 }; });
await page.setRequestInterception(true);
page.on("request", async (r) => {
  if (!r.url().endsWith("/__mock__/invoke")) return r.continue();
  const body = JSON.parse(r.postData());
  if (!(body.command.startsWith("studio_") || body.command.startsWith("style_compare") || ["reverse_pick_local_image", "reverse_import_local_image", "style_models_list"].includes(body.command))) return r.continue();
  calls.push(body);
  try {
    let value;
    if (body.command === "reverse_pick_local_image") value = `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="140"><rect width="100" height="140" fill="plum"/><text y="50">${++picked}</text></svg>`)}`;
    else if (body.command === "reverse_import_local_image") value = `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="140"><rect width="100" height="140" fill="plum"/><text y="50">${picked}</text></svg>`)}`;
    else if (body.command === "style_models_list") value = ["clip-vit-base-patch32", "csd-vit-large-patch14", "megastyle-siglip-so400m"].map((model) => ({ model, label: model.split('-')[0], present: true }));
    else value = await answerStudio(body.command, body.args, scenario);
    if (body.command === "studio_compare" && partialVisual) value = { ...value, text: "原文完整保留：结构化排名未完成", ranking: [] };
    if (body.command === "studio_start" && holdStart) await new Promise((resolve) => { releaseStart = resolve; });
    body.result = value; await r.respond({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
  } catch (error) { if (body.command === "studio_start" && holdStart) await new Promise((resolve) => { releaseStart = resolve; }); body.error = error; await r.respond({ status: 400, contentType: "application/json", body: JSON.stringify(error) }); }
});
async function button(text, contains = false) {
  for (const e of await page.$$("button")) if (await e.evaluate((el, value) => { const r = el.getBoundingClientRect(); return r.width && r.height && (value.contains ? el.textContent.trim().includes(value.text) : el.textContent.trim() === value.text); }, { text, contains })) { await e.evaluate((el) => el.scrollIntoView({ block: "center" })); await e.click(); return; }
  throw Error(`Missing button ${text}`);
}
async function field(selector, value) { await page.waitForSelector(selector); await page.$eval(selector, (e, v) => { e.value = v; e.dispatchEvent(new Event("input", { bubbles: true })); e.dispatchEvent(new Event("change", { bubbles: true })); }, value); }
async function go(mode = "draw", state = "") { scenario = state; await page.goto(`${base}/?probe=${Date.now()}#/studio?mode=${mode}`, { waitUntil: "networkidle0" }); await page.waitForFunction(() => document.querySelector('[data-testid="studio"]') && !document.body.textContent.includes("正在读取插件实验能力")); }
async function waitText(text) { await page.waitForFunction((s) => document.body.textContent.includes(s), {}, text); }
async function select(label, text) { const selector = `.n-select[aria-label="${label}"]`; await page.$eval(selector, (e) => e.scrollIntoView({ block: "center" })); await page.click(selector); await page.waitForSelector(".n-base-select-option"); const options = await page.$$(".n-base-select-option"); for (const option of options) if ((await option.evaluate((e) => e.textContent)).includes(text)) { await option.click(); break; } await page.keyboard.press("Escape"); }
async function measure(name) {
  for (const width of [1280, 390]) for (const theme of ["light", "dark"]) {
    await page.setViewport({ width, height: 1000 }); await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
    await page.waitForFunction((t) => document.documentElement.dataset.theme === t, {}, theme); await new Promise((r) => setTimeout(r, 1000));
    const result = await page.evaluate(() => {
      const visible = (e) => { const b = e.getBoundingClientRect(); return e.checkVisibility() && b.width > 0 && b.height > 0 && getComputedStyle(e).visibility !== "hidden"; };
      const targets = [...document.querySelectorAll('.studio button,.studio input:not([type="hidden"]),.studio select,.studio textarea')].filter(visible).filter((e) => !e.disabled && !e.closest('.n-base-selection-input-tag')).map((e) => ({ text: e.getAttribute("aria-label") || e.textContent.trim().slice(0, 35) || e.tagName, w: e.getBoundingClientRect().width, h: e.getBoundingClientRect().height })).filter((e) => e.w < 24 || e.h < 24);
      const color = (s) => (s.match(/[\d.]+/g) || []).map(Number), blend = (a, b) => [0, 1, 2].map((i) => (a[i] || 0) * (a[3] ?? 1) + b[i] * (1 - (a[3] ?? 1)));
      const lum = (c) => c.slice(0, 3).map((v) => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
      const lowContrast = [], walker = document.createTreeWalker(document.querySelector('.studio'), NodeFilter.SHOW_TEXT); let node;
      while ((node = walker.nextNode())) { const e = node.parentElement; if (!e || !node.textContent.trim() || !visible(e) || ["STYLE", "SCRIPT"].includes(e.tagName) || e.closest('[disabled],.n-button--disabled')) continue; const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p); if (parents.some((p) => +getComputedStyle(p).opacity < .99)) continue; let bg = [255, 255, 255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg); const style = getComputedStyle(e), a = lum(blend(color(style.color), bg)), b = lum(bg), ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05), large = parseFloat(style.fontSize) >= 24 || (parseFloat(style.fontSize) >= 18.66 && parseInt(style.fontWeight) >= 700); if (ratio < (large ? 3 : 4.5) - .01) lowContrast.push({ text: node.textContent.trim().slice(0, 40), ratio, fg: style.color, bg, cls: e.className }); }
      return { overflow: document.documentElement.scrollWidth - innerWidth, targets, lowContrast };
    });
    measurements.push({ name, width, theme, ...result }); assert.equal(result.overflow, 0, JSON.stringify(result)); assert.deepEqual(result.targets, [], JSON.stringify(result)); assert.deepEqual(result.lowContrast, [], JSON.stringify(result));
    await page.evaluate(() => document.querySelector('main.content').scrollTo(0, 0)); await page.screenshot({ path: join(out, `${name}-${width}-${theme}.png`), fullPage: true });
  }
}
try {
  resetStudio(); await page.setViewport({ width: 1280, height: 1000 }); await go();
  await field('textarea[aria-label="必须满足的内容"]', "在沙滩上跳舞；每个方案必须保留这个要求。");
  await page.$eval('[data-testid="studio-reference-mode"]', (e) => { e.value = "all"; e.dispatchEvent(new Event("change", { bubbles: true })); });
  await waitText("同一次构思发送当前范围的全部合格图包");
  assert((await page.$eval('[data-testid="studio-reference-description"]', (e) => e.textContent)).includes("每条发送主参考图＋完整提示词"));
  assert(!/2\s*MiB|4\s*MiB/.test(await page.$eval('.configuration', (e) => e.textContent)), "Studio no longer advertises the retired application byte budgets");
  await button("＋ 添加 skill 组合"); await button("构建对照方案"); await waitText("方案已冻结");
  await page.waitForFunction(() => document.querySelectorAll('.matrix tbody tr').length === 2 && !document.body.textContent.includes("构思中…"));
  assert.equal(studioMetrics.prepare, 1); assert.equal(studioMetrics.plans, 2); assert.equal(studioMetrics.generations, 0);
  const plans = calls.filter((c) => c.command === "studio_plan"); assert.equal(plans[0].args.req.input_id, plans[1].args.req.input_id); assert.deepEqual(plans[0].args.req.characters, plans[1].args.req.characters); assert.equal(plans[0].args.req.requirements, plans[1].args.req.requirements);
  const prepared = calls.find((c) => c.command === "studio_prepare"); assert.equal(prepared.args.req.reference_mode, "all"); assert.equal(prepared.result.reference_count, 188);
  for (const row of plans) { assert.equal(row.result.settings.reference_mode, "all"); for (const p of row.result.plans) assert.deepEqual(p.reference_ids, prepared.result.reference_ids); }
  await measure("matrix"); await button("出图"); await waitText("已完成"); assert.equal(studioMetrics.generations, 1); assert.equal(studioMetrics.plans, 2);
  await page.waitForFunction(() => document.querySelector('.image-open img')); await measure("results");
  await page.reload({ waitUntil: "networkidle0" }); await waitText("方案已冻结"); assert.equal(studioMetrics.plans, 2, "workspace restore must not replan");
  scenario = "studio-start-lost"; await button("出图"); await waitText("提交未确认"); const start = calls.filter((c) => c.command === "studio_start").at(-1); assert.equal(studioMetrics.generations, 2);
  scenario = ""; await button("重试原请求"); await waitText("正在生成："); const retry = calls.filter((c) => c.command === "studio_start").at(-1); assert.equal(start.args.req.request_id, retry.args.req.request_id); assert.equal(studioMetrics.generations, 2);
  await button("停止生成"); await waitText("已停止"); assert.equal(studioMetrics.cancel, 1); assert.equal(studioMetrics.plans, 2);
  await go("reverse"); await measure("reverse"); await button("选择反推原图"); await select("实验角色", "薇拉"); await button("构建对照方案"); await waitText("方案已冻结"); const reverse = calls.filter((c) => c.command === "studio_prepare").at(-1); assert.equal(reverse.args.req.reverse.style_review_provider_id, ""); assert.deepEqual(reverse.args.req.characters, []); assert.equal(reverse.args.req.character, "薇拉");
  await go("review"); await button("＋ 添加本地图片"); await page.waitForSelector('input[aria-label="图 1 人工同风格组"]'); await button("＋ 添加本地图片"); await page.waitForSelector('input[aria-label="图 2 人工同风格组"]'); await button("＋ 添加本地图片"); await page.waitForSelector('input[aria-label="图 3 人工同风格组"]'); await button("＋ 添加本地图片"); await page.waitForSelector('input[aria-label="图 4 人工同风格组"]');
  await field('input[aria-label="图 1 人工同风格组"]', "A"); await field('input[aria-label="图 2 人工同风格组"]', "A");
  await select("比较视觉模型", "视觉模型-A"); await select("本地编码器", "megastyle"); await button("开始比较"); await waitText("准确率 100.0%"); await waitText("第 3 名"); await waitText("合成视觉模型：以图 1 为基准完成候选排序。"); assert.equal(studioMetrics.compare, 1); assert.equal(studioMetrics.local, 1); await measure("review");
  await page.waitForFunction(() => document.querySelector('.n-select[aria-label="比较历史"]'));
  await new Promise((r) => setTimeout(r, 350)); await page.reload({ waitUntil: "networkidle0" }); await waitText("准确率 100.0%");
  assert.equal(studioMetrics.compare, 1, "restoring comparison history must not invoke a model"); assert.equal(studioMetrics.local, 1);
  partialVisual = true; await button("开始比较"); await waitText("模型结果未能解析为排名"); await waitText("原文完整保留：结构化排名未完成");
  assert.equal(await page.$eval('.comparison-result:last-child .ranking-item', (e) => Boolean(e)).catch(() => false), false, "unparsed visual ranking must not invent winners");
  await select("比较历史", "4 图"); await waitText("已恢复历史的图片"); partialVisual = false;
  await go("draw", "studio-malformed"); await waitText("总实验室能力响应不完整"); assert(await page.$eval('.action-bar button:last-child', (e) => e.disabled));
  await go("draw", "studio-history-error"); await waitText("模拟历史读取失败");
  await go("draw", "studio-reference-error"); await waitText("合成资料读取失败");
  for (const [path, expected] of [["/reverse", "reverse"], ["/lab/draw", "draw"]]) { await page.goto(`${base}/#${path}`, { waitUntil: "networkidle0" }); await page.waitForFunction((mode) => location.hash === `#/studio?mode=${mode}`, {}, expected); assert(page.url().endsWith(`/studio?mode=${expected}`), page.url()); }
  // A stop click while start is awaiting its response is retained and cancels that same run.
  resetStudio(); await go("draw"); await button("构建对照方案"); await waitText("方案已冻结");
  holdStart = true; await button("出图"); while (!releaseStart) await new Promise((r) => setTimeout(r, 25));
  await button("停止生成"); await waitText("已记住停止请求"); holdStart = false; releaseStart(); releaseStart = undefined;
  await waitText("已停止"); assert.equal(studioMetrics.generations, 1); assert.equal(studioMetrics.cancel, 1);
  const cancelledRun = (await answerStudio("studio_list"))[0]; assert(cancelledRun.images.every((i) => i.status === "cancelled"));
  // Unknown start response keeps the same request ID and fulfils the prior stop after recovery.
  resetStudio(); await go("draw"); await button("构建对照方案"); await waitText("方案已冻结");
  scenario = "studio-start-lost"; holdStart = true; await button("出图"); while (!releaseStart) await new Promise((r) => setTimeout(r, 25));
  await button("停止生成"); holdStart = false; releaseStart(); releaseStart = undefined; await waitText("提交未确认");
  const unknown = calls.filter((c) => c.command === "studio_start").at(-1); scenario = ""; await button("确认并停止原请求"); await waitText("已停止");
  assert.equal(calls.filter((c) => c.command === "studio_start").at(-1).args.req.request_id, unknown.args.req.request_id); assert.equal(studioMetrics.generations, 1); assert.equal(studioMetrics.cancel, 1);
  // A restored active run survives a failed status read and can be reattached and stopped.
  resetStudio(); await go("draw"); await button("构建对照方案"); await waitText("方案已冻结"); await button("出图"); await waitText("正在生成：");
  await new Promise((r) => setTimeout(r, 350)); scenario = "studio-status-error"; await page.reload({ waitUntil: "networkidle0" }); await waitText("模拟状态读取失败");
  const beforeRecovery = studioMetrics.generations; scenario = ""; await button("重试读取记录"); await waitText("正在生成："); await button("停止生成"); await waitText("已停止"); assert.equal(studioMetrics.generations, beforeRecovery);
  // Failed workspace reads must not overwrite the saved rows with initial empty state.
  const saved = await answerStudio("studio_workspace_get"); await go("draw", "studio-workspace-error"); await waitText("模拟工作区读取失败");
  await field('textarea[aria-label="必须满足的内容"]', "不应覆盖原存档"); await new Promise((r) => setTimeout(r, 400)); assert.deepEqual(await answerStudio("studio_workspace_get"), saved);
  scenario = ""; await button("重试读取工作区"); await waitText("已停止");
  assert.deepEqual(errors, []); await writeFile(join(out, "measurements.json"), JSON.stringify({ measurements, metrics: studioMetrics }, null, 2)); console.log(`PASS studio: frozen matrix, explicit generation, idempotent retry, cancellation, restored workspace, all 3 modes, failed reads, redirects; ${measurements.length} layout/theme measurements. ${out}`);
} finally { await browser.close(); await server.close(); }
