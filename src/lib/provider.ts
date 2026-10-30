// 反推模型的两种来源（CONTRACT §10.1）的纯函数：徽标文字、插件 provider ↔ 本地记录的对应、
// "启用为反推模型"与改 enabled 的入参、列表里的灰字。ProviderManager.vue / RunConfig.vue 只做渲染与调用。
import type { ReverseProvider, ReverseProviderInput, ReverseProviderKind } from "../types";

export const PROVIDER_KIND_LABEL: Record<ReverseProviderKind, string> = { openai: "本地", plugin: "插件" };
/** §10.1：插件 provider 只需要 max_tokens 这一个参数（插件端收下但不透传），固定 1024。 */
export const PLUGIN_PROVIDER_MAX_TOKENS = 1024;

/** 老后端 / 老记录没有 kind（或给了不认识的值）时按 openai 处理（§10.1：老客户端不传即 openai）。 */
export function providerKind(p: Pick<ReverseProvider, "kind">): ReverseProviderKind {
  return p.kind === "plugin" ? "plugin" : "openai";
}
export function isPluginProvider(p: Pick<ReverseProvider, "kind">): boolean {
  return providerKind(p) === "plugin";
}

/** 本地记录里对应某个插件 provider id 的那条（kind=plugin 且 plugin_provider_id 相等）；没有 → null。 */
export function findPluginProvider(providers: readonly ReverseProvider[], pluginProviderId: string): ReverseProvider | null {
  return providers.find((p) => isPluginProvider(p) && p.plugin_provider_id === pluginProviderId) ?? null;
}

/** "启用为反推模型"：按 §10.1 造一条 kind=plugin 的记录（name 默认为 provider id，base_url / model 空串，enabled=true）。 */
export function pluginProviderInput(pluginProviderId: string): ReverseProviderInput {
  return {
    name: pluginProviderId, base_url: "", model: "", max_tokens: PLUGIN_PROVIDER_MAX_TOKENS, enabled: true,
    kind: "plugin", plugin_provider_id: pluginProviderId,
  };
}

/** 现有记录改 enabled / name / max_tokens 时的入参：其余字段原样回填（后端 save 是整条覆盖），kind 缺省按 openai。 */
export function providerInputFrom(
  p: ReverseProvider,
  patch: Partial<Pick<ReverseProviderInput, "enabled" | "name" | "max_tokens">> = {},
): ReverseProviderInput {
  const plugin = isPluginProvider(p);
  return {
    provider_id: p.provider_id, name: p.name, base_url: p.base_url, model: p.model, max_tokens: p.max_tokens, enabled: p.enabled,
    kind: plugin ? "plugin" : "openai", plugin_provider_id: plugin ? p.plugin_provider_id : null,
    ...patch,
  };
}

/** 模型列表里名字后面的灰字：本地给 model，插件给 provider id（与名字相同时省略，免得重复）。 */
export function providerSubtitle(p: ReverseProvider): string {
  if (isPluginProvider(p)) return p.plugin_provider_id && p.plugin_provider_id !== p.name ? p.plugin_provider_id : "";
  return p.model;
}

/** 勾选框的 title：本地给 base_url（没 key 时提醒），插件给 provider id。 */
export function providerTitle(p: ReverseProvider): string {
  if (isPluginProvider(p)) return `AstrBot 插件里的 provider：${p.plugin_provider_id ?? ""}`;
  return p.has_key ? p.base_url : "尚未设置 API key";
}

/** 模型管理对话框上半栏的一行：插件里的 provider id、本地对应的记录（没有 → 可"启用为反推模型"）、插件里是否已没有它。 */
export interface PluginProviderRow { id: string; provider: ReverseProvider | null; missing: boolean }

/**
 * 插件返回的 id 列表 ∪ 本地已有的 plugin 记录：
 * - 列表里的每个 id 一行，按插件给的顺序；
 * - 本地有记录但列表里没有的（AstrBot 里删了）也列出，标 `missing`，仍可停用 / 删除；
 * - `pluginIds=null`（不可达 / 还没取到）时只列本地记录，`missing` 一律 false（不知道，不乱标）；
 * - 同一 id 的重复本地记录各占一行，方便删多余的。
 */
export function pluginProviderRows(pluginIds: readonly string[] | null, providers: readonly ReverseProvider[]): PluginProviderRow[] {
  const rows: PluginProviderRow[] = [];
  const seenIds = new Set<string>();
  const shown = new Set<number>();
  for (const id of pluginIds ?? []) {
    if (!id || seenIds.has(id)) continue;
    seenIds.add(id);
    const provider = findPluginProvider(providers, id);
    if (provider) shown.add(provider.provider_id);
    rows.push({ id, provider, missing: false });
  }
  for (const p of providers) {
    if (!isPluginProvider(p) || shown.has(p.provider_id)) continue;
    shown.add(p.provider_id);
    const id = p.plugin_provider_id ?? "";
    rows.push({ id, provider: p, missing: pluginIds !== null && !seenIds.has(id) });
  }
  return rows;
}
