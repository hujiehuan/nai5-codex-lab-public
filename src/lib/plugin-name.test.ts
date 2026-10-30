import { describe, expect, it } from "vitest";
import { DEFAULT_PLUGIN_NAME, isValidPluginName, pluginPagePrefix } from "./plugin-name";

describe("plugin-name", () => {
  it("缺省名合法，前缀由插件名拼出", () => {
    expect(isValidPluginName(DEFAULT_PLUGIN_NAME)).toBe(true);
    expect(pluginPagePrefix(DEFAULT_PLUGIN_NAME)).toBe(`/api/plug/${DEFAULT_PLUGIN_NAME}/page`);
    expect(pluginPagePrefix("my_plugin_2")).toBe("/api/plug/my_plugin_2/page");
  });

  it("只接受 1–64 位字母、数字、下划线", () => {
    expect(isValidPluginName("a".repeat(64))).toBe(true);
    for (const bad of ["", "a".repeat(65), "has-dash", "has space", "../x", "名字"]) expect(isValidPluginName(bad)).toBe(false);
    expect(pluginPagePrefix("../x")).toBe(`/api/plug/${DEFAULT_PLUGIN_NAME}/page`);
  });
});
