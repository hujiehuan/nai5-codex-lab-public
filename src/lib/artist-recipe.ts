// 画师串解析器。移植自 langbai/src/artist-recipe.ts（MIT），只保留与本应用相关的
// 解析 / 归一 / 序列化部分；随机配方生成等逻辑不在此。
import type { ArtistWeighted, TokenKind } from "../types";

export type RecipeTokenKind = TokenKind;

export interface ParsedRecipeToken {
  raw: string;
  value: string;
  weight: number;
  kind: RecipeTokenKind;
}

export const QUALITY_PATTERN = /^(masterpiece|best quality|amazing quality|very aesthetic|extremely detailed(?: cg)?|ultra[- ]?detailed|high quality)$/i;
export const YEAR_PATTERN = /^year[_ ]?\d{4}$/i;
export const NEGATIVE_PATTERN = /^(no\s+|negative\s+|avoid\s+|-\d+(?:\.\d+)?::)/i;
export const STYLE_PATTERN = /(style|realism|impasto|illustration|painting|lineart|lighting|shading|cg|photorealistic|monochrome|sketch|watercolor)/i;

/** 已知的画师别名（Danbooru 改名），键与值都是 canonical 形式。 */
export const ARTIST_TAG_ALIASES: Readonly<Record<string, string>> = {
  "channel_(_caststation)": "channel_(caststation)",
  "machi_(7769)": "machi_(machi0910)",
};

export function roundWeight(value: number): number {
  return Math.round(Math.max(-10, Math.min(10, value)) * 100) / 100;
}

export function canonicalArtistTagName(raw: string): string {
  const normalized = raw
    .normalize("NFKC")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, "_");
  return ARTIST_TAG_ALIASES[normalized] ?? normalized;
}

export function classify(value: string): RecipeTokenKind {
  const normalized = value.trim();
  if (/^artist\s*:/i.test(normalized)) return "artist";
  if (YEAR_PATTERN.test(normalized)) return "year";
  if (QUALITY_PATTERN.test(normalized)) return "quality";
  if (NEGATIVE_PATTERN.test(normalized)) return "negative";
  if (STYLE_PATTERN.test(normalized)) return "style";
  return "other";
}

/**
 * 同时解析旧式花括号/方括号强调与 V4 数值强调。全角标点会被归一，
 * 但调用方保留原文，方便随时回显原串。
 */
export function parseArtistRecipe(input: string): ParsedRecipeToken[] {
  const source = input.normalize("NFKC").replace(/\u00a0/g, " ").replace(/[，、；;]/g, ",");
  const output: ParsedRecipeToken[] = [];
  let token = "";
  let curly = 0;
  let square = 0;
  let startingLevel: number | null = null;
  let numericWeight: number | null = null;

  const flush = () => {
    let raw = token.trim().replace(/^,+|,+$/g, "").trim();
    token = "";
    if (!raw || raw === "::") {
      startingLevel = null;
      return;
    }
    const numericStart = raw.match(/^(-?\d+(?:\.\d+)?)\s*::\s*([\s\S]*)$/);
    if (numericStart) {
      numericWeight = Number(numericStart[1]);
      raw = numericStart[2].trim();
    }
    const closesNumeric = raw.endsWith("::");
    if (closesNumeric) raw = raw.slice(0, -2).trim();
    if (raw) {
      const weight = numericWeight ?? Math.pow(1.05, startingLevel ?? (curly - square));
      const value = raw.replace(/^\((.*)\)$/s, "$1").trim();
      output.push({ raw, value, weight: roundWeight(weight), kind: weight < 0 ? "negative" : classify(value) });
    }
    if (closesNumeric) numericWeight = null;
    startingLevel = null;
  };

  for (const char of source) {
    if (char === "{") { curly += 1; continue; }
    if (char === "}") { curly = Math.max(0, curly - 1); continue; }
    if (char === "[") { square += 1; continue; }
    if (char === "]") { square = Math.max(0, square - 1); continue; }
    if (char === ",") { flush(); continue; }
    if (startingLevel === null && !/\s/.test(char)) startingLevel = curly - square;
    token += char;
  }
  flush();
  return output;
}

/** 权重 1 输出原值，否则 `w::value ::`（原项目格式，:: 前带空格）。 */
export function formatToken(token: Pick<ParsedRecipeToken, "value" | "weight">): string {
  if (Math.abs(token.weight - 1) < 0.001) return token.value;
  return `${roundWeight(token.weight)}::${token.value} ::`;
}

// ---- 以下为本应用自己的补充（对应 CONTRACT §3.1，前端只用于展示 / 复制） ----

/** 解析前剥掉 `<artist>` / `<style>` 标记与换行。 */
export function stripRecipeMarkup(input: string): string {
  return input.replace(/<\/?(artist|style)>/gi, " ").replace(/[\r\n]+/g, ", ");
}

/**
 * 从 tags 串里取画师（去 `artist:` 前缀、canonical），保持原顺序，
 * 同名只保留首次出现；负权重画师不算。
 */
export function extractArtists(input: string, options: { bareNames?: boolean } = {}): ArtistWeighted[] {
  const seen = new Set<string>();
  const output: ArtistWeighted[] = [];
  for (const token of parseArtistRecipe(stripRecipeMarkup(input))) {
    if (token.weight <= 0) continue;
    const isArtist = token.kind === "artist" || (options.bareNames === true && token.kind === "other");
    if (!isArtist) continue;
    const name = canonicalArtistTagName(token.value.replace(/^artist\s*:/i, ""));
    if (!name || seen.has(name)) continue;
    seen.add(name);
    output.push({ name, weight: token.weight });
  }
  return output;
}

/** 权重最多两位小数，去尾零（`1.50` → `1.5`，`2.00` → `2`）。 */
export function formatPoolWeight(weight: number): string {
  return String(Number(roundWeight(weight).toFixed(2)));
}

/** §3.1 的 core：`artist:name` 或 `w::artist:name::`，`, ` 连接。 */
export function formatArtistPoolCore(artists: readonly ArtistWeighted[]): string {
  return artists
    .map((artist) => {
      const name = canonicalArtistTagName(artist.name);
      return Math.abs(artist.weight - 1) < 0.001
        ? `artist:${name}`
        : `${formatPoolWeight(artist.weight)}::artist:${name}::`;
    })
    .join(", ");
}

export const ARTIST_POOL_SUFFIX = "amazing quality, very aesthetic, absurdres, masterpiece, amazing, best quality, -2::artist collaboration::";

/** §3.1 输出行：core + 固定质量后缀。core 为空返回空串。 */
export function formatArtistPoolLine(artists: readonly ArtistWeighted[]): string {
  const core = formatArtistPoolCore(artists);
  return core ? `${core}, ${ARTIST_POOL_SUFFIX}` : "";
}
