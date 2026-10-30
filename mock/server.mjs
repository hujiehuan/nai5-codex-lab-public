// Only loaded by Vite mode=mock. Synthetic fixtures shaped like the plugin's HTTP API, never user library data.
import { PROVIDERS, libraryStatus } from "./plugin/fixtures.mjs";
import * as desktop from "./desktop.mjs";
import { answerCreative } from "./creative.mjs";
import { answerStudio, providers as studioProviders } from "./studio.mjs";
import { answerUpdater, appVersion } from "./updater.mjs";
const modelCatalog = [...new Set([...studioProviders, "synthetic/prompt-model", "synthetic/second-model", ...PROVIDERS])];
const settings = {
  plugin_base_url: "http://mock.invalid",
  webui_username: "offline-demo",
  download_concurrency: 4,
  style_device: "auto",
};
export const calls = [];
export async function answer(command, args = {}, scenario = "") {
  calls.push({ command, args });
  if (scenario === "error")
    throw { code: "network", message: "模拟：连接失败，请重试" };
  if (command.startsWith("app_update_")) return answerUpdater(command, args, scenario);
  if (command.startsWith("creative_")) return answerCreative(command, args, scenario);
  if (command.startsWith("studio_") || command === "style_compare" || command === "style_compare_cancel") return answerStudio(command, args, scenario);
  switch (command) {
    case "app_info":
      return {
        version: appVersion,
        app_data_dir: "/offline-demo",
        db_path: "/offline-demo/library.db",
        latest_release: "mock-release",
      };
    case "settings_get":
      return settings;
    case "settings_set":
      Object.assign(settings, args.patch);
      return settings;
    case "secret_has":
      return false;
    case "secret_set":
    case "secret_delete":
      return null;
    case "sync_status":
      return {
        running: false,
        phase: "idle",
        message: "尚未开始同步",
        images_total: 0,
        images_done: 0,
        images_failed: 0,
      };
    case "sync_logs":
    case "events_recent":
    case "blacklist_list":
    case "reverse_templates_list":
    case "reverse_rulesets_list":
    case "reverse_providers_list":
    case "reverse_questions":
    case "reverse_batches_list":
      return [];
    case "list_books":
      return desktop.books();
    case "list_entries":
      return desktop.list(args.query);
    case "get_entry":
      return desktop.detail(args.entry_id);
    case "set_decision":
      return desktop.decide(args.entry_ids, args.decision);
    case "undo_decision":
      return desktop.undo();
    case "set_member_note":
      desktop.note(args.entry_id, args.note);
      return null;
    case "style_model_status":
      return {
        model: "megastyle-siglip-so400m",
        label: "MegaStyle-Encoder",
        description: "模拟模型状态",
        dir: "/offline-demo/models/megastyle",
        present: true,
        size_bytes: 816900000,
        indexed: 1313,
        total: 1313,
        mean_ready: true,
        dim: 1152,
        download_size_bytes: 816900000,
        default_center: true,
        device_preference: "auto",
        active_device: "metal",
        metal_available: true,
        device_message: null,
      };
    case "style_models_list":
      return [await answer("style_model_status")];
    case "style_classifiers_list":
      return [];
    case "plugin_library_status":
      return {
        reachable: true,
        message: "已读取",
        ...libraryStatus(),
      };
    case "plugin_providers":
      return [...PROVIDERS];
    case "plugin_model_selection":
      // Plugin-owned shortlist (GET model-selection). Read-only: the desktop never edits it.
      if (scenario === "empty") return {};
      if (scenario === "401")
        throw { code: "plugin_rejected", message: "HTTP 401：模拟插件认证失败" };
      return { enabled: false, selected: [], available: modelCatalog, effective: modelCatalog, missing: [] };
    case "lab_run_list":
      return [];
    case "reverse_source_image":
      return { path: null, kind: "missing" };
    case "plugin:event|listen":
      return 1;
    case "plugin:event|unlisten":
      return null;
    default:
      throw { code: "not_found", message: `Mock 未登记命令 ${command}` };
  }
}
export function mockPlugin() {
  return {
    name: "offline-plugin-mock",
    configureServer(server) {
      server.middlewares.use("/__mock__/invoke", async (req, res) => {
        let raw = "";
        for await (const chunk of req) raw += chunk;
        try {
          const { command, args, scenario } = JSON.parse(raw);
          // URLSearchParams.get() sends null when the page has no ?scenario=.
          const value = await answer(command, args, scenario ?? "");
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify(value));
        } catch (error) {
          res.statusCode = 400;
          res.setHeader("Content-Type", "application/json");
          res.end(
            JSON.stringify(
              error?.code ? error : { code: "mock", message: String(error) },
            ),
          );
        }
      });
    },
  };
}
