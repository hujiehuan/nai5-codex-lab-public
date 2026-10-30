// codex store 的两处最有逻辑的地方：setDecision 失败回滚、reload 的 seq 竞态。
// 不依赖 DOM：mock 掉 ../api，用 createPinia + setActivePinia 在 node 环境直接跑。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { Book, Decision, EntryCard, EntryPage } from "../types";

vi.mock("../api", () => ({
  api: {
    listEntries: vi.fn(),
    setDecision: vi.fn(),
    listBooks: vi.fn(),
    appInfo: vi.fn(),
    settingsGet: vi.fn(),
  },
  errMessage: (e: unknown) => (e instanceof Error ? e.message : String(e)),
  Channel: class {},
  convertFileSrc: (p: string) => p,
}));

import { api } from "../api";
import { useCodex } from "./codex";
import { useToast } from "./toast";

function card(id: string, decision: Decision): EntryCard {
  return {
    entry_id: id, codex_id: "book", book_order: 0, title: id, tags_first_line: "", rating: "safe", batches: [], path: [],
    decision, thumb_path: null, image_path: null, image_w: null, image_h: null, image_count: 1, dup_count: 1, is_new: false,
  };
}

function page(items: EntryCard[]): EntryPage {
  const counts = { include: 0, exclude: 0, undecided: 0 };
  for (const it of items) counts[it.decision] += 1;
  return { total: items.length, counts, items };
}

function book(counts: Book["counts"]): Book {
  return { codex_id: "book", type: "string", title: "书", version: "1", author: null, entry_count: counts.total, tree: [], update_filters: [], counts };
}

/** 手动控制先后顺序的 Promise */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

beforeEach(() => {
  setActivePinia(createPinia());
  vi.mocked(api.listEntries).mockReset();
  vi.mocked(api.setDecision).mockReset();
  vi.mocked(api.listBooks).mockReset().mockResolvedValue([]);
});

describe("codex store · setDecision 乐观更新与回滚", () => {
  it("失败时按条回滚，计数减回去，并弹错误提示", async () => {
    const codex = useCodex();
    codex.items = [card("a", "undecided"), card("b", "include")];
    codex.counts = { include: 1, exclude: 0, undecided: 1 };
    vi.mocked(api.setDecision).mockRejectedValueOnce(new Error("db locked"));

    const ok = await codex.setDecision(["a"], "include", "key");

    expect(ok).toBe(false);
    expect(codex.items.map((it) => it.decision)).toEqual(["undecided", "include"]);
    expect(codex.counts).toEqual({ include: 1, exclude: 0, undecided: 1 });
    expect(useToast().items.some((t) => t.kind === "error" && t.message.includes("已回滚"))).toBe(true);
    expect(api.listBooks).not.toHaveBeenCalled();
  });

  it("并发两次、先发的那次失败：只回滚它自己的改动，不吞掉第二次的计数", async () => {
    const codex = useCodex();
    codex.items = [card("a", "undecided"), card("b", "undecided")];
    codex.counts = { include: 0, exclude: 0, undecided: 2 };
    const first = deferred<{ changed: number }>();
    const second = deferred<{ changed: number }>();
    vi.mocked(api.setDecision).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const p1 = codex.setDecision(["a"], "include", "key");
    const p2 = codex.setDecision(["b"], "exclude", "key");
    // 两次乐观更新都已落到本地
    expect(codex.items.map((it) => it.decision)).toEqual(["include", "exclude"]);
    expect(codex.counts).toEqual({ include: 1, exclude: 1, undecided: 0 });

    second.resolve({ changed: 1 });
    await p2;
    first.reject(new Error("boom"));
    await p1;

    expect(codex.items.map((it) => it.decision)).toEqual(["undecided", "exclude"]);
    expect(codex.counts).toEqual({ include: 0, exclude: 1, undecided: 1 });
  });

  it("回滚时跳过已被别人改走的条目", async () => {
    const codex = useCodex();
    codex.items = [card("a", "undecided")];
    codex.counts = { include: 0, exclude: 0, undecided: 1 };
    const first = deferred<{ changed: number }>();
    vi.mocked(api.setDecision).mockReturnValueOnce(first.promise).mockResolvedValueOnce({ changed: 1 });

    const p1 = codex.setDecision(["a"], "include", "key");
    await codex.setDecision(["a"], "exclude", "key"); // 期间又改成了 exclude 且成功
    first.reject(new Error("boom"));
    await p1;

    expect(codex.items[0].decision).toBe("exclude");
    expect(codex.counts).toEqual({ include: 0, exclude: 1, undecided: 0 });
  });

  it("成功后重拉书目，侧栏计数跟着刷新", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    codex.books = [book({ include: 0, exclude: 0, undecided: 1, total: 1 })];
    codex.items = [card("a", "undecided")];
    codex.counts = { include: 0, exclude: 0, undecided: 1 };
    vi.mocked(api.setDecision).mockResolvedValueOnce({ changed: 1 });
    vi.mocked(api.listBooks).mockResolvedValueOnce([book({ include: 1, exclude: 0, undecided: 0, total: 1 })]);

    expect(await codex.setDecision(["a"], "include", "click")).toBe(true);
    await flush();

    expect(api.listBooks).toHaveBeenCalledTimes(1);
    expect(codex.book?.counts.include).toBe(1);
  });

  it("连按时 list_books 合并：进行中只补跑一次", async () => {
    const codex = useCodex();
    codex.items = [card("a", "undecided"), card("b", "undecided"), card("c", "undecided")];
    codex.counts = { include: 0, exclude: 0, undecided: 3 };
    vi.mocked(api.setDecision).mockResolvedValue({ changed: 1 });
    const slow = deferred<Book[]>();
    vi.mocked(api.listBooks).mockReturnValueOnce(slow.promise).mockResolvedValue([]);

    await codex.setDecision(["a"], "include", "key");
    await codex.setDecision(["b"], "include", "key");
    await codex.setDecision(["c"], "include", "key");
    expect(api.listBooks).toHaveBeenCalledTimes(1);
    slow.resolve([]);
    await flush();

    expect(api.listBooks).toHaveBeenCalledTimes(2);
  });
});

describe("codex store · reload 竞态", () => {
  it("换画师来源时清空旧卡片和选择，新来源读取失败也不会混入旧书", async () => {
    const codex = useCodex();
    codex.codexId = "artist_nai5_personal";
    codex.books = [book({ include: 1, exclude: 0, undecided: 0, total: 1 })];
    codex.items = [card("old", "include")];
    codex.total = 1;
    codex.counts = { include: 1, exclude: 0, undecided: 0 };
    codex.selected = new Set(["old"]);
    codex.drawerEntryId = "old";
    const next = deferred<EntryPage>();
    vi.mocked(api.listEntries).mockReturnValueOnce(next.promise);

    const opened = codex.open("nax_v5_artist");
    await flush();
    expect(codex.codexId).toBe("nax_v5_artist");
    expect(codex.items).toEqual([]);
    expect(codex.total).toBe(0);
    expect(codex.selectedCount).toBe(0);
    expect(codex.drawerEntryId).toBeNull();
    next.reject(new Error("NAX read failed"));
    await opened;
    expect(codex.items).toEqual([]);
    expect(codex.listError).toBe("NAX read failed");
  });

  it("NAX 本次新增使用自己的快照 release，不会被法典的 release 清掉", async () => {
    const codex = useCodex();
    codex.codexId = "nax_v5_artist";
    codex.books = [{ ...book({ include: 0, exclude: 0, undecided: 1, total: 1 }), codex_id: "nax_v5_artist", update_filters: [{ id: "nax-v5-current", label: "NAX 当前同步", latest: true }] }];
    codex.filter = { ...codex.filter, first_seen_release: "nax-v5-current" };
    vi.mocked(api.appInfo).mockResolvedValueOnce({ version: "1", latest_release: "codex-release", app_data_dir: "/tmp", db_path: "/tmp/library.db" });
    vi.mocked(api.listEntries).mockResolvedValueOnce(page([card("new", "undecided")]));
    await codex.open("nax_v5_artist");

    expect(codex.latestRelease).toBe("codex-release");
    expect(codex.currentRelease).toBe("nax-v5-current");
    expect(codex.filter.first_seen_release).toBe("nax-v5-current");
    expect(api.listEntries).toHaveBeenLastCalledWith(expect.objectContaining({ codex_id: "nax_v5_artist", first_seen_release: "nax-v5-current" }));
  });

  it("晚到的旧请求不覆盖新结果，也不动 loading", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    const older = deferred<EntryPage>();
    const newer = deferred<EntryPage>();
    vi.mocked(api.listEntries).mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);

    const p1 = codex.reload();
    const p2 = codex.reload();
    expect(codex.loading).toBe(true);

    newer.resolve(page([card("new", "undecided")]));
    await p2;
    expect(codex.items.map((it) => it.entry_id)).toEqual(["new"]);
    expect(codex.loading).toBe(false);

    older.resolve(page([card("old", "include")]));
    await p1;
    expect(codex.items.map((it) => it.entry_id)).toEqual(["new"]);
    expect(codex.counts).toEqual({ include: 0, exclude: 0, undecided: 1 });
    expect(codex.loading).toBe(false);
  });

  it("旧请求失败不弹错", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    const older = deferred<EntryPage>();
    vi.mocked(api.listEntries).mockReturnValueOnce(older.promise).mockResolvedValueOnce(page([]));

    const p1 = codex.reload();
    await codex.reload();
    older.reject(new Error("stale"));
    await p1;

    expect(useToast().items).toEqual([]);
    expect(codex.loading).toBe(false);
  });

  it("新请求失败才弹错，并且 loading 归零", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    vi.mocked(api.listEntries).mockRejectedValueOnce(new Error("db gone"));

    await codex.reload();

    expect(useToast().items.some((t) => t.kind === "error" && t.message.includes("db gone"))).toBe(true);
    expect(codex.loading).toBe(false);
  });
});

describe("codex store · 显式分页", () => {
  function pageOf(total: number, items: EntryCard[]): EntryPage {
    return { total, counts: { include: 0, exclude: 0, undecided: total }, items };
  }

  it("筛选变化时 page 归 1，并从 offset 0 重拉", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    codex.filter = { ...codex.filter, page: 4, page_size: 50 };
    vi.mocked(api.listEntries).mockResolvedValue(pageOf(500, [card("a", "undecided")]));

    codex.setFilter({ keyword: "inkwell" });
    await flush();

    expect(codex.filter.page).toBe(1);
    expect(codex.filter.page_size).toBe(50);
    expect(api.listEntries).toHaveBeenLastCalledWith(expect.objectContaining({ offset: 0, limit: 50, keyword: "inkwell" }));
  });

  it("只翻页不动筛选：offset 跟着页码走，焦点落到第一张", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    codex.total = 500;
    codex.focus = 7;
    vi.mocked(api.listEntries).mockResolvedValue(pageOf(500, [card("a", "undecided"), card("b", "undecided")]));

    codex.nextPage();
    await flush();

    expect(codex.filter.page).toBe(2);
    expect(api.listEntries).toHaveBeenLastCalledWith(expect.objectContaining({ offset: 100, limit: 100 }));
    expect(codex.focus).toBe(0);
  });

  it("total 变小导致当前页越界：回最后一页重拉", async () => {
    const codex = useCodex();
    codex.codexId = "book";
    codex.filter = { ...codex.filter, page: 5, page_size: 100 };
    vi.mocked(api.listEntries)
      .mockResolvedValueOnce(pageOf(120, []))
      .mockResolvedValueOnce(pageOf(120, [card("z", "undecided")]));

    await codex.reload();

    expect(codex.filter.page).toBe(2);
    expect(api.listEntries).toHaveBeenCalledTimes(2);
    expect(api.listEntries).toHaveBeenLastCalledWith(expect.objectContaining({ offset: 100, limit: 100 }));
    expect(codex.items.map((it) => it.entry_id)).toEqual(["z"]);
    expect(codex.pageCount).toBe(2);
    expect(codex.focus).toBe(0);
  });
});
