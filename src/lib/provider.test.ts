// provider.ts 纯函数：模型来源徽标、插件 provider ↔ 本地记录、入参构造、对话框行（CONTRACT §10.1）。
import { describe, expect, it } from "vitest";
import type { ReverseProvider } from "../types";
import {
  PLUGIN_PROVIDER_MAX_TOKENS, PROVIDER_KIND_LABEL, findPluginProvider, isPluginProvider, pluginProviderInput, pluginProviderRows,
  providerInputFrom, providerKind, providerSubtitle, providerTitle,
} from "./provider";

function local(id: number, name: string, extra: Partial<ReverseProvider> = {}): ReverseProvider {
  return {
    provider_id: id, name, base_url: "https://api.example.com/v1", model: "gpt-4o", max_tokens: 2048, enabled: true,
    has_key: true, created_at: "2026-09-07T00:00:00Z", kind: "openai", plugin_provider_id: null, ...extra,
  };
}
function plugin(id: number, pid: string, extra: Partial<ReverseProvider> = {}): ReverseProvider {
  return {
    provider_id: id, name: pid, base_url: "", model: "", max_tokens: 1024, enabled: true,
    has_key: true, created_at: "2026-09-07T00:00:00Z", kind: "plugin", plugin_provider_id: pid, ...extra,
  };
}

describe("providerKind / isPluginProvider", () => {
  it("plugin 只认字面 plugin，其它（含老记录缺省）都是 openai", () => {
    expect(providerKind({ kind: "plugin" })).toBe("plugin");
    expect(providerKind({ kind: "openai" })).toBe("openai");
    expect(providerKind({ kind: undefined as unknown as "openai" })).toBe("openai");
    expect(providerKind({ kind: "whatever" as "openai" })).toBe("openai");
    expect(isPluginProvider(plugin(1, "gemini"))).toBe(true);
    expect(isPluginProvider(local(2, "本地"))).toBe(false);
  });
  it("徽标文字", () => {
    expect(PROVIDER_KIND_LABEL.plugin).toBe("插件");
    expect(PROVIDER_KIND_LABEL.openai).toBe("本地");
  });
});

describe("findPluginProvider", () => {
  it("按 kind=plugin 且 plugin_provider_id 相等找，openai 记录同名也不算", () => {
    const list = [local(1, "gemini"), plugin(2, "gemini"), plugin(3, "gpt")];
    expect(findPluginProvider(list, "gemini")?.provider_id).toBe(2);
    expect(findPluginProvider(list, "gpt")?.provider_id).toBe(3);
    expect(findPluginProvider(list, "none")).toBeNull();
    expect(findPluginProvider([], "gemini")).toBeNull();
  });
});

describe("pluginProviderInput / providerInputFrom", () => {
  it("启用为反推模型：§10.1 的字段", () => {
    expect(pluginProviderInput("gemini_flash")).toEqual({
      name: "gemini_flash", base_url: "", model: "", max_tokens: PLUGIN_PROVIDER_MAX_TOKENS, enabled: true,
      kind: "plugin", plugin_provider_id: "gemini_flash",
    });
    expect(PLUGIN_PROVIDER_MAX_TOKENS).toBe(1024);
  });
  it("改 enabled 时其余字段原样回填，带 provider_id", () => {
    const p = plugin(7, "gemini", { enabled: true, max_tokens: 512 });
    expect(providerInputFrom(p, { enabled: false })).toEqual({
      provider_id: 7, name: "gemini", base_url: "", model: "", max_tokens: 512, enabled: false, kind: "plugin", plugin_provider_id: "gemini",
    });
  });
  it("本地记录回填 kind=openai、plugin_provider_id=null；老记录缺 kind 也一样", () => {
    const p = local(3, "本地", { kind: undefined as unknown as "openai", plugin_provider_id: "leftover" });
    const input = providerInputFrom(p);
    expect(input.kind).toBe("openai");
    expect(input.plugin_provider_id).toBeNull();
    expect(input.base_url).toBe("https://api.example.com/v1");
    expect(input.enabled).toBe(true);
  });
});

describe("providerSubtitle / providerTitle", () => {
  it("本地给 model 与 base_url，没 key 时 title 提醒", () => {
    expect(providerSubtitle(local(1, "本地"))).toBe("gpt-4o");
    expect(providerTitle(local(1, "本地"))).toBe("https://api.example.com/v1");
    expect(providerTitle(local(1, "本地", { has_key: false }))).toBe("尚未设置 API key");
  });
  it("插件：名字与 id 相同时灰字省略，不同才给 id", () => {
    expect(providerSubtitle(plugin(1, "gemini"))).toBe("");
    expect(providerSubtitle(plugin(1, "gemini", { name: "闪电" }))).toBe("gemini");
    expect(providerSubtitle(plugin(1, "gemini", { plugin_provider_id: null }))).toBe("");
    expect(providerTitle(plugin(1, "gemini"))).toBe("AstrBot 插件里的 provider：gemini");
  });
});

describe("pluginProviderRows", () => {
  it("按插件给的顺序列 id，已加入的带记录，本地多出来的标 missing", () => {
    const list = [local(1, "本地"), plugin(2, "gpt"), plugin(3, "gone")];
    const rows = pluginProviderRows(["gemini", "gpt"], list);
    expect(rows.map((r) => r.id)).toEqual(["gemini", "gpt", "gone"]);
    expect(rows[0]).toEqual({ id: "gemini", provider: null, missing: false });
    expect(rows[1].provider?.provider_id).toBe(2);
    expect(rows[1].missing).toBe(false);
    expect(rows[2].provider?.provider_id).toBe(3);
    expect(rows[2].missing).toBe(true);
  });
  it("不可达（null）时只列本地 plugin 记录，missing 一律 false，本地 openai 不出现", () => {
    const rows = pluginProviderRows(null, [local(1, "本地"), plugin(2, "gpt")]);
    expect(rows).toEqual([{ id: "gpt", provider: rows[0].provider, missing: false }]);
    expect(rows[0].provider?.provider_id).toBe(2);
  });
  it("插件列表里的空串与重复 id 跳过；同 id 的重复本地记录各占一行", () => {
    const list = [plugin(2, "gpt"), plugin(5, "gpt")];
    const rows = pluginProviderRows(["", "gpt", "gpt"], list);
    expect(rows.map((r) => [r.id, r.provider?.provider_id, r.missing])).toEqual([["gpt", 2, false], ["gpt", 5, false]]);
  });
  it("两边都空 → 空", () => {
    expect(pluginProviderRows([], [])).toEqual([]);
    expect(pluginProviderRows(null, [])).toEqual([]);
  });
});
