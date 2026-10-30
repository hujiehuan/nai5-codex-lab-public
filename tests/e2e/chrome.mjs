// 无头浏览器路径：CHROME_BIN > puppeteer 缓存（~/.cache/puppeteer，Linux / macOS 布局都认）> 本机 Google Chrome。
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const MAC_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

function fromCache() {
  const cache = join(homedir(), ".cache/puppeteer/chrome-headless-shell");
  if (!existsSync(cache)) return null;
  for (const build of readdirSync(cache).sort().reverse()) {
    // linux-<ver>/chrome-headless-shell-linux64/…、mac_arm-<ver>/chrome-headless-shell-mac-arm64/…
    for (const dir of readdirSync(join(cache, build)).filter((d) => d.startsWith("chrome-headless-shell-"))) {
      const bin = join(cache, build, dir, "chrome-headless-shell");
      if (existsSync(bin)) return bin;
    }
  }
  return null;
}

export const executablePath =
  process.env.CHROME_BIN || fromCache() || (existsSync(MAC_CHROME) ? MAC_CHROME : null);
if (!executablePath)
  throw new Error("找不到浏览器：设置 CHROME_BIN，或运行 npx @puppeteer/browsers install chrome-headless-shell@stable --path ~/.cache/puppeteer");
