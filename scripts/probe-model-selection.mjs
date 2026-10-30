// Offline regression for the plugin-owned shortlist. Synthetic data; no bot/model calls.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, providers as catalog, studioMetrics } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";
const output = process.env.MODEL_SELECTION_OUT || "/tmp/nai5-model-selection-probe";
await mkdir(output, { recursive: true });
const server = await createServer({ mode: "mock", server: { port: 0, host: "127.0.0.1" } });
await server.listen(); const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), errors = [], calls = [], measurements = [];
let enabled = true, selected = [...catalog], failure = "", picked = 0, holdCapabilities = false, releaseCapabilities;
function metadata() { return { enabled, selected, available: catalog, effective: enabled ? selected.filter((id) => catalog.includes(id)) : catalog, missing: selected.filter((id) => !catalog.includes(id)) }; }
page.on("pageerror", (e) => errors.push(e.message));
await page.evaluateOnNewDocument(() => { window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, convertFileSrc: (p) => p, invoke: async () => 1 }; });
await page.setRequestInterception(true);
page.on("request", async (r) => {
  if (!r.url().endsWith("/__mock__/invoke")) return r.continue();
  const body = JSON.parse(r.postData()), { command, args } = body;
  if (!(command.startsWith("studio_") || command.startsWith("style_compare") || ["plugin_model_selection", "reverse_pick_local_image", "reverse_import_local_image", "style_models_list"].includes(command))) return r.continue();
  calls.push(body);
  try {
    let value;
    if (command === "plugin_model_selection") {
      if (failure === "401") throw { code: "plugin_rejected", message: "HTTP 401: synthetic authentication failure" };
      value = failure === "malformed" ? {} : metadata();
    } else if (command === "reverse_pick_local_image") value = `/synthetic/${++picked}.png`;
    else if (command === "reverse_import_local_image") value = `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text y="40">${picked}</text></svg>`)}`;
    else if (command === "style_models_list") value = [{ model: "megastyle-siglip-so400m", label: "MegaStyle", present: true }];
    else { value = await answerStudio(command, args); if (command === "studio_capabilities") { value.providers = metadata().effective; if (holdCapabilities) { holdCapabilities = false; await new Promise((resolve) => { releaseCapabilities = resolve; }); } } }
    await r.respond({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
  } catch (e) { await r.respond({ status: 400, contentType: "application/json", body: JSON.stringify(e) }); }
});
async function button(text) {
  for (const el of await page.$$("button")) if (await el.evaluate((e, t) => e.checkVisibility() && e.textContent.trim() === t, text)) { await el.evaluate((e) => e.scrollIntoView({ block: "center" })); await el.click(); return; }
  throw Error(`Missing button: ${text}`);
}
async function waitText(text) { await page.waitForFunction((s) => document.body.textContent.includes(s), {}, text); }
async function waitReady() { await page.waitForFunction(() => { const x = document.querySelector('.n-select[aria-label="实验模型"]'); return x && !x.querySelector('.n-base-selection--disabled'); }); }
async function options(label) {
  const selector = `.n-select[aria-label="${label}"]`; await page.$eval(selector, (e) => e.scrollIntoView({ block: "center" })); await page.click(selector);
  await new Promise((r) => setTimeout(r, 150));
  const values = await page.$$eval('.n-base-select-option', (elements) => elements.filter((e) => e.checkVisibility()).map((e) => e.textContent.trim()));
  await page.keyboard.press("Escape"); await new Promise((r) => setTimeout(r, 350)); return [...new Set(values)];
}
async function choose(label, value) {
  const selector = `.n-select[aria-label="${label}"]`; await page.$eval(selector, (e) => e.scrollIntoView({ block: "center" })); await page.click(selector); await page.waitForSelector('.n-base-select-option');
  for (const el of await page.$$('.n-base-select-option')) if ((await el.evaluate((e) => e.checkVisibility() ? e.textContent.trim() : "")) === value) { await el.click(); break; }
  await page.keyboard.press("Escape"); await new Promise((r) => setTimeout(r, 350));
}
async function refresh() { await button("刷新模型名单"); await page.waitForFunction(() => !document.querySelector('button[disabled]')?.textContent.includes("刷新模型名单")); await new Promise((r) => setTimeout(r, 200)); }
async function measure(name) {
  for (const width of [1280, 390]) for (const theme of ["light", "dark"]) {
    await page.setViewport({ width, height: 1100 }); await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
    await page.waitForFunction((t) => document.documentElement.dataset.theme === t, {}, theme); await new Promise((r) => setTimeout(r, 1000));
    const result = await page.evaluate(() => {
      const visible = (e) => { const b = e.getBoundingClientRect(); return e.checkVisibility() && b.width > 0 && b.height > 0 && getComputedStyle(e).visibility !== "hidden"; };
      const small = [...document.querySelectorAll('.workspace-page button')].filter((e) => visible(e) && !e.disabled).map((e) => ({ text: e.textContent.trim(), w: e.getBoundingClientRect().width, h: e.getBoundingClientRect().height })).filter((e) => e.w < 24 || e.h < 24);
      const color = (s) => (s.match(/[\d.]+/g) || []).map(Number), blend = (a, b) => [0, 1, 2].map((i) => (a[i] || 0) * (a[3] ?? 1) + b[i] * (1 - (a[3] ?? 1)));
      const lum = (c) => c.slice(0, 3).map((v) => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
      const lowContrast = [], ratios = [], walker = document.createTreeWalker(document.querySelector('.workspace-page'), NodeFilter.SHOW_TEXT); let node;
      while ((node = walker.nextNode())) {
        const e = node.parentElement; if (!e || !node.textContent.trim() || !visible(e) || !e.closest('.status-banner,.field-row,.model-selection-note,.error,.n-select') || e.closest('[disabled],.n-button--disabled,.n-select--disabled') || ['SCRIPT','STYLE'].includes(e.tagName)) continue;
        const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p);
        if (parents.some((p) => +getComputedStyle(p).opacity < .99)) continue;
        let bg = [255,255,255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg);
        const style = getComputedStyle(e), a = lum(blend(color(style.color), bg)), b = lum(bg), ratio = (Math.max(a,b)+.05)/(Math.min(a,b)+.05), large = parseFloat(style.fontSize)>=24 || (parseFloat(style.fontSize)>=18.66 && parseInt(style.fontWeight)>=700);
        ratios.push(ratio); if (ratio < (large ? 3 : 4.5)-.01) lowContrast.push({text:node.textContent.trim().slice(0,50),ratio,fg:style.color,bg,cls:e.className});
      }
      const alignment = ['.status-banner','.field-control'].map((selector) => { const xs = [...document.querySelectorAll(selector)].filter(visible).map((e) => e.getBoundingClientRect().left); return {selector,count:xs.length,spread:xs.length ? Math.max(...xs)-Math.min(...xs) : 0}; });
      return { overflow: document.documentElement.scrollWidth-innerWidth, small, lowContrast, contrastNodes:ratios.length, minContrast:ratios.length ? Math.min(...ratios) : null, alignment };
    });
    assert.equal(result.overflow, 0, JSON.stringify(result)); assert.deepEqual(result.small, [], JSON.stringify(result)); assert.deepEqual(result.lowContrast, [], JSON.stringify({name,width,theme,...result})); assert(result.contrastNodes > 0);
    for (const a of result.alignment) assert(a.spread <= 1, JSON.stringify(a)); measurements.push({ name, width, theme, ...result });
    await page.screenshot({ path: join(output, `${name}-${width}-${theme}.png`), fullPage: true });
  }
  await page.setViewport({ width: 1280, height: 1100 });
}
try {
  resetStudio(); await page.setViewport({ width: 1280, height: 1100 });
  await page.goto(`${base}/#/studio?mode=draw`, { waitUntil: "networkidle0" }); await waitReady();
  await button("构建对照方案"); await waitText("方案已冻结"); assert.equal(studioMetrics.plans, 1);
  selected = [catalog[0]]; holdCapabilities = true;
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  while (!releaseCapabilities) await new Promise((r) => setTimeout(r, 20));
  selected = [catalog[1]]; await refresh(); await waitReady();
  releaseCapabilities(); releaseCapabilities = undefined; await new Promise((r) => setTimeout(r, 200));
  await waitText("这些模型已不在插件可选名单"); await waitReady();
  assert.deepEqual(await options("实验模型"), [catalog[1]]); assert.deepEqual(await options("评分模型"), [catalog[1]]);
  assert(await page.$eval('.n-select[aria-label="实验模型"]', (e, id) => e.textContent.includes(id), catalog[0]), "saved missing choice must stay visible");
  assert(await page.$$eval('button', (es) => es.find((e) => e.textContent.trim() === "构建对照方案").disabled));
  await measure("invalid-saved-model");
  await button("出图"); await waitText("已完成"); assert.equal(studioMetrics.generations, 1, "frozen plan can still run"); assert.equal(studioMetrics.plans, 1, "no silent replacement/replan");
  selected = [catalog[0]]; await page.evaluate(() => { location.hash = "#/studio?mode=reverse"; }); await waitReady(); await waitText("这些模型已不在插件可选名单");
  assert.deepEqual(await options("实验模型"), [catalog[0]]); assert.deepEqual(await options("评分模型"), [catalog[0]]);
  await page.evaluate(() => { const d = [...document.querySelectorAll('details')].find((e) => e.querySelector('summary')?.textContent === '反推模板与画风复审'); if (d) d.open = true; const e = [...document.querySelectorAll('select')].find((e) => [...e.options].some((o) => o.value === 'precise')); if (e) { e.value = 'precise'; e.dispatchEvent(new Event('change', { bubbles: true })); } });
  await page.waitForSelector('.n-select[aria-label="画风复审模型"]');
  assert.deepEqual(await options("画风复审模型"), [catalog[0]]);
  await page.evaluate(() => { location.hash = "#/studio?mode=review"; }); await waitText("比较样本"); await new Promise((r) => setTimeout(r, 250));
  assert.deepEqual(await options("比较视觉模型"), [catalog[0]]);
  await choose("比较视觉模型", catalog[0]); await button("＋ 添加本地图片"); await page.waitForSelector('input[aria-label="图 1 人工同风格组"]'); await button("＋ 添加本地图片"); await page.waitForSelector('input[aria-label="图 2 人工同风格组"]');
  selected = ["已删除的模型"]; await refresh(); await waitText("名单中的模型均已失效"); await waitText("这些模型已不在插件可选名单");
  assert.deepEqual(await options("比较视觉模型"), []); assert.equal(studioMetrics.compare, 0);
  assert(await page.$$eval('button', (es) => es.find((e) => e.textContent.trim() === "开始比较").disabled));
  await measure("all-missing");
  for (const value of ["401", "malformed"]) { failure = value; await refresh(); await waitText(value === "401" ? "HTTP 401" : "模型名单格式错误"); assert(await page.$eval('.n-select[aria-label="比较视觉模型"]', (e) => !!e.querySelector('.n-base-selection--disabled'))); }
  failure = ""; enabled = false; await refresh(); await waitText("插件未启用模型筛选"); assert.deepEqual(await options("比较视觉模型"), catalog);
  const reads = calls.filter((c) => c.command === "plugin_model_selection");
  assert(reads.length > 0, "shortlist is read through plugin_model_selection");
  assert(reads.every((c) => !c.args || !Object.keys(c.args).length), "Mac never manages shortlist: the read takes no arguments");
  assert.deepEqual(errors, []); await writeFile(join(output, "measurements.json"), JSON.stringify({ measurements, metrics: studioMetrics }, null, 2));
  console.log(`PASS: shortlist across prompt/score/rerank/review; enter/focus/manual refresh; stale IDs retained; frozen run allowed; all-missing/401/malformed fail closed; plugin-disabled filter falls back to the full catalog; read-only argument-free command. ${measurements.length} viewport/theme checks. ${output}`);
} finally { await browser.close(); await server.close(); }
