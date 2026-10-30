// 插件目录名（Settings.plugin_name）：决定插件页面前缀 /api/plug/{plugin_name}/page。规则与后端校验一致。
export const DEFAULT_PLUGIN_NAME = "astrbot_plugin_codexlab";
export const PLUGIN_NAME_RE = /^[A-Za-z0-9_]{1,64}$/;

export function isValidPluginName(name: string): boolean {
  return PLUGIN_NAME_RE.test(name);
}

/** 插件页面接口前缀；名字不合法时退回缺省名，避免拼出可被路径注入的地址。 */
export function pluginPagePrefix(name: string): string {
  return `/api/plug/${isValidPluginName(name) ? name : DEFAULT_PLUGIN_NAME}/page`;
}
