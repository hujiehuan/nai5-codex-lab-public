import assert from "node:assert/strict";
import puppeteer from "puppeteer-core";
import { createServer } from "vite";
import { executablePath } from "./chrome.mjs";
const server = await createServer({
  mode: "mock",
  server: { port: 0, host: "127.0.0.1" },
});
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await puppeteer.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});
const page = await browser.newPage();
await page.setViewport({ width: 1280, height: 1000 });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
await page.evaluateOnNewDocument(() => {
  let id = 0;
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => ++id,
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async () => 1,
  };
});
try {
  await page.goto(base + "/#/codex/artist_nai5_personal", {
    waitUntil: "networkidle0",
  });
  const first = '[data-entry-id="fixture-artist_nai5_personal-0"]';
  await page.waitForSelector(first);
  assert(await page.$(first + ".undecided"));
  for (const state of ["include", "exclude", "undecided"]) {
    await page.click(first + " .thumb");
    await page.waitForSelector(first + "." + state);
  }
  await page.focus('.grid[tabindex="0"]');
  await page.keyboard.press("a");
  await page.waitForSelector(first + ".include");
  await page.keyboard.down("Meta");
  await page.keyboard.press("z");
  await page.keyboard.up("Meta");
  await page.waitForSelector(first + ".undecided");
  await page.keyboard.down("Meta");
  await page.keyboard.press("a");
  await page.keyboard.up("Meta");
  await page.waitForSelector(".batchbar");
  await page.evaluate(() =>
    [...document.querySelectorAll(".batchbar button")]
      .find((e) => e.textContent === "全部加入")
      .click(),
  );
  await page.waitForFunction(() =>
    [...document.querySelectorAll("[data-entry-id]")].every((e) =>
      e.classList.contains("include"),
    ),
  );
  // Right click opens details; long prompts remain inside their own container.
  await page.click(first, { button: "right" });
  await page.waitForSelector(".decisions");
  await page.keyboard.press("Escape");
  await page.waitForSelector(".decisions", { hidden: true });
  // Artist sources share the browser, while filters and decisions stay local to each source.
  await page.type('.filterbar input[type="search"]', "合成示例 1");
  await page.waitForFunction(() =>
    document.querySelector(".summary")?.textContent.includes("当前筛选 11 条"),
  );
  await page.click('.source-tab[href="#/codex/nax_v5_artist"]');
  const naxFirst = '[data-entry-id="fixture-nax_v5_artist-0"]';
  await page.waitForSelector(naxFirst + ".undecided");
  assert.equal(
    await page.$eval('.filterbar input[type="search"]', (e) => e.value),
    "",
  );
  assert(
    await page.$(
      '.sidebar a[href="#/codex/artist_nai5_personal"][aria-current="page"]',
    ),
  );
  assert(
    await page.$(
      '.source-tab[href="#/codex/nax_v5_artist"][aria-current="page"]',
    ),
  );
  assert.equal(
    await page.$('[data-entry-id^="fixture-artist_nai5_personal"]'),
    null,
  );
  await page.click(naxFirst + " .thumb");
  await page.waitForSelector(naxFirst + ".include");
  await page.type('.filterbar input[type="search"]', "synthetic_artist_1");
  await page.waitForFunction(() =>
    document.querySelector(".summary")?.textContent.includes("当前筛选 11 条"),
  );
  await page.evaluate(() =>
    [...document.querySelectorAll(".filterbar label")]
      .find((e) => e.textContent.includes("本次新增"))
      .querySelector("input")
      .click(),
  );
  await page.waitForFunction(() =>
    document
      .querySelector(".summary")
      ?.textContent.includes("新增于 nax-v5-fixture"),
  );
  await page.click(naxFirst, { button: "right" });
  await page.waitForSelector(".decisions");
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text) => {
          window.__copiedArtist = text;
        },
      },
    });
    [...document.querySelectorAll("button")]
      .find((e) => e.textContent.trim() === "复制画师串")
      .click();
  });
  await page.waitForFunction(() =>
    window.__copiedArtist?.startsWith("artist:synthetic_artist_1,"),
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector(".decisions", { hidden: true });
  await page.click('.source-tab[href="#/codex/artist_nai5_personal"]');
  await page.waitForSelector(first + ".include");
  assert.equal(
    await page.$eval('.filterbar input[type="search"]', (e) => e.value),
    "合成示例 1",
  );
  await page.click('.source-tab[href="#/codex/nax_v5_artist"]');
  await page.waitForSelector(naxFirst + ".include");
  assert.equal(
    await page.$eval('.filterbar input[type="search"]', (e) => e.value),
    "synthetic_artist_1",
  );
  // Reload proves persisted source filters still apply; choosing NAX never auto-includes its entries.
  await page.reload({ waitUntil: "networkidle0" });
  await page.waitForSelector(naxFirst + ".include");
  assert.equal(
    await page.$eval('.filterbar input[type="search"]', (e) => e.value),
    "synthetic_artist_1",
  );
  assert.match(
    await page.$eval(".summary", (e) => e.textContent),
    /新增于 nax-v5-fixture/,
  );
  assert(await page.$('[data-entry-id="fixture-nax_v5_artist-9"].undecided'));
  await page.goto(base + "/#/sync", { waitUntil: "networkidle0" });
  await page.waitForSelector(".page-heading");
  assert.match(
    await page.$eval(".page-heading", (e) => e.textContent),
    /NAX V5/,
  );
  for (const route of [
    "/sync",
    "/settings",
    "/reverse",
    "/codex/artist_nai5_personal",
    "/lab/draw",
  ]) {
    await page.goto(
      `${base}/?scenario=error&screen=${encodeURIComponent(route)}#${route}`,
      { waitUntil: "networkidle0" },
    );
    assert(
      await page.$('[role="alert"],.err'),
      "failed request is visibly distinct from empty data on " + route,
    );
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: three-state cycle, A shortcut, Cmd-Z undo, Cmd-A and bulk include, NAX source switch/filter persistence/independent decisions/copy, detail/Escape, five read-error screens. Synthetic data only.",
  );
} finally {
  await browser.close();
  await server.close();
}
