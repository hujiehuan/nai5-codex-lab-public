//! nai5-codex-lab 后端入口。模块划分固定，命令清单以 CONTRACT.md §1 为准。
//!
//! - `error`     AppError（已定稿，勿改形状）
//! - `models`    DTO，与 src/types.ts 逐字段一致
//! - `db`        SQLite 打开、PRAGMA、迁移、通用查询辅助
//! - `state`     AppState（数据目录、DB 连接、同步任务句柄、节流器）
//! - `sync`      §1.3 同步算法（release 链、校验、条目 upsert、图片队列、缩略图）
//! - `artist`    画师串解析器（移植自 Langbai artist-recipe.ts，MIT）、signature、序列化
//! - `export`    三个导出文件 + 完整池（§8）+ 推送 / 插件状态
//! - `plugin`    插件 HTTP 传输层（登录换 JWT、X-Library-Token、401 重登），export / lab / reverse 共用；`plugin_providers`（§10.1）
//! - `lab`       画图实验室（§9.4）：转发插件路由、本地 `lab_run` 镜像、图片缓存
//! - `library`   词典页数据层：书目/条目分页/决策/撤销/黑名单/日志读取
//! - `reverse`   反推实验室：内置模板/规则、视觉模型调用、清洗、指标；v0.2.4 重画对照（`render`，§11）
//! - `style`     画风索引与反推画风（§12）：candle + CLIP ViT-B/32（CPU）建向量、减均值余弦检索、权重下载
//! - `secrets`   keyring 封装
//! - `commands`  tauri 命令层，薄，只做参数校验与调用

pub mod artist;
pub mod activity;
pub mod commands;
pub mod console;
pub mod creative;
pub mod db;
pub mod error;
pub mod export;
pub mod lab;
pub mod library;
pub mod models;
pub mod plugin;
pub mod reverse;
pub mod secrets;
pub mod state;
pub mod style;
pub mod studio;
pub mod sync;
pub mod update_backup;
pub mod update_installer;
pub mod updater;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if update_installer::run_helper_if_requested() { return; }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("app_data_dir");
            let state = state::AppState::init(app.handle().clone(), data_dir)?;
            app.manage(state);
            app.state::<state::AppState>().schedule_daily_backup();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_update_status,
            commands::app_update_choose_source,
            commands::app_update_check,
            commands::app_update_prepare,
            commands::app_update_cancel,
            commands::app_update_install,
            commands::studio_capabilities,
            commands::studio_references,
            commands::studio_prepare,
            commands::studio_plan,
            commands::studio_plan_get,
            commands::studio_start,
            commands::studio_status,
            commands::studio_list,
            commands::studio_cancel,
            commands::studio_image,
            commands::studio_compare,
            commands::studio_skills_list,
            commands::studio_skills_import,
            commands::studio_skills_delete,
            commands::studio_skills_fetch,
            commands::studio_skills_sync,
            commands::studio_workspace_get,
            commands::studio_workspace_save,
            commands::style_compare,
            commands::style_compare_cancel,
            console::plugin_model_selection,
            console::reverse_source_image,
            commands::creative_capabilities,
            commands::creative_references,
            commands::creative_plan,
            commands::creative_edit,
            commands::creative_plans_list,
            commands::creative_plan_references,
            commands::creative_start,
            commands::creative_status,
            commands::creative_list,
            commands::creative_cancel,
            commands::creative_image,
            commands::app_info,
            commands::settings_get,
            commands::settings_set,
            commands::secret_set,
            commands::secret_has,
            commands::secret_delete,
            commands::sync_status,
            commands::sync_start,
            commands::sync_cancel,
            commands::sync_logs,
            commands::list_books,
            commands::list_entries,
            commands::get_entry,
            commands::set_decision,
            commands::set_member_note,
            commands::undo_decision,
            commands::download_images,
            commands::blacklist_list,
            commands::blacklist_add,
            commands::blacklist_remove,
            commands::export_libraries,
            commands::push_to_plugin,
            commands::export_full_pool,
            commands::push_full_to_plugin,
            commands::plugin_library_status,
            commands::lab_run_preview,
            commands::lab_run_start,
            commands::lab_run_status,
            commands::lab_run_list,
            commands::lab_run_image,
            commands::lab_run_rate,
            commands::reverse_templates_list,
            commands::reverse_template_save,
            commands::reverse_rulesets_list,
            commands::reverse_ruleset_save,
            commands::reverse_providers_list,
            commands::reverse_provider_save,
            commands::reverse_provider_delete,
            commands::plugin_providers,
            commands::reverse_questions,
            commands::reverse_pick_local_image,
            commands::reverse_import_local_image,
            commands::reverse_run_batch,
            commands::reverse_cancel_batch,
            commands::reverse_batches_list,
            commands::reverse_batch_results,
            commands::reverse_run_get,
            commands::reverse_compare,
            commands::reverse_judge,
            commands::reverse_render_run,
            commands::style_models_list,
            commands::style_model_select,
            commands::style_classifiers_list,
            commands::style_classifier_download,
            commands::style_classify,
            commands::style_model_status,
            commands::style_model_download,
            commands::style_model_download_cancel,
            commands::style_index_rebuild,
            commands::style_search,
            commands::events_recent,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<state::AppState>() { state.updater.shutdown(); }
            }
        });
}
