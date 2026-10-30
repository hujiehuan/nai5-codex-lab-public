import puppeteer from "puppeteer-core";
import assert from "node:assert/strict";
import { createServer as viteServer } from "vite";
import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { join, extname, resolve } from "node:path";
import { answer } from "../mock/server.mjs";
import { executablePath } from "./chrome.mjs";
const baseline = process.argv.includes("--baseline");
const dist = resolve(process.env.BASELINE_DIST || "dist");
const out = process.env.SMOKE_OUT || "/tmp/nai5-desktop-checks";
await mkdir(out, { recursive: true });
let server, port;
if (baseline) {
  server = createServer(async (req, res) => {
    try {
      if (req.url === "/__mock__/invoke") {
        let raw = "";
        for await (const x of req) raw += x;
        const { command, args } = JSON.parse(raw);
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify(await answer(command, args)));
        return;
      }
      const path = resolve(
        dist,
        new URL(req.url, "http://local").pathname === "/"
          ? "index.html"
          : new URL(req.url, "http://local").pathname.slice(1),
      );
      if (!path.startsWith(dist + "/")) throw Error("path");
      res.setHeader(
        "Content-Type",
        { ".js": "text/javascript", ".css": "text/css", ".html": "text/html" }[
          extname(path)
        ] || "application/octet-stream",
      );
      res.end(await readFile(path));
    } catch (e) {
      res.statusCode = 400;
      res.end(JSON.stringify({ code: "mock", message: String(e) }));
    }
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  port = server.address().port;
} else {
  server = await viteServer({
    mode: "mock",
    server: { port: 0, host: "127.0.0.1", strictPort: false },
  });
  await server.listen();
  port = server.httpServer.address().port;
}
const browser = await puppeteer.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"],
});
const results = [];
try {
  for (const width of [1280, 700])
    for (const theme of ["light", "dark"]) {
      const page = await browser.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.setViewport({ width, height: 900 });
      await page.emulateMediaFeatures([
        { name: "prefers-color-scheme", value: theme },
      ]);
      await page.evaluateOnNewDocument(() => {
        let id = 0;
        window.__TAURI_INTERNALS__ = {
          metadata: {
            currentWindow: { label: "main" },
            currentWebview: { label: "main" },
          },
          transformCallback: () => ++id,
          unregisterCallback: () => {},
          convertFileSrc: (p) => p,
          invoke: async (command, args) => {
            const r = await fetch("/__mock__/invoke", {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ command, args }),
            });
            if (!r.ok) throw await r.json();
            return r.json();
          },
        };
      });
      const routes = baseline
        ? ["/sync", "/settings", "/reverse"]
        : process.env.SMOKE_ROUTES?.split(",") || [
            "/sync",
            "/settings",
            "/reverse",
            "/codex/artist_nai5_personal",
            "/codex/nai5_community_pack",
            "/lab/draw",
            "/studio?mode=review",
          ];
      for (const route of routes) {
        await page.goto(
          `http://127.0.0.1:${port}/?screen=${encodeURIComponent(route)}#${route}`,
          { waitUntil: "networkidle0" },
        );
        await page.waitForFunction(
          () =>
            document
              .querySelector(".router-link-active")
              ?.getAttribute("href")?.split("?")[0] === location.hash.split("?")[0],
        );
        if (!baseline && ["/reverse", "/lab/draw"].includes(route)) {
          const mode = { "/reverse": "reverse", "/lab/draw": "draw" }[route];
          assert(page.url().endsWith(`/studio?mode=${mode}`), `Legacy route must redirect: ${route}`);
          assert(await page.$('[data-testid="studio"]'), "Total studio must be rendered");
          assert(await page.$eval('.studio', (e) => e.textContent.includes('查看旧版本实验记录')), "Read-only legacy history remains accessible");
        }
        const metrics = await page.evaluate(() => {
          const visible = (e) => {
            const r = e.getBoundingClientRect(),
              s = getComputedStyle(e);
            return e.checkVisibility() && r.width > 0 && r.height > 0 && s.visibility !== "hidden";
          };
          const color = (s) => (s.match(/[\d.]+/g) || []).map(Number);
          const blend = (a, b) => {
            const t = a[3] ?? 1;
            return [0, 1, 2].map((i) => (a[i] ?? 0) * t + b[i] * (1 - t));
          };
          const lum = (c) =>
            c
              .slice(0, 3)
              .map((v) => {
                v /= 255;
                return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
              })
              .reduce((a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i], 0);
          const contrast = [];
          const tw = document.createTreeWalker(
            document.body,
            NodeFilter.SHOW_TEXT,
          );
          let node;
          while ((node = tw.nextNode())) {
            const e = node.parentElement;
            if (
              !node.textContent.trim() ||
              !e ||
              !visible(e) ||
              ["SCRIPT", "STYLE"].includes(e.tagName) ||
              e.closest("[disabled],.n-button--disabled")
            )
              continue;
            const parents = [];
            for (let p = e; p; p = p.parentElement) parents.push(p);
            if (parents.some((p) => Number(getComputedStyle(p).opacity) < 0.99))
              continue;
            let bg = [255, 255, 255];
            for (const p of parents.reverse())
              bg = blend(color(getComputedStyle(p).backgroundColor), bg);
            const s = getComputedStyle(e),
              fg = blend(color(s.color), bg);
            const a = lum(fg),
              b = lum(bg),
              ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05),
              large =
                parseFloat(s.fontSize) >= 24 ||
                (parseFloat(s.fontSize) >= 18.66 &&
                  parseInt(s.fontWeight) >= 700);
            if (ratio < (large ? 3 : 4.5) - 0.01)
              contrast.push({
                text: node.textContent.trim().slice(0, 40),
                ratio: +ratio.toFixed(2),
                class: e.className,
              });
          }
          const targets = [
            ...document.querySelectorAll(
              'button,a,input,select,textarea,[role="button"]',
            ),
          ]
            .filter(visible)
            .filter((e) => !e.disabled)
            .map((e) => ({
              label: (
                e.getAttribute("aria-label") ||
                e.textContent ||
                e.getAttribute("placeholder") ||
                e.tagName
              )
                .trim()
                .slice(0, 35),
              width: e.getBoundingClientRect().width,
              height: e.getBoundingClientRect().height,
            }))
            .filter((r) => r.width < 24 || r.height < 24);
          const main = document.querySelector("main"),
            r = main?.getBoundingClientRect();
          return {
            documentOverflow: document.documentElement.scrollWidth - innerWidth,
            mainOverflow: main ? main.scrollWidth - main.clientWidth : 0,
            targets,
            contrast,
            mainLeft: r?.left,
            bodyText: document.body.innerText.length,
          };
        });
        results.push({ width, theme, route, ...metrics, errors: [...errors] });
        await page.screenshot({
          path: join(
            out,
            `${baseline ? "before" : "after"}-${width}-${theme}-${route.replaceAll("/", "_")}.png`,
          ),
          fullPage: true,
        });
      }
      await page.close();
    }
} finally {
  await browser.close();
  await server.close();
}
await writeFile(
  join(out, baseline ? "baseline.json" : "layout.json"),
  JSON.stringify(results, null, 2),
);
for (const r of results)
  console.log(
    JSON.stringify({
      width: r.width,
      theme: r.theme,
      route: r.route,
      overflow: r.documentOverflow,
      mainOverflow: r.mainOverflow,
      smallTargets: r.targets.length,
      lowContrast: r.contrast.length,
      errors: r.errors,
    }),
  );
if (
  !baseline &&
  results.some(
    (r) =>
      r.documentOverflow > 1 ||
      r.mainOverflow > 1 ||
      r.targets.length ||
      r.contrast.length ||
      r.errors.length,
  )
)
  process.exitCode = 1;
