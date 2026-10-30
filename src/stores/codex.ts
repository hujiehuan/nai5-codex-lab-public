import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api, errMessage } from "../api";
import type {
  Book,
  Decision,
  DecisionCounts,
  DecisionVia,
  EntryCard,
} from "../types";
import { cycleDecision, DECISION_LABEL, shiftCounts } from "../lib/decision";
import { rangeIndices } from "../lib/grid";
import {
  applyFilterPatch,
  buildEntryQuery,
  clampPage,
  defaultFilter,
  defaultSortDir,
  effectiveSortDir,
  loadFilter,
  pageCount as countPages,
  saveFilter,
  type CodexFilter,
  type PageSize,
  type SortDir,
  type SortKey,
} from "../lib/query";
import { useSettings } from "./settings";
import { useToast } from "./toast";
import { NAX_ARTIST_ID } from "../lib/artist-sources";

export const useCodex = defineStore("codex", () => {
  const toast = useToast();
  const settingsStore = useSettings();

  const books = ref<Book[]>([]);
  const booksLoading = ref(false);
  const booksError = ref("");
  const listError = ref("");
  const latestRelease = ref<string | null>(null);

  const codexId = ref<string | null>(null);
  const filter = ref<CodexFilter>(defaultFilter());

  const items = ref<EntryCard[]>([]);
  const total = ref(0);
  const counts = ref<DecisionCounts>({ include: 0, exclude: 0, undecided: 0 });
  const loading = ref(false);
  /** 每次 reload 成功落地 +1：EntryGrid 据此回顶（替代按首条 id 猜测"是否换了筛选"的启发式）。 */
  const listVersion = ref(0);
  let seq = 0;

  const selected = ref<Set<string>>(new Set());
  const anchor = ref<number | null>(null);
  const focus = ref<number | null>(null);
  const drawerEntryId = ref<string | null>(null);

  const book = computed(
    () => books.value.find((b) => b.codex_id === codexId.value) ?? null,
  );
  /** NAX 使用自己的快照 release；法典仍沿用 app_info 的上游 release。 */
  const currentRelease = computed(() =>
    codexId.value === NAX_ARTIST_ID
      ? book.value?.update_filters.find((batch) => batch.latest)?.id ?? null
      : latestRelease.value,
  );
  const defaults = computed(() => defaultFilter());
  const selectedCount = computed(() => selected.value.size);
  const focusedEntry = computed(() =>
    focus.value === null ? null : (items.value[focus.value] ?? null),
  );
  const pageCount = computed(() =>
    countPages(total.value, filter.value.page_size),
  );
  /** 当前生效的排序方向（显式 sort_dir 或 §7 缺省）。 */
  const sortDir = computed<SortDir>(() => effectiveSortDir(filter.value));
  const sortDirIsDefault = computed(() => filter.value.sort_dir === undefined);

  async function loadBooks() {
    booksLoading.value = true;
    booksError.value = "";
    try {
      const data = await api.listBooks();
      if (!Array.isArray(data)) throw Error("书目格式错误");
      books.value = data;
    } catch (e) {
      booksError.value = errMessage(e);
      toast.error(e, "读取书目失败");
    } finally {
      booksLoading.value = false;
    }
  }

  /**
   * 重拉书目（决策后 / 同步后刷新侧栏与树上的计数）。同一时刻只跑一次：
   * 进行中再被叫只做个标记，跑完补一次，键盘连按也不会堆一串 list_books。
   */
  let booksRefreshing: Promise<void> | null = null;
  let booksDirty = false;
  function refreshBooks(): Promise<void> {
    if (booksRefreshing) {
      booksDirty = true;
      return booksRefreshing;
    }
    booksRefreshing = (async () => {
      do {
        booksDirty = false;
        await loadBooks();
      } while (booksDirty);
    })().finally(() => {
      booksRefreshing = null;
    });
    return booksRefreshing;
  }

  async function loadAppInfo() {
    try {
      latestRelease.value = (await api.appInfo()).latest_release;
    } catch {
      /* 侧栏已经报过错 */
    }
  }

  function persist() {
    if (codexId.value) saveFilter(codexId.value, filter.value);
  }

  /** 进入某本书：恢复它的筛选状态（含 page / page_size / sort / sort_dir）并拉上次那一页。 */
  async function open(id: string, opts: { firstSeen?: string | null } = {}) {
    await settingsStore.ensureLoaded();
    await Promise.all([
      books.value.length ? Promise.resolve() : loadBooks(),
      loadAppInfo(),
    ]);
    if (codexId.value !== id) {
      codexId.value = id;
      filter.value = loadFilter(id, defaults.value);
      // 换来源后不能让上一本书的卡片继续可点；新来源读取失败时也不残留旧结果。
      items.value = [];
      total.value = 0;
      counts.value = { include: 0, exclude: 0, undecided: 0 };
      selected.value = new Set();
      anchor.value = null;
      focus.value = null;
      drawerEntryId.value = null;
    }
    // 持久化的"本次新增"指向旧 release 时清掉，否则复选框看着是勾的、筛的却是上一次同步。
    // 走 applyFilterPatch：这是筛选变化，page 一并归 1；没变化时页码原样保留。
    if (
      filter.value.first_seen_release !== null &&
      filter.value.first_seen_release !== currentRelease.value
    ) {
      filter.value = applyFilterPatch(filter.value, {
        first_seen_release: null,
      });
      persist();
    }
    if (opts.firstSeen) {
      filter.value = applyFilterPatch(filter.value, {
        first_seen_release: opts.firstSeen,
      });
      persist();
    }
    await reload();
  }

  /** 同步完成后：书目（条目数、目录树、计数）与 latest_release 都可能变了，重拉；不动当前列表与选择。 */
  async function afterSync() {
    await Promise.all([refreshBooks(), loadAppInfo()]);
  }

  /**
   * 合并筛选 patch、持久化并重拉。页码联动在 applyFilterPatch：
   * 筛选 / 排序变化 → page 归 1；page_size 变化 → 按首条序号换算页码。页码变了就把焦点放到第一张。
   */
  function setFilter(patch: Partial<CodexFilter>) {
    const before = filter.value;
    filter.value = applyFilterPatch(before, patch);
    persist();
    void reload({ focusFirst: filter.value.page !== before.page });
  }

  /** 重置筛选条件；排序与每页条数不是筛选（isFilterActive 同样不看它们），保留。 */
  function resetFilter() {
    setFilter({
      decision: [],
      batch: null,
      first_seen_release: null,
      path_prefix: [],
      keyword: "",
      multi_image_only: false,
      dup_only: false,
    });
  }

  // ---- 分页 / 排序 ----
  function setPage(page: number) {
    const next = clampPage(page, total.value, filter.value.page_size);
    if (next === filter.value.page) return;
    setFilter({ page: next });
  }
  function nextPage() {
    setPage(filter.value.page + 1);
  }
  function prevPage() {
    setPage(filter.value.page - 1);
  }
  function setPageSize(size: PageSize) {
    setFilter({ page_size: size });
  }
  /** 换排序键时方向回到该键的缺省方向（§7）。 */
  function setSort(sort: SortKey) {
    if (sort === filter.value.sort) return;
    setFilter({ sort, sort_dir: undefined });
  }
  /** 升/降切换；切回该键的缺省方向时存 undefined（查询里不发 sort_dir）。 */
  function toggleSortDir() {
    const next: SortDir = sortDir.value === "asc" ? "desc" : "asc";
    setFilter({
      sort_dir: next === defaultSortDir(filter.value.sort) ? undefined : next,
    });
  }

  /**
   * 按当前 filter 拉一页。total 变小导致当前页越界时回到最后一页再拉一次。
   * focusFirst：换页后把焦点放到第一张；否则沿用旧焦点（钳到页内）。
   */
  async function reload(opts: { focusFirst?: boolean } = {}) {
    const id = codexId.value;
    if (!id) return;
    seq += 1;
    const my = seq;
    loading.value = true;
    listError.value = "";
    try {
      let page = await api.listEntries(
        buildEntryQuery(id, filter.value),
      );
      if (my !== seq) return;
      if (!Array.isArray(page.items) || typeof page.total !== "number")
        throw Error("条目响应格式错误");
      let focusFirst = opts.focusFirst ?? false;
      const last = clampPage(
        filter.value.page,
        page.total,
        filter.value.page_size,
      );
      if (last !== filter.value.page) {
        filter.value = { ...filter.value, page: last };
        persist();
        if (page.total > 0) {
          page = await api.listEntries(
            buildEntryQuery(id, filter.value),
          );
          if (my !== seq) return;
        }
        focusFirst = true;
      }
      items.value = page.items;
      total.value = page.total;
      counts.value = page.counts;
      listVersion.value += 1;
      selected.value = new Set();
      anchor.value = null;
      focus.value = page.items.length
        ? focusFirst
          ? 0
          : Math.min(focus.value ?? 0, page.items.length - 1)
        : null;
    } catch (e) {
      if (my === seq) {
        listError.value = errMessage(e);
        toast.error(e, "读取条目失败");
      }
    } finally {
      if (my === seq) loading.value = false;
    }
  }

  /** 只改本地（撤销回写、抽屉内改动后同步卡片）。 */
  function applyLocal(entryId: string, decision: Decision) {
    const idx = items.value.findIndex((it) => it.entry_id === entryId);
    if (idx < 0) return;
    const it = items.value[idx];
    if (it.decision === decision) return;
    counts.value = shiftCounts(counts.value, it.decision, decision);
    const next = items.value.slice();
    next[idx] = { ...it, decision };
    items.value = next;
  }

  /** 乐观更新，失败回滚并提示。 */
  async function setDecision(
    ids: string[],
    decision: Decision,
    via: DecisionVia,
  ): Promise<boolean> {
    if (!ids.length) return true;
    const idSet = new Set(ids);
    const prev = new Map<string, Decision>();
    let nextCounts = { ...counts.value };
    items.value = items.value.map((it) => {
      if (!idSet.has(it.entry_id) || it.decision === decision) return it;
      prev.set(it.entry_id, it.decision);
      nextCounts = shiftCounts(nextCounts, it.decision, decision);
      return { ...it, decision };
    });
    counts.value = nextCounts;
    try {
      await api.setDecision(ids, decision, via);
      void refreshBooks(); // 侧栏"加入/不加入/未定"跟着变
      return true;
    } catch (e) {
      // 按条回滚，且只回滚仍停在本次乐观值上的条目；计数用 shiftCounts 反向减回去而不是恢复快照，
      // 这样期间并发的另一次 setDecision / reload 的结果不会被一起抹掉。
      let rolled = counts.value;
      items.value = items.value.map((it) => {
        const before = prev.get(it.entry_id);
        if (before === undefined || it.decision !== decision) return it;
        rolled = shiftCounts(rolled, decision, before);
        return { ...it, decision: before };
      });
      counts.value = rolled;
      toast.error(e, "写入决策失败，已回滚");
      return false;
    }
  }

  function cycle(entryId: string, via: DecisionVia = "click") {
    const it = items.value.find((x) => x.entry_id === entryId);
    if (!it) return Promise.resolve(false);
    return setDecision([entryId], cycleDecision(it.decision), via);
  }

  async function undo() {
    try {
      const r = await api.undoDecision();
      if (!r) {
        toast.push("没有可撤销的决策", "info");
        return;
      }
      applyLocal(r.entry_id, r.restored);
      toast.push(`已撤销 → ${DECISION_LABEL[r.restored]}`, "info");
    } catch (e) {
      toast.error(e, "撤销失败");
    }
  }

  // ---- 选择 ----
  function clearSelection() {
    selected.value = new Set();
  }
  function setFocus(index: number | null) {
    focus.value = index;
  }
  function toggleSelect(index: number) {
    const id = items.value[index]?.entry_id;
    if (!id) return;
    const s = new Set(selected.value);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selected.value = s;
    anchor.value = index;
  }
  function selectRange(index: number) {
    const from = anchor.value ?? focus.value ?? index;
    const s = new Set(selected.value);
    for (const k of rangeIndices(from, index)) {
      const id = items.value[k]?.entry_id;
      if (id) s.add(id);
    }
    selected.value = s;
  }
  function selectAll() {
    selected.value = new Set(items.value.map((it) => it.entry_id));
  }
  function setAnchor(index: number) {
    anchor.value = index;
  }
  function selectedIds(): string[] {
    return [...selected.value];
  }

  // ---- 抽屉 ----
  function openDrawer(entryId: string) {
    drawerEntryId.value = entryId;
  }
  function closeDrawer() {
    drawerEntryId.value = null;
  }
  function toggleDrawer(entryId: string) {
    drawerEntryId.value = drawerEntryId.value === entryId ? null : entryId;
  }

  return {
    books,
    booksLoading,
    booksError,
    listError,
    latestRelease,
    currentRelease,
    codexId,
    filter,
    items,
    total,
    counts,
    loading,
    listVersion,
    selected,
    anchor,
    focus,
    drawerEntryId,
    book,
    defaults,
    selectedCount,
    focusedEntry,
    pageCount,
    sortDir,
    sortDirIsDefault,
    loadBooks,
    refreshBooks,
    afterSync,
    open,
    setFilter,
    resetFilter,
    reload,
    setPage,
    nextPage,
    prevPage,
    setPageSize,
    setSort,
    toggleSortDir,
    applyLocal,
    setDecision,
    cycle,
    undo,
    clearSelection,
    setFocus,
    toggleSelect,
    selectRange,
    selectAll,
    setAnchor,
    selectedIds,
    openDrawer,
    closeDrawer,
    toggleDrawer,
  };
});
