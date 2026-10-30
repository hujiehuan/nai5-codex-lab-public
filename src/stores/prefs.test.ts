// prefs store：卡片档位的读取 / 持久化 / 步进。node 环境没有 localStorage，用一个 Map 顶上。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { CARD_SIZE_KEY } from "../lib/grid";
import { usePrefs } from "./prefs";

function fakeStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => { map.set(k, String(v)); },
    removeItem: (k: string) => { map.delete(k); },
    clear: () => map.clear(),
    get length() { return map.size; },
    key: (i: number) => [...map.keys()][i] ?? null,
    _map: map,
  };
}

beforeEach(() => setActivePinia(createPinia()));
afterEach(() => vi.unstubAllGlobals());

describe("prefs store · cardSize", () => {
  it("defaults to m when nothing is stored or storage is unavailable", () => {
    vi.stubGlobal("localStorage", fakeStorage());
    expect(usePrefs().cardSize).toBe("m");
  });

  it("restores the stored size and ignores garbage", () => {
    vi.stubGlobal("localStorage", fakeStorage({ [CARD_SIZE_KEY]: "xl" }));
    expect(usePrefs().cardSize).toBe("xl");
    setActivePinia(createPinia());
    vi.stubGlobal("localStorage", fakeStorage({ [CARD_SIZE_KEY]: "huge" }));
    expect(usePrefs().cardSize).toBe("m");
  });

  it("persists on set and steps within s..xl", () => {
    const storage = fakeStorage();
    vi.stubGlobal("localStorage", storage);
    const prefs = usePrefs();
    prefs.setCardSize("l");
    expect(prefs.cardSize).toBe("l");
    expect(storage._map.get(CARD_SIZE_KEY)).toBe("l");
    prefs.biggerCard();
    expect(prefs.cardSize).toBe("xl");
    prefs.biggerCard();
    expect(prefs.cardSize).toBe("xl");
    prefs.smallerCard();
    prefs.smallerCard();
    prefs.smallerCard();
    prefs.smallerCard();
    expect(prefs.cardSize).toBe("s");
    expect(storage._map.get(CARD_SIZE_KEY)).toBe("s");
    // 非法值不接受
    prefs.setCardSize("giant" as never);
    expect(prefs.cardSize).toBe("s");
  });
});
