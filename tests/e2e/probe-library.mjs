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
  // Right click opens details; the style-dictionary drawer copies the parsed style recipe.
  await page.click(first, { button: "right" });
  await page.waitForSelector(".decisions");
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text) => {
          window.__copiedRecipe = text;
        },
      },
    });
    // The recipe copy button is the only labelled one; the tags / negative buttons read just "复制".
    [...document.querySelectorAll("button")]
      .find((e) => e.textContent.trim().startsWith("复制") && e.textContent.trim() !== "复制")
      .click();
  });
  await page.waitForFunction(() =>
    window.__copiedRecipe?.includes("synthetic_example"),
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector(".decisions", { hidden: true });
  // Filters are stored per library: the pack library starts unfiltered, the style dictionary keeps its keyword.
  await page.type('.filterbar input[type="search"]', "合成示例 1");
  await page.waitForFunction(() =>
    document.querySelector(".summary")?.textContent.includes("当前筛选 11 条"),
  );
  await page.click('.sidebar a[href="#/codex/nai5_community_pack"]');
  await page.waitForSelector('[data-entry-id="fixture-nai5_community_pack-0"]');
  assert.equal(
    await page.$eval('.filterbar input[type="search"]', (e) => e.value),
    "",
  );
  assert.equal(
    await page.$('[data-entry-id^="fixture-artist_nai5_personal"]'),
    null,
  );
  // Reload proves the persisted filter and the decisions made above both survive.
  await page.goto(base + "/#/codex/artist_nai5_personal", {
    waitUntil: "networkidle0",
  });
  await page.reload({ waitUntil: "networkidle0" });
  await page.waitForSelector(first + ".include");
  assert.equal(
    await page.$eval('.filterbar input[type="search"]', (e) => e.value),
    "合成示例 1",
  );
  assert.match(
    await page.$eval(".summary", (e) => e.textContent),
    /当前筛选 11 条/,
  );
  for (const route of [
    "/sync",
    "/settings",
    "/eval",
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
    "PASS: three-state cycle, A shortcut, Cmd-Z undo, Cmd-A and bulk include, detail/Escape, style-recipe copy, per-library filter persistence, five read-error screens. Synthetic data only.",
  );
} finally {
  await browser.close();
  await server.close();
}
