// 散文提示词判定（CONTRACT §11.1；与 Rust `artist::is_prose_prompt` / 插件 `lab_sync.is_prose_prompt` 同一语义的 TS 移植）。
// 社区图包里有一批条目不是逗号分隔的标签，而是 XML 结构（`<artwork>…`、`<scene>…</scene>`）、代码围栏或整段自然语言。
// 题库（QuestionBank.vue）用它给散文条目打"散文"徽标、做"只看标签题"筛选；批次里每条 run 的 truth_kind 由后端判，前端不复算。
// - `stripArtistBlock`：删除 `<artist>…</artist>`（大小写不敏感、跨行；没有闭合标签的原样保留，不猜到哪结束）。
// - `isProsePrompt`：对 strip 后文本，满足任一即 true：含 `<artwork>`；含 "```"；存在独立成行的 XML 标签
//   `^\s*<[a-z_]+>\s*$`（`<style>` 除外；只认小写标签名与不带属性的形式，照契约原文）；
//   按 `[,\n]` 切分后非空 token 的平均词数 > 5。`<style>…</style>` 的内容按标签处理：只去掉标签本身，内容照常参与切分。
import type { Question } from "../types";

/** 平均词数阈值（§11.1：> 5 为散文）。 */
export const PROSE_AVG_WORDS = 5;

const ARTIST_BLOCK = /<artist>[\s\S]*?<\/artist>/gi;
/** 契约原文 `^\s*<[a-z_]+>\s*$`（多行）；捕获组 1 是标签名。 */
const XML_LINE = /^\s*<([a-z_]+)>\s*$/gm;
const STYLE_TAG = /<\/?style>/gi;

/** 删除 `<artist>…</artist>`（大小写不敏感、跨行、多块都删）。 */
export function stripArtistBlock(t: string): string {
  return t.replace(ARTIST_BLOCK, "");
}

/** 去掉 `<style>` / `</style>` 标签本身（内容保留）。 */
export function stripStyleTags(t: string): string {
  return t.replace(STYLE_TAG, "");
}

/** 按 `[,\n]` 切分后非空 token 的平均词数（空白分词）；没有 token 为 0。 */
export function avgWords(t: string): number {
  let tokens = 0;
  let words = 0;
  for (const raw of t.split(/[,\n]/)) {
    const tok = raw.trim();
    if (!tok) continue;
    tokens += 1;
    words += tok.split(/\s+/).length;
  }
  return tokens === 0 ? 0 : words / tokens;
}

/** `isProsePrompt` 命中的那条规则（按 §11.1 的顺序判），给徽标 title / 测试用。 */
export type ProseKind = "artwork" | "fence" | "xml_line" | "long_tokens";
export const PROSE_KIND_LABEL: Record<ProseKind, string> = {
  artwork: "含 <artwork> 块",
  fence: "含代码围栏",
  xml_line: "有独立成行的 XML 标签",
  long_tokens: "平均每个 token 超过 5 个词",
};

/** 散文判定的诊断版本：null = 标签题。 */
export function proseKind(t: string): ProseKind | null {
  const s = stripArtistBlock(t);
  if (s.includes("<artwork>")) return "artwork";
  if (s.includes("```")) return "fence";
  for (const m of s.matchAll(XML_LINE)) {
    if (m[1] !== "style") return "xml_line";
  }
  if (avgWords(stripStyleTags(s)) > PROSE_AVG_WORDS) return "long_tokens";
  return null;
}

/** CONTRACT §11.1 `is_prose_prompt`。 */
export function isProsePrompt(t: string): boolean {
  return proseKind(t) !== null;
}

/**
 * `Question.truth_preview` 是归一 token 用 ", " 连接、截 200 字的串（后端 `reverse_questions`），不是 tags 原文；
 * 把每个 token 还原成一行，让"独立成行的 XML 标签"规则能在 token 级别生效（token 里不会再有逗号）。
 */
export function previewAsLines(preview: string): string {
  return preview.split(",").map((s) => s.trim()).filter(Boolean).join("\n");
}

/**
 * 题库条目是不是散文题：后端给了 `prose`（前端向契约追加的可选键，见 docs/CONTRACT_CHANGES.md）就用它；
 * 否则从 `truth_preview` 退化判定——归一（小写、`_`→空格、去权重）与截 200 字会让少数条目漏判，
 * 批次跑完后以 `RunRow.truth_kind` 为准。
 */
export function questionIsProse(q: Pick<Question, "truth_preview" | "prose">): boolean {
  if (typeof q.prose === "boolean") return q.prose;
  return isProsePrompt(previewAsLines(q.truth_preview));
}
