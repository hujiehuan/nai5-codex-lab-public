// Actual Studio UI, synthetic offline reference metadata only. No model/image service calls.
// Exercises image+text guarantees independently from rendering generated pictures.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, studioMetrics } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";

const out = process.env.STUDIO_REFERENCE_OUT || "/tmp/nai5-studio-reference-images-probe";
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
async function go(mode = "draw", state = "") {
  scenario = state; resetStudio();
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
    await delay(250);
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
async function assertFrozen(offset, expected, groups = 1) {
  await finishedPlans(groups);
  const prepares = caseCalls("studio_prepare", offset), plans = caseCalls("studio_plan", offset);
  assert.equal(prepares.length, 1, "All model/skill combinations must share a single frozen input");
  assert.equal(plans.length, groups);
  const input = prepares[0].result;
  assert.equal(prepares[0].args.req.reference_mode, "all", "Full image+text mode must be the default");
  assert.equal(input.reference_count, expected);
  assert.equal(input.reference_image_count, expected, "Each frozen reference must have one primary image");
  for (const call of plans) {
    assert.equal(call.args.req.input_id, input.input_id);
    assert.equal(call.args.req.reference_mode, "all");
    assert.equal(call.result.settings.reference_image_count, expected, "Plan must preserve the actual transmitted image count");
    for (const plan of call.result.plans) assert.deepEqual(plan.reference_ids, input.reference_ids);
  }
  const notice = await page.$eval('[data-testid="studio-reference-notice"]', (element) => element.textContent);
  assert(notice.includes(String(expected)) && /图/.test(notice) && /文字|文本|提示词/.test(notice), `Frozen notice must report text and image counts: ${notice}`);
  const summary = await page.$eval('[data-testid="studio-reference-summary"]', (element) => element.textContent);
  assert(summary.includes(String(expected)) && /图/.test(summary), `Plan must display provided reference images: ${summary}`);
  assert.equal(caseCalls("studio_start", offset).length, 0, "Constructing a plan must not generate images");
  cases.push({ scenario: scenario || "default", references: expected, images: input.reference_image_count, groups, prepared: prepares.length, plans: plans.length });
}
try {
  await page.setViewport({ width: 1280, height: 1000 });
  let offset = await go();
  assert.equal(await page.$eval('[data-testid="studio-reference-mode"] select, select[data-testid="studio-reference-mode"]', (element) => element.value), "all");
  await page.$eval('textarea[aria-label="必须满足的内容"]', (element) => { element.value = "合成场景约束：同一角色在海边跳舞"; element.dispatchEvent(new Event("input", { bubbles: true })); });
  await button("＋ 添加 skill 组合"); await button("构建对照方案");
  await assertFrozen(offset, 188, 2); await measure("all-references-frozen");
  const firstRequests = caseCalls("studio_plan", offset).map((call) => call.args.req);
  assert.equal(firstRequests[0].requirements, firstRequests[1].requirements);
  const beforeRestore = calls.length;
  await page.reload({ waitUntil: "networkidle0" }); await finishedPlans(2);
  assert.equal(caseCalls("studio_prepare", beforeRestore).length, 0, "Restoring frozen plans must not refetch reference material");
  assert.equal(caseCalls("studio_plan", beforeRestore).length, 0, "Restoring history must not charge for another model call");

  offset = await go("draw", "studio-reference-images-large");
  await button("构建对照方案"); await assertFrozen(offset, 1367); await measure("1367-image-references");

  for (const mode of ["draw"]) {
    offset = await go(mode, "studio-no-reference-images");
    await page.waitForSelector('[data-testid="studio-reference-capability"]');
    const text = await page.$eval('[data-testid="studio-reference-capability"]', (element) => element.textContent);
    assert(/图/.test(text) && /更新|升级|重载/.test(text), `Old plugin must explain missing image capability: ${text}`);
    assert(await page.$eval('.action-bar button:last-child', (element) => element.disabled), "Unsupported image+text request must be blocked before planning");
    await page.keyboard.down("Control"); await page.keyboard.press("Enter"); await page.keyboard.up("Control"); await delay(350);
    assert.equal(caseCalls("studio_prepare", offset).length, 0, "Keyboard shortcut must honor the same capability gate");
    assert.equal(caseCalls("studio_plan", offset).length, 0);
    cases.push({ scenario, mode, blockedBeforePrepare: true });
    if (mode === "draw") await measure("old-plugin-image-capability");
  }
  offset = await go("reverse", "studio-no-reference-images");
  assert.equal(await page.$('[data-testid="studio-reference-capability"]'), null, "Image+text pack requirement must not disable ordinary reverse");
  await button("选择反推原图"); await button("构建对照方案"); await finishedPlans(1);
  assert.equal(caseCalls("studio_plan", offset).length, 1);
  cases.push({ scenario, mode: "reverse", plans: 1 });

  for (const state of ["studio-reference-image-missing", "studio-reference-image-count-mismatch"]) {
    offset = await go("draw", state); await button("构建对照方案");
    await page.waitForFunction(() => document.querySelector('.action-bar [role="alert"]')?.textContent.trim());
    const text = await page.$eval('.action-bar [role="alert"]', (element) => element.textContent);
    assert(/图|响应|冻结/.test(text), `Failure must explain invalid/missing image material: ${text}`);
    assert.equal(caseCalls("studio_prepare", offset).length, 1);
    assert.equal(caseCalls("studio_plan", offset).length, 0, "Incomplete image material must never become a text-only model request");
    assert.equal(await page.$('[data-testid="studio-reference-notice"]'), null, "Failed input must not claim success or zero references");
    cases.push({ scenario, error: text, plans: 0 });
    await measure(state);
  }

  offset = await go("draw", "studio-reference-image-limit"); await button("构建对照方案");
  await page.waitForFunction(() => document.querySelector('.matrix .error')?.textContent.trim());
  const error = await page.$eval('.matrix .error', (element) => element.textContent);
  assert(/图|上下文|容量|模型/.test(error), `Actual provider limit must be shown: ${error}`);
  await delay(600);
  assert.equal(caseCalls("studio_plan", offset).length, 1, "Provider limit must not silently retry with fewer images or text-only input");
  assert.equal(caseCalls("studio_prepare", offset).length, 1);
  assert.equal(caseCalls("studio_start", offset).length, 0);
  assert.equal(await page.$eval('[data-testid="studio-reference-mode"] select, select[data-testid="studio-reference-mode"]', (element) => element.value), "all");
  cases.push({ scenario, error, plans: 1, automaticFallback: false }); await measure("provider-image-capacity-error");

  assert.deepEqual(errors, [], "No browser execution exceptions");
  assert.deepEqual(blocked, [], "No external network should be attempted by the offline probe");
  assert.equal(calls.filter((call) => call.command === "studio_start").length, 0);
  console.log(`PASS Studio image+text references: ${cases.length} scenarios, ${measurements.length} layout/theme measurements; 188/1367 frozen image counts, capability gate, preserved reverse, missing-image fail-closed, explicit provider capacity failure, no silent text fallback. ${out}`);
} finally {
  await writeFile(join(out, "measurements.json"), JSON.stringify({ cases, measurements, errors, blocked, finalMetrics: studioMetrics,
    calls: calls.map((call) => ({ command: call.command, scenario: call.scenario, input_id: call.args?.req?.input_id, reference_mode: call.args?.req?.reference_mode, error: call.error,
      reference_count: call.result?.reference_count, reference_image_count: call.result?.reference_image_count ?? call.result?.settings?.reference_image_count })) }, null, 2));
  await browser.close(); await server.close();
}
