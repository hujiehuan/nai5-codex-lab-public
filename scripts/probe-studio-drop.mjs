// Offline native-event integration. Uses the real Tauri webview/event JS; only the
// native invoke/callback transport and image IPC responses are synthetic.
// Baseline: STUDIO_DROP_ROOT=/tmp/<snapshot> EXPECT_STUDIO_DROP=0 node scripts/probe-studio-drop.mjs
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerStudio, resetStudio, studioMetrics } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";

const baseline = process.env.EXPECT_STUDIO_DROP === "0";
const root = resolve(process.env.STUDIO_DROP_ROOT || ".");
const out = process.env.STUDIO_DROP_OUT || `/tmp/nai5-studio-drop-${baseline ? "before" : "after"}`;
await mkdir(out, { recursive: true });
const server = await createServer({ root, mode: "mock", cacheDir: join(out, "vite-cache"), optimizeDeps: { include: ["@tauri-apps/api/webview", "@tauri-apps/plugin-dialog"] }, server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), calls = [], errors = [], layouts = [], passed = [];
let picked = 0, nextPick = undefined;
const held = new Map(), releases = new Map();
const zone = '[data-testid="studio-source-drop"]';
page.on("pageerror", e => errors.push(e.message));
await page.evaluateOnNewDocument(() => {
  localStorage.setItem("codexlab.zoom", "1.2");
  Object.defineProperty(navigator, "platform", { value: "MacIntel", configurable: true });
  let next = 0;
  const callbacks = new Map(), listeners = new Map(), retired = [], commands = [];
  const remove = (event, id) => { const listener = listeners.get(id); if (listener && listener.event === event) { retired.push({ ...listener, callback: callbacks.get(listener.handler) }); listeners.delete(id); callbacks.delete(listener.handler); } };
  const state = {
    commands, holdNextListen: false, releaseListen: null,
    registrations: () => [...listeners.values()].filter(l => l.event.startsWith("tauri://drag-")),
    emit(event, payload) { for (const l of [...listeners.values()]) if (l.event === event) callbacks.get(l.handler)?.({ event, id: l.id, payload }); },
    emitRetired(event, payload) { for (const l of retired) if (l.event === event) l.callback?.({ event, id: l.id, payload }); },
  };
  window.__nativeDrop = state;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: remove };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback(fn) { const id = ++next; callbacks.set(id, fn); return id; },
    unregisterCallback(id) { callbacks.delete(id); },
    convertFileSrc(path) {
      if (path.includes("corrupt")) return "data:image/png;base64,bm90LWFuLWltYWdl";
      return `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="160" height="200"><rect width="160" height="200" fill="#a276b8"/><text x="10" y="100" font-size="12">${path.replace(/[<>&]/g, "_")}</text></svg>`)}`;
    },
    async invoke(command, args) {
      commands.push({ command, args });
      if (command === "plugin:event|listen") {
        const id = ++next; listeners.set(id, { id, ...args });
        if (state.holdNextListen && args.event.startsWith("tauri://drag-")) { state.holdNextListen = false; await new Promise(r => { state.releaseListen = r; }); }
        return id;
      }
      if (command === "plugin:event|unlisten") { remove(args.event, args.eventId); return null; }
      return null;
    },
  };
});
await page.setRequestInterception(true);
page.on("request", async request => {
  if (!request.url().endsWith("/__mock__/invoke")) return request.continue();
  const body = JSON.parse(request.postData());
  if (!body.command.startsWith("studio_") && !["reverse_pick_local_image", "reverse_import_local_image"].includes(body.command)) return request.continue();
  calls.push(body);
  const key = body.command === "reverse_import_local_image" ? body.args.path : body.command;
  try {
    if (held.has(key)) await new Promise(r => releases.set(key, r));
    let value;
    if (body.command === "reverse_pick_local_image") { value = nextPick === undefined ? `/synthetic/private/picked-${++picked}.png` : nextPick; nextPick = undefined; }
    else if (body.command === "reverse_import_local_image") {
      if (body.args.path.includes("missing")) throw { code: "io", message: "合成文件读取失败" };
      value = `/synthetic/private/${body.args.path.split("/").at(-1)}`;
    } else value = await answerStudio(body.command, body.args);
    body.result = value;
    await request.respond({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
  } catch (error) {
    body.error = error;
    await request.respond({ status: 400, contentType: "application/json", body: JSON.stringify(error) });
  }
});
const count = command => calls.filter(c => c.command === command).length;
const imports = () => count("reverse_import_local_image");
const pause = () => new Promise(r => setTimeout(r, 120));
async function waitText(text, scope = "body") { await page.waitForFunction(({ text, scope }) => document.querySelector(scope)?.textContent.includes(text), {}, { text, scope }); }
async function button(text, scope = "body") {
  await page.waitForFunction(({ text, scope }) => [...document.querySelectorAll(`${scope} button`)].some(e => e.checkVisibility() && !e.disabled && e.textContent.trim() === text), {}, { text, scope });
  for (const e of await page.$$(`${scope} button`)) if (await e.evaluate((el, value) => el.checkVisibility() && !el.disabled && el.textContent.trim() === value, text)) { await e.evaluate(el => el.scrollIntoView({ block: "center" })); await e.click(); return; }
}
async function mode(name) {
  await page.$$eval('.mode-tabs button', (es, name) => es.find(e => e.querySelector('strong')?.textContent.trim().replace(/^\//, "") === name).click(), name);
  await pause();
}
async function listeners(n) { await page.waitForFunction(n => window.__nativeDrop.registrations().length === n, {}, n); }
async function imagePath() { return page.$eval('img.source-image', e => decodeURIComponent(e.getAttribute('src')).match(/<text[^>]*>(.*?)<\/text>/)?.[1]); }
async function expectImage(path) { await page.waitForFunction(({ zone, path }) => { const e = document.querySelector(`${zone} img.source-image`); return e?.complete && e.naturalWidth > 0 && decodeURIComponent(e.getAttribute('src')).includes(path); }, {}, { zone, path }); }
async function point(where = "inside") {
  await page.$eval(zone, e => e.scrollIntoView({ block: "center" }));
  return page.$eval(zone, (e, where) => { const r = e.getBoundingClientRect(), ratio = navigator.platform.startsWith("Mac") ? 1 : devicePixelRatio; return where === "outside" ? { x: 2 * ratio, y: 2 * ratio } : { x: (r.left + 6) * ratio, y: (r.top + 6) * ratio }; }, where);
}
async function emit(type, paths = [], where = "inside", retired = false) {
  const position = type === "leave" ? undefined : await point(where);
  await page.evaluate(({ type, paths, position, retired }) => window.__nativeDrop[retired ? "emitRetired" : "emit"](`tauri://drag-${type}`, { paths, position }), { type, paths, position, retired });
  await pause();
}
async function release(key) { held.delete(key); releases.get(key)?.(); releases.delete(key); await pause(); }
async function waitHeld(key) { for (let i = 0; i < 150 && !releases.has(key); i++) await new Promise(r => setTimeout(r, 20)); assert(releases.has(key), `held operation reached: ${key}`); }
async function measure() {
  // Restore the app's real CSS zoom through its shortcut before normal layout checks.
  await page.keyboard.down("Control"); await page.keyboard.press("0"); await page.keyboard.up("Control");
  for (const width of [1280, 390]) for (const theme of ["light", "dark"]) {
    await page.setViewport({ width, height: 1000, deviceScaleFactor: 1 });
    await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
    await page.waitForFunction(t => document.documentElement.dataset.theme === t, {}, theme);
    await new Promise(r => setTimeout(r, 500));
    await emit("enter", ["/synthetic/layout.png"]);
    const measured = await page.$eval(zone, root => {
      const visible = e => e.checkVisibility() && e.getBoundingClientRect().width > 0;
      const targets = [...root.querySelectorAll('button,[role="button"]')].filter(visible).filter(e => !e.disabled).map(e => ({ text: e.textContent.trim(), w: e.getBoundingClientRect().width, h: e.getBoundingClientRect().height })).filter(e => e.w < 24 || e.h < 24);
      const canvas = document.createElement("canvas"); canvas.width = canvas.height = 1; const ctx = canvas.getContext("2d", { willReadFrequently: true });
      const color = s => { ctx.clearRect(0, 0, 1, 1); ctx.fillStyle = s; ctx.fillRect(0, 0, 1, 1); const c = [...ctx.getImageData(0, 0, 1, 1).data]; c[3] /= 255; return c; };
      const blend = (a, b) => [0, 1, 2].map(i => a[i] * a[3] + b[i] * (1 - a[3]));
      const lum = c => c.slice(0, 3).map(v => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
      const low = [], walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT); let node;
      while ((node = walker.nextNode())) {
        const e = node.parentElement; if (!node.textContent.trim() || !e || !visible(e) || e.closest('[disabled]')) continue;
        const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p);
        if (parents.some(p => +getComputedStyle(p).opacity < .99)) continue;
        let bg = [255, 255, 255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg);
        const s = getComputedStyle(e), a = lum(blend(color(s.color), bg)), b = lum(bg), ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
        const large = parseFloat(s.fontSize) >= 24 || (parseFloat(s.fontSize) >= 18.66 && parseInt(s.fontWeight) >= 700);
        if (ratio < (large ? 3 : 4.5) - .01) low.push({ text: node.textContent.trim(), ratio });
      }
      return { overflow: document.documentElement.scrollWidth - innerWidth, targets, low, highlighted: root.classList.contains("drag-over") };
    });
    assert.equal(measured.overflow, 0); assert.deepEqual(measured.targets, []); assert.deepEqual(measured.low, []); assert(measured.highlighted);
    layouts.push({ width, theme, ...measured });
    await page.screenshot({ path: join(out, `${width}-${theme}.png`) }); await emit("leave");
  }
}
try {
  resetStudio(); await page.setViewport({ width: 1280, height: 1000, deviceScaleFactor: 2 });
  await page.goto(`${base}/#/studio?mode=reverse`, { waitUntil: "networkidle0" });
  await page.waitForFunction(() => document.querySelector('[data-testid="studio"]') && !document.body.textContent.includes("正在读取插件实验能力"));
  if (baseline) {
    assert.equal(await page.$(zone), null); await listeners(0);
    await page.evaluate(() => window.__nativeDrop.emit("tauri://drag-drop", { paths: ["/synthetic/baseline.png"], position: { x: 400, y: 500 } }));
    await pause(); assert.equal(imports(), 0);
    await button("选择反推原图"); await page.waitForSelector('img.source-image');
    assert.equal(count("reverse_pick_local_image"), 1); assert.equal(imports(), 1);
    passed.push("baseline: zero native drag listeners, drop ignored, old click selection still works");
  } else {
    await page.waitForSelector(zone); await listeners(4);
    const registrations = await page.evaluate(() => window.__nativeDrop.registrations());
    assert.deepEqual(registrations.map(e => e.event).sort(), ["enter", "over", "drop", "leave"].map(e => `tauri://drag-${e}`).sort());
    assert(registrations.every(e => e.target.kind === "Webview" && e.target.label === "main"));
    assert.equal(await page.evaluate(() => devicePixelRatio), 2); assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).zoom), "1.2");
    await emit("enter", ["/Users/example/Pictures/测试 原图.PNG"]); assert(await page.$eval(zone, e => e.classList.contains("drag-over")));
    await emit("over", [], "outside"); assert(!(await page.$eval(zone, e => e.classList.contains("drag-over"))));
    await emit("over"); assert(await page.$eval(zone, e => e.classList.contains("drag-over")));
    await emit("leave"); assert(!(await page.$eval(zone, e => e.classList.contains("drag-over"))));
    await emit("drop", ["/Users/example/Pictures/测试 原图.PNG"]); await expectImage("/synthetic/private/测试 原图.PNG");
    assert.deepEqual(calls.find(c => c.command === "reverse_import_local_image").args, { path: "/Users/example/Pictures/测试 原图.PNG" });
    assert.equal(imports(), 1); passed.push("real Tauri event registration; Mac logical coordinates at DPR 2 + CSS zoom 1.2; exact source path IPC; hover/leave");
    const first = await imagePath(); let before = imports();
    await emit("drop", ["/synthetic/outside.png"], "outside"); assert.equal(imports(), before);
    await page.evaluate(() => { const cover = document.createElement('div'); cover.id = 'probe-cover'; Object.assign(cover.style, { position: 'fixed', inset: '0', zIndex: '999999' }); document.body.append(cover); });
    await emit("enter", ["/synthetic/covered.png"]); assert(!(await page.$eval(zone, e => e.classList.contains("drag-over"))));
    await emit("drop", ["/synthetic/covered.png"]); assert.equal(imports(), before); await page.$eval('#probe-cover', e => e.remove());
    for (const paths of [[], ["/synthetic/a.png", "/synthetic/b.jpg"], ["/synthetic/not-image.txt"], ["/synthetic/disguised.png.exe"]]) {
      await emit("drop", paths); await page.waitForSelector(`${zone} [role="alert"]`); assert.equal(imports(), before); assert.equal(await imagePath(), first);
    }
    await emit("drop", ["/synthetic/missing.jpg"]); await waitText("合成文件读取失败", zone); assert.equal(await imagePath(), first);
    await emit("drop", ["/synthetic/corrupt.png"]); await waitText("图片无法读取", zone); assert.equal(await imagePath(), first);
    passed.push("outside/covered drop ignored; empty/multiple/non-image rejected; read/decode failure preserves previous image");
    for (const extension of ["jpg", "jpeg", "webp", "gif"]) { const path = `/synthetic/accepted.${extension}`; await emit("drop", [path]); await expectImage(`/synthetic/private/accepted.${extension}`); }
    before = imports(); await button("更换反推原图", zone); await expectImage("/synthetic/private/picked-1.png"); assert.equal(imports(), before, "picker already imports: no second copy");
    nextPick = null; await button("更换反推原图", zone); await page.waitForFunction(s => document.querySelector(s).getAttribute('aria-busy') === 'false', {}, zone); await expectImage("/synthetic/private/picked-1.png");
    passed.push("all five supported extensions; click selection imports once; cancelled picker retains previous image");
    const slow = "/synthetic/slow.png"; held.set(slow, true); await emit("drop", [slow]); await waitHeld(slow); await waitText("读取图片中…", zone);
    assert(await page.$eval(`${zone} button`, e => e.disabled)); assert(await page.$eval('.action-bar button.primary', e => e.disabled));
    before = imports(); await emit("drop", ["/synthetic/duplicate.png"]); assert.equal(imports(), before);
    await page.keyboard.down("Control"); await page.keyboard.press("Enter"); await page.keyboard.up("Control"); await pause(); assert.equal(studioMetrics.prepare, 0);
    await release(slow); await expectImage("/synthetic/private/slow.png"); passed.push("pending import locks picker/drop/build button/keyboard shortcut");
    const stale = "/synthetic/stale.png"; held.set(stale, true); await emit("drop", [stale]); await waitHeld(stale);
    await mode("画"); await listeners(0); assert.equal(await page.$(zone), null);
    before = imports(); await page.evaluate(() => { const payload = { paths: ['/synthetic/after-unmount.png'], position: { x: 600, y: 700 } }; window.__nativeDrop.emit('tauri://drag-drop', payload); window.__nativeDrop.emitRetired('tauri://drag-drop', payload); }); await pause(); assert.equal(imports(), before);
    await mode("反推"); await listeners(4); await button("更换反推原图", zone); await expectImage("/synthetic/private/picked-2.png");
    await release(stale); await expectImage("/synthetic/private/picked-2.png");
    await pause(); assert.equal(await imagePath(), "/synthetic/private/picked-2.png");
    passed.push("unmount unsubscribes; late queued event ignored; delayed prior import cannot overwrite next page/image");
    // A component can unmount while Tauri's four registrations are still awaiting IPC.
    await mode("画"); await listeners(0); await page.evaluate(() => { window.__nativeDrop.holdNextListen = true; });
    await mode("反推"); await page.waitForFunction(() => typeof window.__nativeDrop.releaseListen === 'function'); await mode("画");
    await page.evaluate(() => { window.__nativeDrop.releaseListen(); window.__nativeDrop.releaseListen = null; }); await listeners(0);
    await mode("反推"); await listeners(4); passed.push("late native subscription is cleaned up after unmount");
    held.set("studio_prepare", true); await button("构建对照方案"); await waitHeld("studio_prepare");
    before = imports(); await emit("drop", ["/synthetic/while-planning.png"]); assert.equal(imports(), before); assert(await page.$eval(`${zone} button`, e => e.disabled));
    await release("studio_prepare"); await waitText("方案已冻结");
    assert.equal(calls.filter(c => c.command === "studio_prepare").at(-1).args.req.reverse.image_path, "/synthetic/private/picked-2.png");
    passed.push("active experiment locks source; prepared request uses the imported private image");
    await measure();
    // Wry's macOS callback uses logical AppKit points; other supported platforms
    // emit physical coordinates. Exercise both branches with the real SDK event.
    await page.evaluateOnNewDocument(() => Object.defineProperty(navigator, "platform", { value: "Linux x86_64", configurable: true }));
    await page.setViewport({ width: 1280, height: 1000, deviceScaleFactor: 2 });
    await page.reload({ waitUntil: "networkidle0" }); await listeners(4);
    await emit("drop", ["/synthetic/linux-retina.jpg"]); await expectImage("/synthetic/private/linux-retina.jpg");
    passed.push("non-Mac physical coordinates at DPR 2 + CSS zoom 1.2");
    await page.evaluate(() => { location.hash = '#/settings'; }); await listeners(0);
    assert.equal(await page.$(zone), null); passed.push("leaving Studio unregisters all native drop listeners");
  }
  assert.deepEqual(errors, []); assert.equal(studioMetrics.generations, 0);
  const result = { pass: true, baseline, root, passed, imports: imports(), pickerCalls: count("reverse_pick_local_image"), realModelCalls: 0, generatedImages: 0, errors, layouts };
  await writeFile(join(out, "result.json"), JSON.stringify(result, null, 2)); console.log(JSON.stringify(result));
} catch (e) { await page.screenshot({ path: join(out, "failure.png") }); console.error(JSON.stringify({ passed, calls: calls.map(({ command, args, error }) => ({ command, path: args?.path, error })), errors, source: await page.$eval(zone, e => e.innerText).catch(() => "unmounted") })); throw e; }
finally { for (const release of releases.values()) release(); await browser.close(); await server.close(); }
