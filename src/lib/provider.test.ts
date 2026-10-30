// provider.ts 纯函数：来源判定、评测界面只留 OpenAI 兼容直连端点、列表灰字（CONTRACT §10.1）。
import { describe, expect, it } from "vitest";
import type { ReverseProvider } from "../types";
import { directProviders, isPluginProvider, providerKind, providerSubtitle, providerTitle } from "./provider";

function direct(id: number, name: string, extra: Partial<ReverseProvider> = {}): ReverseProvider {
  return {
    provider_id: id, name, base_url: "https://api.example.com/v1", model: "vision-model-a", max_tokens: 2048, enabled: true,
    has_key: true, created_at: "2026-09-07T00:00:00Z", kind: "openai", plugin_provider_id: null, ...extra,
  };
}
function legacyPlugin(id: number, pid: string): ReverseProvider {
  return direct(id, pid, { base_url: "", model: "", kind: "plugin", plugin_provider_id: pid });
}

describe("providerKind / isPluginProvider / directProviders", () => {
  it("plugin 只认字面 plugin，其它（含老记录缺省）都是 openai", () => {
    expect(providerKind({ kind: "plugin" })).toBe("plugin");
    expect(providerKind({ kind: "openai" })).toBe("openai");
    expect(providerKind({ kind: undefined as unknown as "openai" })).toBe("openai");
    expect(providerKind({ kind: "whatever" as "openai" })).toBe("openai");
    expect(isPluginProvider(legacyPlugin(1, "remote"))).toBe(true);
    expect(isPluginProvider(direct(2, "A"))).toBe(false);
  });
  it("评测界面只列直连端点，保持原顺序", () => {
    const list = [direct(1, "A"), legacyPlugin(2, "remote"), direct(3, "B", { kind: undefined as unknown as "openai" })];
    expect(directProviders(list).map((p) => p.provider_id)).toEqual([1, 3]);
    expect(directProviders([])).toEqual([]);
  });
});

describe("providerSubtitle / providerTitle", () => {
  it("灰字给模型名，title 给 base_url；没 key 时提醒", () => {
    expect(providerSubtitle(direct(1, "A"))).toBe("vision-model-a");
    expect(providerTitle(direct(1, "A"))).toBe("https://api.example.com/v1");
    expect(providerTitle(direct(1, "A", { has_key: false }))).toBe("尚未设置 API key");
  });
});
