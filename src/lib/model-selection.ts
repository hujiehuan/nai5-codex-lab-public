/** Plugin-owned model selection. Saved IDs remain readable; they are never substituted. */
export interface ModelSelection {
  enabled: boolean;
  selected: string[];
  available: string[];
  effective: string[];
  missing: string[];
}
function strings(value: unknown): value is string[] {
  return (
    Array.isArray(value) &&
    value.every((s) => typeof s === "string" && !!s.trim()) &&
    new Set(value).size === value.length
  );
}
export function parseModelSelection(value: unknown): ModelSelection {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("插件模型名单格式错误");
  const v = value as ModelSelection;
  if (
    typeof v.enabled !== "boolean" ||
    ![v.selected, v.available, v.effective, v.missing].every(strings)
  )
    throw new Error("插件模型名单格式错误");
  if (v.enabled && !v.selected.length)
    throw new Error("插件启用了空的模型名单，请在插件总览修正");
  const expected = v.enabled
    ? v.selected.filter((id) => v.available.includes(id))
    : v.available;
  const missing = v.selected.filter((id) => !v.available.includes(id));
  if (
    JSON.stringify(expected) !== JSON.stringify(v.effective) ||
    JSON.stringify(missing) !== JSON.stringify(v.missing)
  )
    throw new Error("插件模型名单与有效候选不一致，请刷新插件总览");
  return v;
}
export function missingModelIds(
  ids: readonly string[],
  allowed: readonly string[],
): string[] {
  return [...new Set(ids.filter((id) => !!id && !allowed.includes(id)))];
}
export function modelSelectionIssue(
  ids: readonly string[],
  allowed: readonly string[],
): string | null {
  const missing = missingModelIds(ids, allowed);
  return missing.length
    ? `这些模型已不在插件可选名单：${missing.join("、")}。旧值已保留，请移除或在插件总览调整名单。`
    : null;
}
export function unsupportedModelSelection(error: unknown): boolean {
  const e = error as { code?: string; message?: string } | null;
  return (
    e?.code === "not_found" ||
    (e?.code === "plugin_rejected" && /^HTTP (404|405)\b/.test(e.message ?? ""))
  );
}
