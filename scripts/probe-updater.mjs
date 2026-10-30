// Real Vue/store/IPC with a synthetic updater. No Git, builds, backups or installation.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { answerUpdater, resetUpdater } from "../mock/updater.mjs";
import { answerStudio, resetStudio } from "../mock/studio.mjs";
import { executablePath } from "./chrome.mjs";

const out = process.env.UPDATER_OUT || "/tmp/nai5-updater-probe";
await mkdir(out, { recursive: true });
const server = await createServer({ mode: "mock", cacheDir: join(out, "vite-cache"), server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen(); const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), calls = [], errors = [], measurements = [], passed = [];
let scenario = "", holdSaves = false, failSave = false, holdPrepare = false;
const saved = [], prepared = [];
await page.setViewport({ width: 1280, height: 1000 });
await page.evaluateOnNewDocument(() => { window.__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => {}, convertFileSrc: (p) => p, invoke: async () => 1 }; });
page.on("pageerror", (e) => errors.push(e.message));
await page.setRequestInterception(true);
page.on("request", async (r) => {
  if (!r.url().endsWith("/__mock__/invoke")) return r.continue();
  const body = JSON.parse(r.postData()); calls.push(body);
  if (!body.command.startsWith("app_update_") && body.command !== "studio_workspace_save") return r.continue();
  try {
    if (body.command === "studio_workspace_save") {
      if (holdSaves) await new Promise((resolve) => saved.push(resolve));
      if (failSave) throw { code: "io", message: "模拟工作区保存失败" };
      await answerStudio(body.command, body.args);
      return r.respond({ status: 200, contentType: "application/json", body: "null" });
    }
    if (body.command === "app_update_prepare" && holdPrepare) await new Promise((resolve) => prepared.push(resolve));
    const value = await answerUpdater(body.command, body.args, scenario); body.result = value;
    return r.respond({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
  } catch (error) { body.error = error; return r.respond({ status: 400, contentType: "application/json", body: JSON.stringify(error) }); }
});
const count = (command) => calls.filter((c) => c.command === command).length;
async function go(state = "") {
  scenario = state; resetUpdater();
  await page.goto(`${base}/?updateprobe=${Date.now()}#/settings#software-update`, { waitUntil: "networkidle0" });
  await page.waitForSelector('[data-testid="software-update"]');
  await page.waitForFunction(() => !document.querySelector('.update-state')?.textContent.includes("正在读取"));
}
async function text(value) { await page.waitForFunction((v) => document.body.textContent.includes(v), {}, value); }
async function button(value) {
  for (const e of await page.$$('#software-update button')) if ((await e.evaluate((e) => e.textContent.trim())) === value) { await e.evaluate((e) => e.scrollIntoView({ block: "center" })); await e.click(); return; }
  throw Error(`Missing updater button: ${value}`);
}
async function chooseAndCheck() { await button("选择源码目录"); await text("更新来源"); await button("检查更新"); }
async function waitHeld(list) { const deadline = Date.now() + 10_000; while (!list.length && Date.now() < deadline) await new Promise((r) => setTimeout(r, 20)); assert(list.length, "expected held IPC request"); }
async function measure(name) {
  for (const width of [1280, 390]) for (const theme of ["light", "dark"]) {
    await page.setViewport({ width, height: 1000 }); await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: theme }]);
    await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, {}, theme);
    await new Promise((r) => setTimeout(r, 400));
    await page.evaluate(() => document.querySelector('main.content').scrollTo(0, 0));
    const result = await page.evaluate(() => {
      const root = document.querySelector('#software-update');
      const visible = (e) => e.checkVisibility() && e.getBoundingClientRect().width > 0 && e.getBoundingClientRect().height > 0;
      const targets = [...root.querySelectorAll('button,summary,a')].filter(visible).filter((e) => !e.disabled).map((e) => ({ text: e.textContent.trim(), width: e.getBoundingClientRect().width, height: e.getBoundingClientRect().height })).filter((e) => e.width < 24 || e.height < 24);
      const color = (s) => (s.match(/[\d.]+/g) || []).map(Number), blend = (a, b) => [0, 1, 2].map((i) => (a[i] || 0) * (a[3] ?? 1) + b[i] * (1 - (a[3] ?? 1)));
      const lum = (c) => c.slice(0, 3).map((v) => (v /= 255) <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, v, i) => a + v * [.2126, .7152, .0722][i], 0);
      const lowContrast = [], walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT); let node;
      while ((node = walker.nextNode())) {
        const e = node.parentElement; if (!e || !node.textContent.trim() || !visible(e) || e.closest('[disabled],.n-button--disabled')) continue;
        const parents = []; for (let p = e; p; p = p.parentElement) parents.push(p); if (parents.some((p) => +getComputedStyle(p).opacity < .99)) continue;
        let bg = [255, 255, 255]; for (const p of parents.reverse()) bg = blend(color(getComputedStyle(p).backgroundColor), bg);
        const s = getComputedStyle(e), a = lum(blend(color(s.color), bg)), b = lum(bg), ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
        const large = parseFloat(s.fontSize) >= 24 || (parseFloat(s.fontSize) >= 18.66 && parseInt(s.fontWeight) >= 700);
        if (ratio < (large ? 3 : 4.5) - .01) lowContrast.push({ text: node.textContent.trim().slice(0, 50), ratio });
      }
      return { overflow: document.documentElement.scrollWidth - innerWidth, targets, lowContrast };
    });
    measurements.push({ name, width, theme, ...result }); assert.equal(result.overflow, 0); assert.deepEqual(result.targets, []); assert.deepEqual(result.lowContrast, []);
    await page.screenshot({ path: join(out, `${name}-${width}-${theme}.png`), fullPage: true });
  }
}
try {
  await go(); assert.equal(count("app_update_check"), 0); assert.equal(count("app_update_prepare"), 0);
  await text("Xcode 命令行工具"); assert.equal(await page.$('#software-update .update-source'), null);
  await measure("first-use"); await chooseAndCheck(); await text("发现新版本");
  assert.equal(await page.$eval('.update-source', (e) => e.open), false);
  await measure("available");
  holdPrepare = true; const before = count("app_update_prepare"); await button("准备更新"); await waitHeld(prepared);
  assert.equal(await page.$$eval('#software-update button', (els) => els.filter((e) => !e.disabled && e.textContent.includes("准备更新")).length), 0);
  await page.click('a.nav[href="#/sync"]'); holdPrepare = false; prepared.splice(0).forEach((r) => r());
  await new Promise((r) => setTimeout(r, 1400)); await page.click('a.nav[href="#/settings"]'); await text("正在准备更新");
  assert.equal(count("app_update_prepare"), before + 1); assert.equal(await page.$eval('[role="progressbar"]', (e) => e.hasAttribute("aria-valuenow")), false);
  await text("更新已准备好"); assert.equal(count("app_update_install"), 0); await measure("ready");
  passed.push("initial status only; source setup; check separate from preparation; no duplicate preparation; cross-page progress; no fake percentage; explicit install");

  await go("update-build-error"); await chooseAndCheck(); await text("发现新版本"); await button("准备更新"); await text("缺少 Rust / Cargo");
  assert.equal(await page.$$eval('#software-update button', (els) => els.filter((e) => e.textContent.includes("安装并重启")).length), 0);
  await button("重新准备更新"); await text("正在准备更新"); await button("取消更新"); await text("已取消更新");
  passed.push("build failure never permits install; explicit retry/cancel");
  await go("update-auth-error"); await chooseAndCheck(); await text("Git 鉴权失败"); assert(!(await page.$eval('.update-state', (e) => e.textContent)).includes("最新"));
  await go("update-picker-cancel"); await button("选择源码目录"); await text("首次选择本机");
  await go("update-latest"); await chooseAndCheck(); await text("当前已是最新稳定版");
  await go("update-interrupted"); await text("上次准备更新已中断"); assert.equal(count("app_update_install"), 0);
  for (const s of ["update-read-error", "update-malformed"]) { await go(s); await text("重试读取"); assert(!(await page.$eval('.update-state', (e) => e.textContent)).includes("最新")); }
  await go("update-unwritable"); await text("只读位置"); assert(await page.$$eval('#software-update button', (els) => els.find((e) => e.textContent.includes("安装并重启"))?.disabled));
  await go("update-unsupported"); assert(await page.$$eval('#software-update button', (els) => els.find((e) => e.textContent.includes("选择源码目录"))?.disabled));
  passed.push("authentication failure, cancelled picker, up-to-date, interrupted restart, malformed/read error, unsupported platform, unwritable app");

  await go("update-install-error"); await button("安装并重启"); await text("升级备份失败"); assert.equal(await page.$eval('.shell', (e) => e.inert), false);
  passed.push("backup/install rejection unlocks the existing application");
  await go("update-ready"); resetStudio(); await page.click('a.nav[href="#/studio"]'); await page.waitForSelector('textarea[aria-label="必须满足的内容"]');
  await page.waitForFunction(() => !document.body.textContent.includes("正在读取插件实验能力"));
  holdSaves = true;
  await page.$eval('textarea[aria-label="必须满足的内容"]', (e) => { e.value = "必须保存的实验要求"; e.dispatchEvent(new Event("input", { bubbles: true })); });
  await waitHeld(saved); await page.click('a.nav[href="#/settings"]'); await text("更新已准备好");
  const beforeInstall = count("app_update_install"); await button("安装并重启"); await page.waitForSelector('[role="dialog"][aria-labelledby="install-title"]');
  assert.equal(await page.$eval('.shell', (e) => e.inert), true); assert.equal(count("app_update_install"), beforeInstall);
  holdSaves = false; saved.splice(0).forEach((r) => r()); await text("离线模拟不会退出或替换应用");
  assert.equal(count("app_update_install"), beforeInstall + 1);
  assert.equal((await answerStudio("studio_workspace_get")).forms.draw.requirements, "必须保存的实验要求");
  passed.push("installation locks all interaction before draining real Studio saves across route change; exact latest requirements persisted before install");

  await go("update-ready"); await page.click('a.nav[href="#/studio"]'); await page.waitForSelector('textarea[aria-label="必须满足的内容"]');
  await page.waitForFunction(() => !document.body.textContent.includes("正在读取插件实验能力")); failSave = true;
  await page.$eval('textarea[aria-label="必须满足的内容"]', (e) => { e.value = "不能丢失的未保存要求"; e.dispatchEvent(new Event("input", { bubbles: true })); });
  await text("模拟工作区保存失败"); await page.click('a.nav[href="#/settings"]'); await text("更新已准备好");
  const blocked = count("app_update_install"); await button("安装并重启"); await text("模拟工作区保存失败");
  assert.equal(count("app_update_install"), blocked); assert.equal(await page.$eval('.shell', (e) => e.inert), false); failSave = false;
  passed.push("failed Studio save blocks native installation and restores interaction");
  assert.deepEqual(errors, []);
  const result = { passed, measurements, errors, actualGitCalls: 0, actualBuilds: 0, actualInstalls: 0 };
  await writeFile(join(out, "result.json"), JSON.stringify(result, null, 2)); console.log(`PASS updater: ${passed.length} behavior groups, ${measurements.length} width/theme measurements. ${out}`);
} finally { saved.splice(0).forEach((r) => r()); prepared.splice(0).forEach((r) => r()); await browser.close(); await server.close(); }
