// dnd.ts 纯函数：扩展名判定、拖放路径分拣、高亮状态（CONTRACT §10.3）。
import { describe, expect, it } from "vitest";
import { IMAGE_EXTENSIONS, NON_IMAGE_TOAST, isDragActive, isImagePath, pathExtension, splitDropPaths } from "./dnd";

describe("pathExtension", () => {
  it("取最后一段的扩展名并小写", () => {
    expect(pathExtension("/Users/a/Pictures/cat.PNG")).toBe("png");
    expect(pathExtension("C:\\Users\\a\\cat.Jpeg")).toBe("jpeg");
    expect(pathExtension("cat.webp")).toBe("webp");
  });
  it("多个点只看最后一个", () => {
    expect(pathExtension("/a/b/archive.tar.gz")).toBe("gz");
    expect(pathExtension("/a/b/2026.09.07.jpg")).toBe("jpg");
  });
  it("没有扩展名、点开头、点结尾都算空", () => {
    expect(pathExtension("/a/b/Makefile")).toBe("");
    expect(pathExtension("/a/b/.DS_Store")).toBe("");
    expect(pathExtension("/a/b/.png")).toBe("");
    expect(pathExtension("/a/b/weird.")).toBe("");
    expect(pathExtension("")).toBe("");
  });
  it("目录名里的点不算扩展名", () => {
    expect(pathExtension("/a/b.png/file")).toBe("");
  });
});

describe("isImagePath", () => {
  it("五种扩展名大小写不敏感", () => {
    for (const ext of IMAGE_EXTENSIONS) {
      expect(isImagePath(`/x/y.${ext}`)).toBe(true);
      expect(isImagePath(`/x/y.${ext.toUpperCase()}`)).toBe(true);
    }
  });
  it("其它一律不是", () => {
    for (const p of ["/x/y.txt", "/x/y.bmp", "/x/y.tiff", "/x/y.svg", "/x/y.psd", "/x/y", "/x/.png", "/x/y.png.zip", ""]) {
      expect(isImagePath(p), p).toBe(false);
    }
  });
});

describe("splitDropPaths", () => {
  it("图片与其它分开，保持出现顺序", () => {
    const r = splitDropPaths(["/a/1.png", "/a/notes.txt", "/a/2.JPG", "/a/dir", "/a/3.gif"]);
    expect(r.images).toEqual(["/a/1.png", "/a/2.JPG", "/a/3.gif"]);
    expect(r.others).toEqual(["/a/notes.txt", "/a/dir"]);
  });
  it("同一路径只留第一个，空串丢弃", () => {
    const r = splitDropPaths(["/a/1.png", "", "/a/1.png", "/a/x.txt", "/a/x.txt"]);
    expect(r.images).toEqual(["/a/1.png"]);
    expect(r.others).toEqual(["/a/x.txt"]);
  });
  it("大小写不同的路径视为不同（文件系统的事，这里不猜）", () => {
    const r = splitDropPaths(["/a/1.png", "/a/1.PNG"]);
    expect(r.images).toEqual(["/a/1.png", "/a/1.PNG"]);
  });
  it("空输入 → 两边都空", () => {
    expect(splitDropPaths([])).toEqual({ images: [], others: [] });
  });
  it("不改入参", () => {
    const input = ["/a/1.png", "/a/x.txt"];
    splitDropPaths(input);
    expect(input).toEqual(["/a/1.png", "/a/x.txt"]);
  });
  it("提示文案是 §10.3 原文", () => {
    expect(NON_IMAGE_TOAST).toBe("只接受图片文件");
  });
});

describe("isDragActive", () => {
  it("enter / over 高亮，drop / leave / cancel / 未知都取消", () => {
    expect(isDragActive("enter")).toBe(true);
    expect(isDragActive("over")).toBe(true);
    expect(isDragActive("drop")).toBe(false);
    expect(isDragActive("leave")).toBe(false);
    expect(isDragActive("cancel")).toBe(false);
    expect(isDragActive("")).toBe(false);
  });
});
