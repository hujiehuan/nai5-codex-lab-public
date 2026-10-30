// Source guards for the requirements most likely to regress during UI edits.
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
const problems = [];
let files = 0;
async function walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) await walk(path);
    else if (path.endsWith(".vue")) {
      files++;
      const text = await readFile(path, "utf8");
      const styles = [...text.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style>/g)]
        .map((m) => m[1])
        .join("\n");
      if (/#[\da-f]{3,8}\b|rgba?\(|hsla?\(/i.test(styles))
        problems.push(`${path}: color outside semantic tokens`);
      if (/\bpad\??\s*:\s*boolean/.test(text))
        problems.push(`${path}: use default-false flush prop instead of pad`);
      // Duplicate class names are valid for responsive overrides; report the dangerous
      // combination from the previous bug: an opacity rule applied to a grid container.
      for (const m of styles.matchAll(/([^{}]+)\{([^{}]*)\}/g))
        if (
          m[1].trim() === ".grid" &&
          /opacity\s*:\s*(?:0?\.\d|0\b)/.test(m[2])
        )
          problems.push(`${path}: grid opacity affects every child`);
    }
  }
}
await walk("src");
if (problems.length) {
  console.error(problems.join("\n"));
  process.exitCode = 1;
} else
  console.log(
    `PASS: ${files} Vue components use color tokens; no ambiguous pad prop or faded grid rule.`,
  );
