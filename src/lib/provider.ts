// 反推评测的模型来源（CONTRACT §10.1）纯函数。评测界面只暴露 OpenAI 兼容直连端点：
// 后端仍认 kind=plugin 的旧记录，这里把它们从列表与勾选里滤掉。ProviderManager.vue / RunConfig.vue 只做渲染与调用。
import type { ReverseProvider, ReverseProviderKind } from "../types";

/** 老后端 / 老记录没有 kind（或给了不认识的值）时按 openai 处理（§10.1：老客户端不传即 openai）。 */
export function providerKind(p: Pick<ReverseProvider, "kind">): ReverseProviderKind {
  return p.kind === "plugin" ? "plugin" : "openai";
}
export function isPluginProvider(p: Pick<ReverseProvider, "kind">): boolean {
  return providerKind(p) === "plugin";
}
/** 评测界面可见的模型：OpenAI 兼容直连端点（kind=openai，含缺 kind 的老记录）。 */
export function directProviders<T extends Pick<ReverseProvider, "kind">>(providers: readonly T[]): T[] {
  return providers.filter((p) => !isPluginProvider(p));
}

/** 模型列表里名字后面的灰字：模型名。 */
export function providerSubtitle(p: Pick<ReverseProvider, "model">): string {
  return p.model;
}

/** 勾选框的 title：base_url；没 key 时提醒。 */
export function providerTitle(p: Pick<ReverseProvider, "base_url" | "has_key">): string {
  return p.has_key ? p.base_url : "尚未设置 API key";
}
