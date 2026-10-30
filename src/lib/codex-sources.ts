/** 所长 N5 法典（v0.5.16）：随图库一起同步，在「所长法典」页按本切换浏览、复制、标记；不推送到插件。 */
export const SUOZHANG_REGULAR_ID = "suozhang_nai5";
export const SUOZHANG_SITE_URL =
  "https://novelai.quicktagcloud.com/?c=suozhang_nai5";

export const SUOZHANG_SOURCES = [
  { id: SUOZHANG_REGULAR_ID, label: "N5 常规" },
] as const;

export function isSuozhangBook(id: unknown): boolean {
  return SUOZHANG_SOURCES.some((source) => source.id === id);
}
