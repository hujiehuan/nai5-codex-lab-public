// Reverse-evaluation page against mock/reverse.mjs: synthetic batches, per-model summary, run detail, batch compare,
// and a new batch that finishes through the page's own polling. Offline only; no model or plugin calls.
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { executablePath } from "./chrome.mjs";

const out = process.env.EVAL_OUT || "/tmp/nai5-eval-probe";
await mkdir(out, { recursive: true });
const server = await createServer({ mode: "mock", cacheDir: join(out, "vite-cache"), server: { port: 0, host: "127.0.0.1", strictPort: false } });
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage(), errors = [];
page.on("pageerror", (e) => errors.push(e.message));
await page.evaluateOnNewDocument(() => {
  let id = 0;
  window.__TAURI_INTERNALS__ = { transformCallback: () => ++id, unregisterCallback: () => {}, convertFileSrc: (p) => p, invoke: async () => 1 };
});
const waitText = (text) => page.waitForFunction((s) => document.body.textContent.includes(s), {}, text);
async function clickButton(text) {
  const handle = await page.waitForFunction((t) => [...document.querySelectorAll("button")].find((b) => b.textContent.trim().includes(t) && !b.disabled), {}, text);
  await handle.asElement().click();
}
try {
  await page.setViewport({ width: 1440, height: 1000 });
  await page.goto(`${base}/#/eval`, { waitUntil: "networkidle0" });
  const batchSelect = ".results select";
  await page.waitForFunction((s) => document.querySelector(s)?.options.length === 4, {}, batchSelect);
  const labels = await page.$$eval(`${batchSelect} option`, (o) => o.map((x) => x.textContent));
  assert(labels.slice(1).every((t) => t.includes("vision-model-a+vision-model-b+vision-model-c")), labels.join("\n"));

  // Per-model summary of the retrieval batch: three models, 12 questions each, retrieval hits recorded.
  await page.select(batchSelect, "rb-20261028-style");
  await page.waitForFunction(() => document.querySelectorAll(".tbl.summary tbody tr").length === 3 && document.querySelectorAll(".tbl.runs tbody tr").length === 36);
  const summary = await page.$$eval(".tbl.summary tbody tr", (rows) => rows.map((r) => [...r.cells].map((c) => c.textContent.trim())));
  assert.deepEqual(summary.map((r) => r[0]), ["vision-model-a", "vision-model-b", "vision-model-c"]);
  assert(summary.every((r) => r[1] === "12" && /%/.test(r.join(" "))), JSON.stringify(summary));

  // Run detail opens with the original, the redraw and the raw model output; Escape closes it.
  await page.click(".tbl.runs tbody tr");
  await page.waitForSelector(".panel-wrap .panel");
  await waitText("raw_response");
  await page.keyboard.press("Escape");
  await page.waitForSelector(".panel-wrap", { hidden: true });

  // Compare the baseline with the template-v2 batch.
  await clickButton("对比两个批次");
  await page.waitForSelector(".compare select");
  const [a, b] = await page.$$(".compare select");
  await a.select("rb-20261020-base");
  await b.select("rb-20261024-pool");
  await page.$$eval(".compare button", (buttons) => buttons.find((x) => x.textContent.trim() === "对比").click());
  await waitText("平均 F1：A");
  assert(await page.$(".compare tbody tr"), "changed questions are listed");
  await page.screenshot({ path: join(out, "eval-compare.png"), fullPage: true });

  // A new batch from the page: one question × one model, completed by the store's 3 s batch-list polling.
  await page.click(".bank .list li input[type=checkbox]");
  const provider = await page.waitForFunction(() => [...document.querySelectorAll(".plist label")].find((l) => l.textContent.includes("vision-model-a")));
  await (await provider.asElement().$("input")).click();
  await clickButton("运行 batch");
  await waitText("批次完成");
  await page.waitForFunction((s) => document.querySelector(s)?.options.length === 5, {}, batchSelect);
  assert.deepEqual(errors, []);
  console.log(`PASS eval: 3 synthetic batches, per-model P/R/F1 summary, run detail, batch compare, new batch via polling. ${out}`);
} finally {
  await browser.close();
  await server.close();
}
