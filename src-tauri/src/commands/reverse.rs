//! reverse_* 命令：模板/规则集/端点 CRUD、题库、批次启动（后台）、结果、详情、比较、评分、本地图片选择；
//! v0.2.3 加 `plugin_providers`（§10.1，列 AstrBot 里配置好的 provider）；v0.2.4 加 `reverse_render_run`（§11.4，单条重画对照）；
//! v0.3.0 `reverse_render_run` 多一个 `artist`（§12.5），再多 `render_artist_mode` / `render_count`（§12.6）；
//! v0.3.1 再多五个 `style_*`（§12.7 精准反推，与 `RunBatchRequest` 同名），`reverse_run_batch` 的 `prepare` 先查画风索引就绪。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::plugin::{self, PluginSession};
use crate::reverse::{batch, clean::CompiledRuleset, render, store};
use crate::state::AppState;

use super::blocking;

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_templates_list(state: State<'_, AppState>) -> AppResult<Vec<ReverseTemplate>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), |c| store::templates_list(c)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_template_save(state: State<'_, AppState>, input: ReverseTemplateInput) -> AppResult<ReverseTemplate> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::template_save(c, &input)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_rulesets_list(state: State<'_, AppState>) -> AppResult<Vec<ReverseRuleset>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), |c| store::rulesets_list(c)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_ruleset_save(state: State<'_, AppState>, input: ReverseRulesetInput) -> AppResult<ReverseRuleset> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::ruleset_save(c, &input)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_providers_list(state: State<'_, AppState>) -> AppResult<Vec<ReverseProvider>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), |c| store::providers_list(c)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_provider_save(state: State<'_, AppState>, input: ReverseProviderInput) -> AppResult<ReverseProvider> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::provider_save(c, &input)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_provider_delete(state: State<'_, AppState>, provider_id: i64) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::provider_delete(c, provider_id)).await
}

/// §10.1：GET `{PLUGIN_PAGE_PATH}/providers` → AstrBot 里配置好的 provider id 列表；不可达 → `network`，
/// 未配置地址 / 密码 → `bad_request` / `secret`，插件拒绝或没有该路由 → `plugin_rejected`。
#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_providers(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let _activity = state.activity.enter()?;
    plugin::plugin_providers(&state).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_questions(state: State<'_, AppState>, query: QuestionQuery) -> AppResult<Vec<Question>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| {
        // 真值预览用内置规则集口径
        let rs = store::rulesets_list(c)?
            .into_iter()
            .find(|r| r.name == crate::reverse::builtin::BUILTIN_NAME)
            .unwrap_or_else(crate::reverse::clean::builtin_ruleset);
        let compiled = CompiledRuleset::compile(&rs)?;
        store::questions(c, &query, &compiled)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_pick_local_image(state: State<'_, AppState>) -> AppResult<Option<String>> {
    let _activity = state.activity.enter()?;
    let app = state.app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("图片", &["png", "jpg", "jpeg", "webp", "gif"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| AppError::new("internal", e.to_string()))?;
    match picked {
        None => Ok(None),
        Some(fp) => {
            let path = fp.into_path().map_err(|e| AppError::bad_request(format!("无法读取所选路径: {e}")))?;
            let data_dir = state.data_dir.clone();
            let stored = tokio::task::spawn_blocking(move || import_local_image(&data_dir, &path))
                .await
                .map_err(|e| AppError::new("internal", e.to_string()))??;
            Ok(Some(stored.to_string_lossy().to_string()))
        }
    }
}

/// v0.2.5：本地图片先拷进数据目录 `local_images/<sha256 前 16 位>.<ext>`。路径落在 asset 协议范围（`$APPDATA/**`）内，
/// 运行详情才显示得出原图；同一文件重复导入得到同一路径，题目 id `local:<路径>` 也就稳定。
pub(crate) fn import_local_image(data_dir: &std::path::Path, src: &std::path::Path) -> AppResult<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(src).map_err(|e| AppError::bad_request(format!("读取图片失败 {}: {e}", src.display())))?;
    if bytes.is_empty() {
        return Err(AppError::bad_request("图片是空文件".to_string()));
    }
    let hash = hex::encode(Sha256::digest(&bytes));
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif"))
        .unwrap_or_else(|| "png".to_string());
    let dir = data_dir.join("local_images");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(format!("创建 local_images 失败: {e}")))?;
    let dest = dir.join(format!("{}.{ext}", &hash[..16]));
    if !dest.exists() {
        std::fs::write(&dest, &bytes).map_err(|e| AppError::io(format!("写入 {} 失败: {e}", dest.display())))?;
    }
    Ok(dest)
}

/// 拖放进来的路径也走同一套拷贝（§10.3 的 drop 现在先经这里再入题库）。
#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_import_local_image(state: State<'_, AppState>, path: String) -> AppResult<String> {
    let _activity = state.activity.enter()?;
    let data_dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || import_local_image(&data_dir, std::path::Path::new(&path)).map(|p| p.to_string_lossy().to_string()))
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_run_batch(state: State<'_, AppState>, req: RunBatchRequest, on_event: Channel<RunEvent>) -> AppResult<RunBatchResult> {
    let _activity = state.activity.enter()?;
    let lease = state.batch.reserve()?;
    // §10.1：批次里有插件端点时先取插件登录信息（地址 / 密码缺失在这里就报错，不会留下一条 running 的批次行），整个批次复用一个 JWT
    // §11.4：render=true 也要插件（重画路由），哪怕所有端点都是本地 OpenAI 兼容的
    let ids = req.provider_ids.clone();
    let needs_plugin = req.render || blocking(state.db.clone(), move |c| store::providers_any_plugin(c, &ids)).await?;
    let plugin = if needs_plugin { Some(Arc::new(PluginSession::new(plugin::plugin_conn(&state).await?))) } else { None };
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let req2 = req.clone();
    let (prepared, lease) = tokio::task::spawn_blocking(move || {
        let prepared = batch::prepare(&db, &data_dir, &req2)?;
        lease.bind(&prepared.batch_id);
        Ok::<_, AppError>((prepared, lease))
    }).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    let ctx = Arc::new(batch::BatchCtx {
        db: state.db.clone(),
        http: state.http.clone(),
        data_dir: state.data_dir.clone(),
        cancel: lease.cancel.clone(),
        emit: Arc::new(move |ev: RunEvent| {
            let _ = on_event.send(ev);
        }),
        plugin,
        style: state.style.clone(),
    });
    let batch_id = prepared.batch_id.clone();
    tauri::async_runtime::spawn(async move {
        let _activity = _activity;
        batch::run(ctx, prepared, lease).await;
    });
    Ok(RunBatchResult { batch_id })
}

/// §12.9：只停止指定的当前批次；已结束或不匹配的 ID 幂等返回，不影响其他批次。
#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_cancel_batch(state: State<'_, AppState>, batch_id: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    state.batch.cancel(&batch_id);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_batches_list(state: State<'_, AppState>) -> AppResult<Vec<BatchSummary>> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), |c| store::batches_list(c)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_batch_results(state: State<'_, AppState>, batch_id: String) -> AppResult<BatchResults> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::batch_results(c, &batch_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_run_get(state: State<'_, AppState>, run_id: i64) -> AppResult<RunDetail> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::run_get(c, run_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_compare(state: State<'_, AppState>, batch_a: String, batch_b: String) -> AppResult<CompareResult> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::compare(c, &batch_a, &batch_b)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_judge(state: State<'_, AppState>, run_id: i64, score: i64, comment: String) -> AppResult<()> {
    let _activity = state.activity.enter()?;
    blocking(state.db.clone(), move |c| store::judge(c, run_id, score, &comment)).await
}

/// §11.4 `reverse_render_run`：单条重画并评分，覆盖旧结果；重画失败不抛，`render_error` 在返回的 `RunRow` 里。
/// `judge_provider_id` null = 插件默认评委；§12.5 `artist` null / 空 = 不注入画师（「用它重画」传检索命中的 `core` / `pack_tags`，
/// 配 §12.6 `render_artist_mode="given"`）；`render_artist_mode` null / 空 = `pool`（与线上 `/反推` 同一概率抽画风池），
/// `render_count` null = 插件默认张数（上限 4）。三个都以本次参数为准，不从批次记录回退；非法值 `bad_request`。
/// §12.7 `render_artist_mode="style"` 时再看五个 `style_*`（与 `RunBatchRequest` 同名；null → 5 / 全部 / 不设门槛 / pool / true），
/// 索引未建 / 模型未下载在碰插件之前就 `bad_request`。
#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_render_run(
    state: State<'_, AppState>,
    run_id: i64,
    judge_provider_id: Option<String>,
    artist: Option<String>,
    render_artist_mode: Option<String>,
    render_count: Option<i64>,
    style_top_k: Option<i64>,
    style_decisions: Option<Vec<String>>,
    style_min_score: Option<f64>,
    style_fallback: Option<String>,
    style_center: Option<bool>,
    style_model: Option<String>,
) -> AppResult<RunRow> {
    let _activity = state.activity.enter()?;
    let mut style = render::StyleParams::from_opts(style_top_k, style_decisions, style_min_score, style_fallback, style_center);
    style.model = style_model;
    render::rerender(&state, run_id, judge_provider_id, artist, render_artist_mode, render_count, style).await
}
