// 命令封装：每个后端命令一个函数，参数名与 CONTRACT.md 一致（snake_case）。
import { Channel, convertFileSrc } from "@tauri-apps/api/core";
import { invoke } from "./ipc";
import type * as T from "./types";
import {
  styleRenderDefaults,
  type StyleRenderFields,
} from "./lib/reverse-summary";

export { Channel, convertFileSrc };

export function isAppError(e: unknown): e is T.AppError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}
export function errMessage(e: unknown): string {
  if (isAppError(e)) return `${e.message}（${e.code}）`;
  return e instanceof Error ? e.message : String(e);
}

export const api = {
  pluginModelSelection: () => invoke<unknown>("plugin_model_selection"),
  reverseSourceImage: (run_id: number) =>
    invoke<T.SourceImageResult>("reverse_source_image", { run_id }),
  appInfo: () => invoke<T.AppInfo>("app_info"),
  appUpdateStatus: () => invoke<T.AppUpdateStatus>("app_update_status"),
  appUpdateChooseSource: () => invoke<T.AppUpdateStatus>("app_update_choose_source"),
  appUpdateCheck: () => invoke<T.AppUpdateStatus>("app_update_check"),
  appUpdatePrepare: () => invoke<T.AppUpdateStatus>("app_update_prepare"),
  appUpdateCancel: () => invoke<T.AppUpdateStatus>("app_update_cancel"),
  appUpdateInstall: () => invoke<T.AppUpdateStatus>("app_update_install"),
  settingsGet: () => invoke<T.Settings>("settings_get"),
  settingsSet: (patch: T.SettingsPatch) =>
    invoke<T.Settings>("settings_set", { patch }),
  secretSet: (account: string, value: string) =>
    invoke<void>("secret_set", { account, value }),
  secretHas: (account: string) => invoke<boolean>("secret_has", { account }),
  secretDelete: (account: string) => invoke<void>("secret_delete", { account }),

  syncStatus: () => invoke<T.SyncStatus>("sync_status"),
  syncStart: (force: boolean, on_event: Channel<T.SyncEvent>) =>
    invoke<void>("sync_start", { force, on_event }),
  syncCancel: () => invoke<void>("sync_cancel"),
  syncLogs: (limit = 20) => invoke<T.SyncLog[]>("sync_logs", { limit }),

  listBooks: () => invoke<T.Book[]>("list_books"),
  listEntries: (query: T.EntryQuery) =>
    invoke<T.EntryPage>("list_entries", { query }),
  getEntry: (entry_id: string) =>
    invoke<T.EntryDetail>("get_entry", { entry_id }),
  setDecision: (
    entry_ids: string[],
    decision: T.Decision,
    via: T.DecisionVia,
  ) =>
    invoke<{ changed: number }>("set_decision", { entry_ids, decision, via }),
  setMemberNote: (entry_id: string, note: string) =>
    invoke<void>("set_member_note", { entry_id, note }),
  undoDecision: () => invoke<T.UndoResult | null>("undo_decision"),
  downloadImages: (
    entry_id: string,
    kind: "display_all" | "originals",
    on_event: Channel<T.SyncEvent>,
  ) => invoke<void>("download_images", { entry_id, kind, on_event }),

  blacklistList: () => invoke<T.BlacklistItem[]>("blacklist_list"),
  blacklistAdd: (name: string, reason: string) =>
    invoke<void>("blacklist_add", { name, reason }),
  blacklistRemove: (name: string) => invoke<void>("blacklist_remove", { name }),

  exportLibraries: (options: T.ExportOptions) =>
    invoke<T.ExportResult>("export_libraries", { options }),
  pushToPlugin: () =>
    invoke<T.PushResult>("push_to_plugin"),
  exportFullPool: (options: T.ExportOptions) =>
    invoke<T.ExportResult>("export_full_pool", { options }),
  pushFullToPlugin: () =>
    invoke<T.PushFullResult>("push_full_to_plugin"),
  pluginLibraryStatus: () =>
    invoke<T.PluginLibraryStatus>("plugin_library_status"),

  reverseTemplatesList: () =>
    invoke<T.ReverseTemplate[]>("reverse_templates_list"),
  reverseTemplateSave: (input: T.ReverseTemplateInput) =>
    invoke<T.ReverseTemplate>("reverse_template_save", { input }),
  reverseRulesetsList: () =>
    invoke<T.ReverseRuleset[]>("reverse_rulesets_list"),
  reverseRulesetSave: (input: T.ReverseRulesetInput) =>
    invoke<T.ReverseRuleset>("reverse_ruleset_save", { input }),
  reverseProvidersList: () =>
    invoke<T.ReverseProvider[]>("reverse_providers_list"),
  reverseProviderSave: (input: T.ReverseProviderInput) =>
    invoke<T.ReverseProvider>("reverse_provider_save", { input }),
  reverseProviderDelete: (provider_id: number) =>
    invoke<void>("reverse_provider_delete", { provider_id }),
  /** v0.2.3（§10.1）：GET {PLUGIN_PAGE_PATH}/providers → AstrBot 里配置好的 provider id 列表；不可达 → AppError network。 */
  pluginProviders: () => invoke<string[]>("plugin_providers"),
  reverseQuestions: (query: T.QuestionQuery) =>
    invoke<T.Question[]>("reverse_questions", { query }),
  reversePickLocalImage: () =>
    invoke<string | null>("reverse_pick_local_image"),
  reverseImportLocalImage: (path: string) =>
    invoke<string>("reverse_import_local_image", { path }),
  reverseRunBatch: (req: T.RunBatchRequest, on_event: Channel<T.RunEvent>) =>
    invoke<{ batch_id: string }>("reverse_run_batch", { req, on_event }),
  reverseCancelBatch: (batch_id: string) =>
    invoke<void>("reverse_cancel_batch", { batch_id }),
  reverseBatchesList: () => invoke<T.BatchSummary[]>("reverse_batches_list"),
  reverseBatchResults: (batch_id: string) =>
    invoke<T.BatchResults>("reverse_batch_results", { batch_id }),
  reverseRunGet: (run_id: number) =>
    invoke<T.RunDetail>("reverse_run_get", { run_id }),
  reverseCompare: (batch_a: string, batch_b: string) =>
    invoke<T.CompareResult>("reverse_compare", { batch_a, batch_b }),
  reverseJudge: (run_id: number, score: number, comment: string) =>
    invoke<void>("reverse_judge", { run_id, score, comment }),
  /**
   * v0.2.4（§11.4）：单条重画并评分，覆盖旧结果；角色与固定种子沿用批次当时的，`judge_provider_id` null = 插件默认评委。重画失败不抛，`render_error` 在返回的行里。
   * v0.3.0（§12.5）：`artist` 非空时作为重画的画师串，写进 `RunRow.render_artist`；缺省 null。
   * v0.3.0（§12.6）：`artist_mode`（后端参数名 `render_artist_mode`）null / 空 = pool（与线上 `/反推` 同一概率抽画风池），"given" 才用 `artist`，"none" 纯标签对照；
   * `count`（后端 `render_count`）null = 插件默认张数，1–4 全部出图评分取最高分那张。非法值后端 `bad_request`。
   * v0.3.1（§12.7）：`artist_mode="style"` = 精准反推，`style` 是六个 `style_model / style_top_k / style_decisions / style_min_score / style_fallback / style_center`
   * 键（与 `RunBatchRequest` 同名同形），总是显式发——不给（其它模式）就发契约缺省，后端只在 style 时看。
   */
  reverseRenderRun: (
    run_id: number,
    judge_provider_id: string | null,
    artist: string | null = null,
    artist_mode: T.RenderArtistMode | null = null,
    count: number | null = null,
    style: StyleRenderFields | null = null,
  ) =>
    invoke<T.RunRow>("reverse_render_run", {
      run_id,
      judge_provider_id,
      artist,
      render_artist_mode: artist_mode,
      render_count: count,
      ...(style ?? styleRenderDefaults()),
    }),

  // ---- v0.2.0 画图实验室（CONTRACT §9.4，七个命令全部转发插件；不可达 → AppError network）----
  labRunPreview: (req: T.LabRunRequest) =>
    invoke<T.LabPlan[]>("lab_run_preview", { req }),
  labRunStart: (req: T.LabRunRequest) =>
    invoke<T.LabRunStart>("lab_run_start", { req }),
  labRunStatus: (run_id: string) =>
    invoke<T.LabRun>("lab_run_status", { run_id }),
  labRunList: (limit = 30) =>
    invoke<T.LabRunSummary[]>("lab_run_list", { limit }),
  /** 返回相对数据目录的路径 `labruns/<run_id>/<idx>[.thumb].png`，显示前用 lib/lab.ts 的 labImagePath 拼成绝对路径再 imgSrc。 */
  labRunImage: (run_id: string, idx: number, full: boolean) =>
    invoke<string>("lab_run_image", { run_id, idx, full }),
  labRunRate: (run_id: string, idx: number, rating: T.LabRating | null) =>
    invoke<void>("lab_run_rate", { run_id, idx, rating }),

  // ---- v0.3.0 画风索引与反推画风（CONTRACT §12.2）----
  styleModelStatus: (model?: string) =>
    invoke<T.StyleModelStatus>("style_model_status", { model }),
  styleModelsList: () => invoke<T.StyleModelStatus[]>("style_models_list"),
  styleModelSelect: (model: string) =>
    invoke<T.StyleModelStatus>("style_model_select", { model }),
  styleClassifiersList: () =>
    invoke<T.StyleClassifierStatus[]>("style_classifiers_list"),
  styleClassifierDownload: (
    model: string,
    on_event: Channel<T.DownloadEvent>,
  ) =>
    invoke<T.StyleClassifierStatus>("style_classifier_download", {
      model,
      on_event,
    }),
  styleClassify: (req: T.StyleClassifyRequest) =>
    invoke<T.StyleClassifyResult>("style_classify", { req }),
  /** 从 Hugging Face 流式下载权重到 models/<model>/（.part 再改名）；命令在下载结束后才返回最新状态，进度走 Channel<DownloadEvent>。 */
  styleModelDownload: (on_event: Channel<T.DownloadEvent>, model?: string) =>
    invoke<T.StyleModelStatus>("style_model_download", { on_event, model }),
  styleModelDownloadCancel: () => invoke<void>("style_model_download_cancel"),
  /** 不经同步单独建索引；force=true 删掉当前模型的全部向量重算。进度复用 SyncEvent（phase=embedding + embed_* 三键），取消走 sync_cancel。 */
  styleIndexRebuild: (
    force: boolean,
    on_event: Channel<T.SyncEvent>,
    model?: string,
  ) => invoke<void>("style_index_rebuild", { force, on_event, model }),
  /** 模型未下载 / 索引为空 → AppError bad_request（message 说明去设置页下载或重建）。 */
  styleSearch: (req: T.StyleSearchRequest) =>
    invoke<T.StyleSearchResult>("style_search", { req }),

  eventsRecent: (limit = 50) =>
    invoke<T.AppEvent[]>("events_recent", { limit }),
};

/** 本地绝对路径 → 可放进 <img src> 的 URL；null 给占位。 */
export function imgSrc(path: string | null | undefined): string | null {
  return path ? convertFileSrc(path) : null;
}
