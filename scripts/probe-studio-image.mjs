// Native-image preview UI regression. Synthetic pictures and an offline mock only.
// STUDIO_IMAGE_BASELINE=1 node scripts/probe-studio-image.mjs captures v0.5.9's old modal.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, studioMetrics } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";

const baseline = process.env.STUDIO_IMAGE_BASELINE === "1";
const out = process.env.STUDIO_IMAGE_OUT || "/tmp/nai5-studio-image-probe";
await mkdir(out, { recursive: true });
const oldComponent = baseline ? execFileSync("git", ["show", "v0.5.9:src/components/studio/StudioResults.vue"], { encoding: "utf8" }) : null;
const componentPath = resolve("src/components/studio/StudioResults.vue");
const server = await createServer({ mode: "mock", cacheDir: join(out, "vite-cache"),
  plugins: baseline ? [{ name: "original-modal-baseline", enforce: "pre", load(id) { if (id === componentPath) return oldComponent; } }] : [],
  optimizeDeps: { include: ["@tauri-apps/plugin-dialog"] }, server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), calls = [], errors = [], measurements = [];
let imageFailure = false, brokenAsset = false, assetVersion = 0, holdOriginal = false, releaseOriginal = null;
const assets = new Map();
page.on("pageerror", (error) => errors.push(error.message));
await page.evaluateOnNewDocument(() => { window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, convertFileSrc: (path) => path, invoke: async () => 1 }; });
await page.setRequestInterception(true);
page.on("request", async (request) => {
  const url = new URL(request.url());
  if (url.pathname.startsWith("/__studio_image_fixture__/")) {
    const asset = assets.get(url.pathname);
    if (holdOriginal && url.pathname.endsWith("original.png")) await new Promise((resolve) => { releaseOriginal = resolve; });
    const fail = brokenAsset && url.pathname.endsWith("original.png");
    return request.respond({ status: fail || !asset ? 404 : 200, contentType: asset?.mime || "text/plain", headers: { "cache-control": "no-store" }, body: fail || !asset ? "synthetic image unavailable" : asset.bytes });
  }
  if (!url.pathname.endsWith("/__mock__/invoke")) return request.continue();
  const body = JSON.parse(request.postData());
  if (!body.command.startsWith("studio_")) return request.continue();
  calls.push(body);
  try {
    let result;
    if (body.command === "studio_image") {
      if (imageFailure) throw { code: "network", message: "合成取图失败，可重试" };
      const format = body.args.format;
      result = { path: `${base}/__studio_image_fixture__/${format === "jpeg" ? "derived.jpg" : "original.png"}?case=${assetVersion}`, original_path: `${base}/__studio_image_fixture__/original.png?case=${assetVersion}`, format,
        original_bytes: assets.get("/__studio_image_fixture__/original.png").bytes.length,
        encoded_bytes: assets.get(`/__studio_image_fixture__/${format === "jpeg" ? "derived.jpg" : "original.png"}`).bytes.length,
        quality: format === "jpeg" ? body.args.quality : null };
    } else result = await answerStudio(body.command, body.args);
    body.result = result;
    await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(result) });
  } catch (error) {
    body.error = error;
    await request.respond({ status: 400, contentType: "application/json", body: JSON.stringify(error) });
  }
});
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function button(text, scope = "button") {
  for (const item of await page.$$(scope)) {
    if (await item.evaluate((element, target) => element.textContent.trim() === target && element.getBoundingClientRect().width > 0, text)) {
      await item.evaluate((element) => element.scrollIntoView({ block: "center" }));
      await item.click(); return;
    }
  }
  throw Error(`Visible button missing: ${text}`);
}
async function seed() {
  resetStudio();
  const req = { mode: "draw", character: "薇拉", characters: [], requirements: "合成测试场景", count: 3, orientation: "portrait", reference_mode: "all", reference_count: 4, reference_decisions: "include", scope: [], skill_ids: ["builtin-guide"], provider_id: "示例/视觉模型-A", score_provider_ids: [], jpeg: { quality: 88 } };
  const input = await answerStudio("studio_prepare", { req });
  const plan = await answerStudio("studio_plan", { req: { ...req, input_id: input.input_id } });
  const run = await answerStudio("studio_start", { req: { plan_id: plan.plan_id, request_id: "synthetic-preview-only", seed: 42 } });
  for (let i = 0; i < 3; i++) await answerStudio("studio_status", { run_id: run.run_id });
  return { req: { ...req, input_id: input.input_id }, plan, run };
}
async function go(zoom = 1, history = true) {
  assetVersion++;
  await page.goto(`${base}/?image-probe=${Date.now()}#/studio?mode=draw`, { waitUntil: "networkidle0" });
  await page.evaluate((zoom) => { localStorage.setItem("codexlab.zoom", String(zoom)); }, zoom);
  await page.reload({ waitUntil: "networkidle0" });
  await page.waitForSelector(".history-list button");
  if (history) {
    await page.$eval(".history-list button", (button) => button.scrollIntoView({ block: "center" }));
    await page.click(".history-list button");
  }
}
async function openFirst(keyboard = false) {
  await page.waitForFunction(() => document.querySelector(".image-open img")?.naturalWidth > 0);
  await page.$eval(".image-open", (element) => { document.querySelectorAll('[data-preview-trigger]').forEach((e) => delete e.dataset.previewTrigger); element.dataset.previewTrigger = "true"; element.scrollIntoView({ block: "center" }); element.focus(); });
  if (keyboard) await page.keyboard.press("Enter");
  else await page.click(".image-open");
  await page.waitForSelector('[role="dialog"][aria-label="实验原图"]');
  await delay(400);
}
async function measure(name) {
  const value = await page.evaluate(() => {
    const dialog = document.querySelector('[role="dialog"][aria-label="实验原图"]');
    const img = dialog?.querySelector("img");
    const box = (element) => { if (!element) return null; const r = element.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height, right: r.right, bottom: r.bottom }; };
    const ancestors = [];
    for (let parent = dialog; parent && ancestors.length < 5; parent = parent.parentElement) ancestors.push({ tag: parent.tagName, className: parent.className, rect: box(parent), position: getComputedStyle(parent).position, transform: getComputedStyle(parent).transform, overflow: getComputedStyle(parent).overflow });
    const visible = (e) => { const b = e.getBoundingClientRect(); return e.checkVisibility() && b.width > 0 && b.height > 0 && getComputedStyle(e).visibility !== "hidden"; };
    const buttons = [...dialog.querySelectorAll("button")].filter(visible).filter((e) => !e.disabled).map((e) => ({ text: e.textContent.trim(), width: e.getBoundingClientRect().width, height: e.getBoundingClientRect().height }));
    const color = (s) => (s.match(/[\d.]+/g) || []).map(Number);
    const blend = (a, b) => [0, 1, 2].map((i) => (a[i] || 0) * (a[3] ?? 1) + b[i] * (1 - (a[3] ?? 1)));
    const lum = (c) => c.slice(0, 3).map((v) => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
    const textContrast = [], walker = document.createTreeWalker(dialog, NodeFilter.SHOW_TEXT); let node;
    while ((node = walker.nextNode())) {
      const e = node.parentElement;
      if (!e || !node.textContent.trim() || !visible(e) || ["STYLE", "SCRIPT"].includes(e.tagName) || e.closest('[disabled],.n-button--disabled')) continue;
      const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p);
      if (!parents.some((p) => +getComputedStyle(p).opacity < .99)) {
        let bg = [255, 255, 255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg);
        const style = getComputedStyle(e), a = lum(blend(color(style.color), bg)), b = lum(bg);
        const large = parseFloat(style.fontSize) >= 24 || (parseFloat(style.fontSize) >= 18.66 && parseInt(style.fontWeight) >= 700);
        textContrast.push({ text: node.textContent.trim(), ratio: (Math.max(a, b) + .05) / (Math.min(a, b) + .05), minimum: large ? 3 : 4.5 });
      }
    }
    const r = img?.getBoundingClientRect();
    const center = r ? document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2) : null;
    return { viewport: { width: innerWidth, height: innerHeight }, zoom: getComputedStyle(document.documentElement).zoom,
      dialog: box(dialog), image: box(img), naturalWidth: img?.naturalWidth ?? 0, naturalHeight: img?.naturalHeight ?? 0, src: img?.currentSrc ?? "",
      buttons, textContrast, imageAtCenter: !!img && (center === img || img.contains(center)), visibility: img ? getComputedStyle(img).visibility : null,
      objectFit: img ? getComputedStyle(img).objectFit : null, loading: dialog?.querySelector('[aria-busy="true"]') !== null, ancestors,
      overflow: document.documentElement.scrollWidth - innerWidth, mainScroll: document.querySelector("main.content")?.scrollTop ?? 0,
      focusInside: !!dialog?.contains(document.activeElement), activeTag: document.activeElement?.tagName, activeMarkup: document.activeElement?.outerHTML.slice(0, 300), documentFocused: document.hasFocus(), focus: document.activeElement?.getAttribute("aria-label") || document.activeElement?.textContent?.trim()?.slice(0, 40) };
  });
  measurements.push({ name, ...value });
  await writeFile(join(out, baseline ? "before.json" : "measurements-latest.json"), JSON.stringify({ measurements }, null, 2));
  await page.screenshot({ path: join(out, `${name}.png`), fullPage: false });
  return value;
}
function assertAccessible(value) {
  assert.deepEqual(value.buttons.filter((button) => button.width < 24 || button.height < 24), [], `Small modal target: ${JSON.stringify(value.buttons)}`);
  assert(value.textContrast.some((text) => text.text === "关闭原图"), "Measure actual modal text contrast");
  assert.deepEqual(value.textContrast.filter((text) => text.ratio + .01 < text.minimum), [], `Low modal text contrast: ${JSON.stringify(value.textContrast)}`);
}
function assertVisible(value) {
  assertAccessible(value);
  assert(value.naturalWidth > 0 && value.naturalHeight > 0, JSON.stringify(value));
  assert(value.image.width > 150 && value.image.height > 250, `Preview collapsed: ${JSON.stringify(value)}`);
  assert(value.image.x >= -1 && value.image.y >= -1 && value.image.right <= value.viewport.width + 1 && value.image.bottom <= value.viewport.height + 1, `Image clipped: ${JSON.stringify(value)}`);
  assert(value.imageAtCenter, `Image is hidden under another element: ${JSON.stringify(value)}`);
  assert.equal(value.visibility, "visible", "Loaded image must be visible");
  assert.equal(value.objectFit, "contain", "Preview must show the whole original image");
  assert.equal(value.loading, false, "Finished image must leave its loading state");
  assert.equal(value.overflow, 0, JSON.stringify(value));
  assert(value.focusInside, "Focus must enter the modal");
}
async function closed() {
  await page.waitForFunction(() => !document.querySelector('[role="dialog"][aria-label="实验原图"]'));
  await page.waitForFunction(() => document.activeElement?.dataset.previewTrigger === "true");
}
try {
  const images = await page.evaluate(() => {
    const c = document.createElement("canvas"); c.width = 832; c.height = 1216;
    const ctx = c.getContext("2d"); ctx.fillStyle = "#ded1ed"; ctx.fillRect(0, 0, c.width, c.height);
    ctx.strokeStyle = "#305b73"; ctx.lineWidth = 14; ctx.strokeRect(14, 14, c.width - 28, c.height - 28);
    ctx.fillStyle = "#80528b"; ctx.beginPath(); ctx.arc(416, 420, 165, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = "#243341"; ctx.font = "30px sans-serif"; ctx.fillText("SYNTHETIC PREVIEW", 235, 1090);
    return { png: c.toDataURL("image/png"), jpeg: c.toDataURL("image/jpeg", .88) };
  });
  for (const [format, uri] of Object.entries(images)) assets.set(`/__studio_image_fixture__/${format === "png" ? "original.png" : "derived.jpg"}`, { mime: `image/${format}`, bytes: Buffer.from(uri.split(",")[1], "base64") });
  const seeded = await seed();
  await page.setCacheEnabled(false);
  await page.setViewport({ width: 1280, height: 1000 });
  await go(); await openFirst();
  const first = await measure(baseline ? "before-1280-light-100" : "after-1280-light-100");
  if (baseline) {
    await writeFile(join(out, "before.json"), JSON.stringify({ measurements, calls: calls.map(({ command, args }) => ({ command, args })), metrics: studioMetrics }, null, 2));
    assert(first.dialog.height < 100 || first.image.height < 100 || first.image.width < 100 || !first.imageAtCenter || first.image.y < 0 || first.image.bottom > first.viewport.height, "Expected the original modal to collapse or clip its image");
    console.log(`BASELINE modal=${first.dialog.width}×${first.dialog.height}, img=${first.image.width}×${first.image.height}, top=${first.image.y}, bottom=${first.image.bottom}, natural=${first.naturalWidth}×${first.naturalHeight}`);
  } else {
    assertVisible(first);
    await page.keyboard.press("Escape"); await closed();
    for (const width of [1280, 390]) for (const theme of ["light", "dark"]) for (const zoom of [1, 1.2]) {
      await page.setViewport({ width, height: 1000 });
      await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
      await go(zoom); await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, {}, theme); await openFirst(zoom === 1.2);
      const value = await measure(`after-${width}-${theme}-${Math.round(zoom * 100)}`);
      assertVisible(value); assert.equal(Number(value.zoom), zoom); assert(value.mainScroll > 0, "Preview must be tested after scrolling the result into view");
      await page.keyboard.press("Tab");
      assert(await page.evaluate(() => document.querySelector('[role="dialog"][aria-label="实验原图"]').contains(document.activeElement)), "Tab must stay inside the preview");
      await page.keyboard.down("Shift"); await page.keyboard.press("Tab"); await page.keyboard.up("Shift");
      assert(await page.evaluate(() => document.querySelector('[role="dialog"][aria-label="实验原图"]').contains(document.activeElement)), "Shift+Tab must stay inside the preview");
      await button("关闭原图", '[role="dialog"][aria-label="实验原图"] button'); await closed();
    }
    // Restoring a saved JPEG row must use its derived path, while the dedicated
    // original-image button uses original_path. No new plan/run is created.
    await answerStudio("studio_workspace_save", { state: { version: 1,
      forms: { draw: { jpeg: true, jpeg_quality: 88 } }, selected_id: "synthetic-jpeg-row", active_id: null,
      rows: [{ id: "synthetic-jpeg-row", label: "Synthetic JPEG preview", request: seeded.req,
        plan_id: seeded.plan.plan_id, run_id: seeded.run.run_id, phase: "done", jpeg_enabled: true,
        request_id: "synthetic-preview-only", seed: 42 }] } });
    await page.setViewport({ width: 1280, height: 1000 });
    await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: "light" }]);
    await go(1, false); await openFirst();
    const jpeg = await measure("jpeg-derived"); assertVisible(jpeg);
    assert.equal(new URL(jpeg.src).pathname, "/__studio_image_fixture__/derived.jpg");
    assert(calls.some((c) => c.command === "studio_image" && c.args.format === "jpeg" && c.args.quality === 88));
    await page.keyboard.press("Escape"); await closed();
    const originalButton = await page.evaluateHandle(() => [...document.querySelectorAll(".image-info button")].find((e) => e.textContent.trim() === "查看 PNG 原图"));
    await originalButton.evaluate((e) => { document.querySelectorAll('[data-preview-trigger]').forEach((el) => delete el.dataset.previewTrigger); e.dataset.previewTrigger = "true"; e.scrollIntoView({ block: "center" }); e.focus(); });
    holdOriginal = true;
    await originalButton.click();
    await page.waitForFunction(() => document.querySelector('[role="dialog"] [role="status"]')?.textContent.includes("正在加载原图"));
    await page.waitForFunction(() => document.querySelector('[role="dialog"] [aria-busy="true"]'));
    for (const theme of ["light", "dark"]) {
      await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
      await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, {}, theme); await delay(400);
      const value = await measure(`original-loading-${theme}`); assertAccessible(value);
      assert(value.loading && value.textContrast.some((text) => text.text.includes("正在加载原图")));
    }
    const beforeLoadingCalls = calls.length;
    for (let i = 0; !releaseOriginal && i < 100; i++) await delay(20);
    assert(releaseOriginal, "Synthetic original request must be blocked by the asset fixture");
    holdOriginal = false; releaseOriginal(); releaseOriginal = null;
    await page.waitForFunction(() => { const img = document.querySelector('[role="dialog"][aria-label="实验原图"] img'); return img?.naturalWidth > 0 && getComputedStyle(img).visibility === "visible"; });
    const original = await measure("png-original"); assertVisible(original);
    assert.equal(new URL(original.src).pathname, "/__studio_image_fixture__/original.png");
    assert.equal(calls.length, beforeLoadingCalls, "Loading the original asset must not trigger another IPC/model request");
    await page.keyboard.press("Escape"); await closed();
    await originalButton.dispose();

    // File decoding/loading failure stays in the dialog with a retry action.
    // A new navigation avoids a previously decoded original masking the failure.
    await go(1, false);
    await page.waitForFunction(() => document.querySelector(".image-open img")?.naturalWidth > 0);
    brokenAsset = true;
    await page.$$eval(".image-info button", (buttons) => { const e = buttons.find((v) => v.textContent.trim() === "查看 PNG 原图"); document.querySelectorAll('[data-preview-trigger]').forEach((el) => delete el.dataset.previewTrigger); e.dataset.previewTrigger = "true"; e.scrollIntoView({ block: "center" }); e.focus(); e.click(); });
    await page.waitForFunction(() => document.querySelector('[role="dialog"] [role="alert"]')?.textContent.includes("原图加载失败"));
    for (const theme of ["light", "dark"]) {
      await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
      await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, {}, theme); await delay(400);
      const value = await measure(`original-load-error-${theme}`); assertAccessible(value);
      assert(value.textContrast.some((text) => text.text.includes("原图加载失败")));
    }
    const beforeRetry = calls.length;
    brokenAsset = false;
    await button("重试加载", '[role="dialog"] button');
    await page.waitForFunction(() => { const img = document.querySelector('[role="dialog"][aria-label="实验原图"] img'); return img?.naturalWidth > 0 && getComputedStyle(img).visibility === "visible"; });
    assertVisible(await measure("original-retried"));
    assert.equal(calls.length, beforeRetry, "Image asset retry must not resubmit the run or call IPC");
    await button("关闭原图", '[role="dialog"] button'); await closed();

    // Transport failure uses the existing image loader retry, without generation.
    imageFailure = true;
    await go(1);
    await page.waitForFunction(() => document.querySelector(".image-placeholder")?.textContent.includes("合成取图失败"));
    imageFailure = false;
    await button("重试取图", ".image-placeholder button");
    await openFirst(); assertVisible(await measure("image-ipc-retried"));
    await page.keyboard.press("Escape"); await closed();
    assert.equal(calls.filter((c) => c.command === "studio_start").length, 0, "Preview must not create image generations");
    assert.equal(calls.filter((c) => c.command === "studio_plan").length, 0, "Preview must not call the model");
    assert.deepEqual(errors, []);
    console.log(`PASS studio image preview: ${measurements.length} sizes/theme/zoom measurements; no generation/model requests`);
  }
  await writeFile(join(out, baseline ? "before.json" : "measurements.json"), JSON.stringify({ measurements, calls: calls.map(({ command, args }) => ({ command, args })), metrics: studioMetrics }, null, 2));
} finally { await browser.close(); await server.close(); }
