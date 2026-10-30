// 拖图进题库（CONTRACT §10.3）的纯函数：路径扩展名判定、拖放路径分拣、高亮状态。
// QuestionBank.vue 只负责订阅 Tauri webview 的 drag-drop 事件并把 paths 交给 store。

/** §10.3 原文：只认这五种扩展名（大小写不敏感）。 */
export const IMAGE_EXTENSIONS: readonly string[] = ["png", "jpg", "jpeg", "webp", "gif"];
/** §10.3 原文：拖进来的文件里有非图片时提示一次。 */
export const NON_IMAGE_TOAST = "只接受图片文件";

/** 路径最后一段的扩展名（小写、不含点）；没有扩展名、点结尾或 `.DS_Store` 这类点开头无扩展名的 → ""。同时认 `/` 与 `\`。 */
export function pathExtension(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  if (dot <= 0 || dot === base.length - 1) return "";
  return base.slice(dot + 1).toLowerCase();
}

/** §10.3：扩展名为 png / jpg / jpeg / webp / gif（大小写不敏感）才算图片。 */
export function isImagePath(path: string): boolean {
  return IMAGE_EXTENSIONS.includes(pathExtension(path));
}

export interface DropSplit { images: string[]; others: string[] }

/** 把一次拖放的 paths 分成图片与其它：保持出现顺序、同一路径只留第一个、空串丢弃。 */
export function splitDropPaths(paths: readonly string[]): DropSplit {
  const images: string[] = [];
  const others: string[] = [];
  const seen = new Set<string>();
  for (const p of paths) {
    if (typeof p !== "string" || !p || seen.has(p)) continue;
    seen.add(p);
    (isImagePath(p) ? images : others).push(p);
  }
  return { images, others };
}

/**
 * drag-drop 事件类型 → 题库面板是否高亮。`enter` / `over` 高亮，`drop` / `leave`（老版本还有 `cancel`）以及
 * 任何不认识的类型都取消高亮（§10.3：over 高亮，leave / cancel 取消）。
 */
export function isDragActive(type: string): boolean {
  return type === "enter" || type === "over";
}
