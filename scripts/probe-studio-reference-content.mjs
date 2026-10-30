// Actual Studio UI: four explicit reference modes, synthetic data and no model/image service calls.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, studioMetrics } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";

const out = process.env.STUDIO_REFERENCE_CONTENT_OUT || "/tmp/nai5-studio-reference-content-probe";
await mkdir(out, { recursive: true });
const server = await createServer({ mode: "mock", cacheDir: join(out, "vite-cache"), optimizeDeps: { include: ["@tauri-apps/plugin-dialog"] }, server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), calls = [], errors = [], blocked = [], measurements = [], cases = [];
let scenario = "";
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
    const result = body.command === "reverse_pick_local_image"
      ? "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='100'%20height='140'%3E%3Crect%20width='100'%20height='140'%20fill='plum'/%3E%3C/svg%3E"
      : await answerStudio(body.command, body.args, scenario);
    call.result = result;
    await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(result) });
  } catch (error) {
    call.error = error;
    await request.respond({ status: 400, contentType: "application/json", body: JSON.stringify(error) });
  }
});
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const caseCalls = (command, offset) => calls.slice(offset).filter((call) => call.command === command);
async function button(text) {
  for (const item of await page.$$("button")) if (await item.evaluate((element, target) => element.textContent.trim() === target && element.getBoundingClientRect().width > 0, text)) {
    await item.evaluate((element) => element.scrollIntoView({ block: "center" }));
    await item.click(); return;
  }
  throw Error(`Visible button missing: ${text}`);
}
async function go(mode = "draw", state = "", workspace = null) {
  await page.goto("about:blank"); await delay(300);
  scenario = state; resetStudio();
  if (workspace) await answerStudio("studio_workspace_save", { state: workspace });
  const offset = calls.length;
  await page.goto(`${base}/?reference-probe=${Date.now()}#/studio?mode=${mode}`, { waitUntil: "networkidle0" });
  await page.waitForFunction(() => document.querySelector('[data-testid="studio"]') && !document.body.textContent.includes("正在读取插件实验能力"));
  return offset;
}
async function finishedPlans(count) {
  await page.waitForFunction((count) => document.querySelectorAll(".matrix tbody tr").length === count && document.querySelectorAll(".matrix tbody tr .primary").length === count, {}, count);
  await page.waitForFunction(() => !document.body.textContent.includes("构思中…"));
}
async function measure(name) {
  for (const width of [1280, 390]) for (const theme of ["light", "dark"]) {
    await page.setViewport({ width, height: 1000 });
    await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
    await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, {}, theme);
    await delay(650);
    const value = await page.evaluate(() => {
      const regions = [...document.querySelectorAll('[data-testid^="studio-reference-"], .action-bar [role="alert"], .matrix .error')];
      const visible = (element) => element.checkVisibility() && element.getBoundingClientRect().width > 0 && getComputedStyle(element).visibility !== "hidden";
      const targets = [...new Set(regions.flatMap((region) => [region, ...region.querySelectorAll('button,input,select,summary')]))].filter((e) => e.matches('button,input,select,summary') && visible(e) && !e.disabled).map((e) => {
        const r = e.getBoundingClientRect(); return { text: e.getAttribute("aria-label") || e.textContent.trim().slice(0, 60), width: r.width, height: r.height };
      });
      const color = (text) => (text.match(/[\d.]+/g) || []).map(Number);
      const blend = (a, b) => [0, 1, 2].map((i) => (a[i] || 0) * (a[3] ?? 1) + b[i] * (1 - (a[3] ?? 1)));
      const lum = (color) => color.slice(0, 3).map((v) => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
      const textContrast = [], visited = new Set();
      for (const region of regions) {
        const walker = document.createTreeWalker(region, NodeFilter.SHOW_TEXT); let node;
        while ((node = walker.nextNode())) {
          if (visited.has(node)) continue; visited.add(node);
          const e = node.parentElement;
          if (!e || !node.textContent.trim() || !visible(e) || ["STYLE", "SCRIPT", "OPTION"].includes(e.tagName) || e.closest('[disabled],.n-button--disabled')) continue;
          const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p);
          if (parents.some((p) => +getComputedStyle(p).opacity < .99)) continue;
          let bg = [255, 255, 255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg);
          const style = getComputedStyle(e), a = lum(blend(color(style.color), bg)), b = lum(bg);
          const large = parseFloat(style.fontSize) >= 24 || (parseFloat(style.fontSize) >= 18.66 && parseInt(style.fontWeight) >= 700);
          textContrast.push({ text: node.textContent.trim().slice(0, 160), ratio: (Math.max(a, b) + .05) / (Math.min(a, b) + .05), minimum: large ? 3 : 4.5 });
        }
      }
      return { overflow: document.documentElement.scrollWidth - innerWidth, regionCount: regions.length, targets, textContrast };
    });
    measurements.push({ name, width, theme, ...value });
    assert(value.regionCount > 0 && value.textContrast.length > 0, `Changed reference region must be measured: ${name}`);
    assert.equal(value.overflow, 0, `Horizontal overflow: ${JSON.stringify(value)}`);
    assert.deepEqual(value.targets.filter((target) => target.width < 24 || target.height < 24), [], `Small reference control: ${JSON.stringify(value.targets)}`);
    assert.deepEqual(value.textContrast.filter((text) => text.ratio + .01 < text.minimum), [], `Low reference text contrast: ${JSON.stringify(value.textContrast)}`);
    await page.evaluate(() => document.querySelector("main.content")?.scrollTo(0, 0));
    await page.screenshot({ path: join(out, `${name}-${width}-${theme}.png`), fullPage: true });
  }
}

const choiceSelector = 'select[data-testid="studio-reference-mode"]';
async function choose(choice) {
  await page.select(choiceSelector, choice);
  assert.equal(await page.$eval(choiceSelector, (e) => e.value), choice);
}
async function assertPlan(offset, { content = "image_text", sampling = "all", count = 188, groups = 1 } = {}) {
  await finishedPlans(groups);
  const prepares = caseCalls("studio_prepare", offset), plans = caseCalls("studio_plan", offset);
  assert.equal(prepares.length, 1); assert.equal(plans.length, groups);
  assert.equal(prepares[0].args.req.reference_content, content);
  assert.equal(prepares[0].args.req.reference_mode, sampling);
  assert.equal(prepares[0].result.reference_content, content);
  assert.equal(prepares[0].result.reference_count, count);
  assert.equal(prepares[0].result.reference_image_count, content === "text" ? 0 : count);
  for (const call of plans) {
    assert.equal(call.args.req.input_id, prepares[0].result.input_id);
    assert.equal(call.args.req.reference_content, content);
    assert.equal(call.args.req.reference_mode, sampling);
    assert.equal(call.result.settings.reference_content, content);
    assert.equal(call.result.settings.reference_image_count, content === "text" ? 0 : count);
    assert.equal(call.result.settings.reference_images.length, content === "text" ? 0 : count);
    assert.deepEqual(call.result.plans[0].reference_ids, prepares[0].result.reference_ids);
  }
  const notice = await page.$eval('[data-testid="studio-reference-notice"]', (e) => e.textContent);
  const summary = await page.$eval('[data-testid="studio-reference-summary"]', (e) => e.textContent);
  assert(notice.includes(String(count)) && summary.includes(String(count)));
  if (content === "text") { assert(notice.includes("0 张参考图")); assert(summary.includes("未附参考图（0 张）")); assert(!summary.includes("所有图片")); }
  else { assert(notice.includes(`${count} 张主参考图`)); assert(summary.includes(`${count} 张主参考图`)); }
  assert.equal(caseCalls("studio_start", offset).length, 0);
}
try {
  await page.setViewport({ width: 1280, height: 1000 });
  for (const mode of ["draw"]) for (const [choice, content, sampling] of [
    ["all", "image_text", "all"], ["sample", "image_text", "sample"],
    ["text_all", "text", "all"], ["text_sample", "text", "sample"],
  ]) {
    const offset = await go(mode);
    assert.equal(await page.$eval(choiceSelector, (e) => e.value), "all", "Default must remain full image+text");
    assert.deepEqual(await page.$$eval(`${choiceSelector} option`, (options) => options.map((e) => e.textContent)),
      ["全部图文一起传", "抽样图文参考", "全部文一起传", "抽样文参考"]);
    await choose(choice);
    if (sampling === "sample") await page.$eval('[data-testid="studio-reference-sample"] input', (e) => { e.value = "3"; e.dispatchEvent(new Event("input", { bubbles: true })); });
    else assert.equal(await page.$('[data-testid="studio-reference-sample"]'), null);
    if (content === "text") assert((await page.$eval('[data-testid="studio-reference-description"]', (e) => e.textContent)).includes("不要求模型具备视觉能力"));
    await button("构建对照方案");
    await assertPlan(offset, { content, sampling, count: sampling === "all" ? 188 : 3 });
    if (mode === "draw") await measure(choice);
    await delay(400);
    const beforeReload = calls.length;
    await page.reload({ waitUntil: "networkidle0" }); await finishedPlans(1);
    assert.equal(await page.$eval(choiceSelector, (e) => e.value), choice, "Saved selection must survive reload");
    assert.equal(caseCalls("studio_prepare", beforeReload).length, 0); assert.equal(caseCalls("studio_plan", beforeReload).length, 0);
    cases.push({ mode, choice, preservedAfterReload: true });
  }
  // Native select remains keyboard accessible, with no hidden second mode setting.
  await go(); await page.focus(choiceSelector); await page.keyboard.press("Home");
  await page.keyboard.press("ArrowDown"); await page.keyboard.press("ArrowDown");
  assert.equal(await page.$eval(choiceSelector, (e) => e.value), "text_all");
  await page.keyboard.down("Control"); await page.keyboard.press("Enter"); await page.keyboard.up("Control");
  await finishedPlans(1); cases.push({ scenario: "keyboard-select-and-compose" });

  for (const choice of ["text_all", "text_sample"]) {
    const offset = await go("draw", "studio-reference-image-missing"); await choose(choice); await button("构建对照方案");
    await assertPlan(offset, { content: "text", sampling: choice === "text_all" ? "all" : "sample", count: choice === "text_all" ? 188 : 4 });
    cases.push({ scenario, choice, missingImageIgnored: true });
  }
  let offset = await go("draw", "studio-reference-image-missing"); await button("构建对照方案");
  await page.waitForFunction(() => document.querySelector('.action-bar [role="alert"]')?.textContent.includes("缺少主参考图"));
  assert.equal(caseCalls("studio_plan", offset).length, 0); cases.push({ scenario, imageTextBlocked: true });

  for (const mode of ["draw"]) {
    offset = await go(mode, "studio-no-reference-text"); await choose("text_all");
    await page.waitForSelector('[data-testid="studio-reference-capability"]');
    assert((await page.$eval('[data-testid="studio-reference-capability"]', (e) => e.textContent)).includes("不支持显式纯文字"));
    assert(await page.$eval('.action-bar button:last-child', (e) => e.disabled));
    await page.keyboard.down("Control"); await page.keyboard.press("Enter"); await page.keyboard.up("Control"); await delay(300);
    assert.equal(caseCalls("studio_prepare", offset).length, 0); assert.equal(caseCalls("studio_plan", offset).length, 0);
    if (mode === "draw") await measure("text-capability-missing");
    cases.push({ scenario, mode, blockedBeforePrepare: true });
  }
  offset = await go("draw", "studio-no-reference-images"); await choose("text_all"); await button("构建对照方案");
  await assertPlan(offset, { content: "text" }); cases.push({ scenario, textWorksWithoutImageCapability: true });

  for (const state of ["studio-reference-text-count-mismatch", "studio-reference-text-plan-mismatch"]) {
    offset = await go("draw", state); await choose("text_all"); await button("构建对照方案");
    const selector = state.includes("plan") ? ".matrix .error" : '.action-bar [role="alert"]';
    await page.waitForFunction((selector) => !!document.querySelector(selector)?.textContent.trim(), {}, selector);
    assert.equal(caseCalls("studio_plan", offset).length, state.includes("plan") ? 1 : 0);
    assert.equal(caseCalls("studio_start", offset).length, 0);
    assert.equal(await page.$('[data-testid="studio-reference-summary"]'), null);
    cases.push({ scenario, refusedMismatchedResponse: true });
  }
  offset = await go("draw", "studio-plan-error"); await choose("text_sample"); await button("构建对照方案");
  await page.waitForSelector(".matrix .error");
  const frozen = structuredClone(caseCalls("studio_plan", offset)[0].args.req);
  await choose("all"); scenario = ""; await button("重试构思（API）"); await finishedPlans(1);
  assert.equal(caseCalls("studio_prepare", offset).length, 1);
  assert.deepEqual(caseCalls("studio_plan", offset)[1].args.req, frozen, "Retry must preserve old content, sampling, skill and input ID");
  assert((await page.$eval('[data-testid="studio-reference-summary"]', (e) => e.textContent)).includes("未附参考图"));
  cases.push({ scenario: "text-retry-after-form-switch", frozenRequestPreserved: true });

  offset = await go("draw", "studio-legacy-reference-images"); await button("构建对照方案"); await finishedPlans(1);
  assert.equal(caseCalls("studio_plan", offset)[0].result.settings.reference_content, undefined);
  assert((await page.$eval('[data-testid="studio-reference-summary"]', (e) => e.textContent)).includes("188 张主参考图"));
  cases.push({ scenario, imageManifestStillVerified: true });

  await go(); await choose("text_all"); await button("构建对照方案"); await finishedPlans(1);
  await delay(400); const legacyRestore = calls.length; scenario = "studio-legacy-text-history";
  await page.reload({ waitUntil: "networkidle0" }); await finishedPlans(1);
  assert((await page.$eval('[data-testid="studio-reference-summary"]', (e) => e.textContent)).includes("旧方案：仅文字参考"));
  assert.equal(caseCalls("studio_prepare", legacyRestore).length, 0); assert.equal(caseCalls("studio_plan", legacyRestore).length, 0);
  cases.push({ scenario, readableWithoutNewPlanning: true });

  offset = await go("draw", "", { version: 1, forms: { draw: { reference_mode: "sample" } } });
  assert.equal(await page.$eval(choiceSelector, (e) => e.value), "sample");
  await button("构建对照方案"); await assertPlan(offset, { sampling: "sample", count: 4 });
  cases.push({ scenario: "legacy-workspace", previousSamplingPreserved: true, defaultContent: "image_text" });

  offset = await go("reverse", "studio-no-reference-text");
  assert.equal(await page.$(choiceSelector), null); assert.equal(await page.$('[data-testid="studio-reference-capability"]'), null);
  await button("选择反推原图"); await button("构建对照方案"); await finishedPlans(1);
  assert.equal(caseCalls("studio_plan", offset)[0].args.req.reference_content, undefined);
  cases.push({ scenario: "reverse-independent" });

  assert.deepEqual(errors, []); assert.deepEqual(blocked, []);
  assert.equal(calls.filter((call) => call.command === "studio_start").length, 0);
  console.log(`PASS Studio reference content: ${cases.length} scenarios, ${measurements.length} layout/theme measurements. ${out}`);
} catch (error) {
  await writeFile(join(out, "failure.json"), JSON.stringify({ error: String(error), cases, measurements, calls: calls.slice(-20) }, null, 2));
  await page.screenshot({ path: join(out, "failure.png"), fullPage: true }).catch(() => {});
  throw error;
} finally {
  await writeFile(join(out, "measurements.json"), JSON.stringify({ cases, measurements, errors, blocked,
    calls: calls.map((call) => ({ command: call.command, scenario: call.scenario, req: call.args?.req, error: call.error,
      reference_count: call.result?.reference_count, reference_image_count: call.result?.reference_image_count ?? call.result?.settings?.reference_image_count })) }, null, 2));
  await browser.close(); await server.close();
}
