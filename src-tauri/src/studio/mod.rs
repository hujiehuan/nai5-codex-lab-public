//! §16 unified studio transport. Local input snapshots never change curation decisions.
mod reference_images;
pub mod skills;
pub(crate) mod store;
use crate::{
    creative,
    error::{AppError, AppResult},
    plugin::{self, PluginConn},
    state::AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path, time::Duration};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceQuery {
    pub mode: String,
    pub decisions: String,
    #[serde(default)]
    pub scope: Vec<Vec<String>>,
    pub search: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}
fn protocol(message: impl Into<String>) -> AppError {
    AppError::new("studio_protocol", message)
}
fn field<'a>(v: &'a Value, key: &str) -> AppResult<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::bad_request(format!("缺少文本字段 {key}")))
}
fn array<'a>(v: &'a Value, key: &str) -> AppResult<&'a Vec<Value>> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| protocol(format!("字段 {key} 不是数组")))
}
fn identity(conn: &PluginConn) -> String {
    store::hash(format!("{}\n{}", conn.base, conn.username).as_bytes())
}
fn check_identity(saved: &Value, conn: &PluginConn) -> AppResult<()> {
    if saved["plugin_identity"].as_str() != Some(identity(conn).as_str()) {
        return Err(AppError::bad_request(
            "该实验属于另一个插件连接，请切回原连接后继续",
        ));
    }
    Ok(())
}
fn decode(status: reqwest::StatusCode, text: &str, route: &str) -> AppResult<Value> {
    let parsed = serde_json::from_str::<Value>(text);
    if let Ok(v) = &parsed {
        if v["status"] == "error" {
            let m = v["message"].as_str().unwrap_or("插件拒绝实验请求");
            if route == "image" && status == 404 && m.contains("尚未生成") {
                return Err(AppError::new("studio_image_pending", "图片准备中"));
            }
            return Err(AppError::new(
                if status == 404 {
                    "not_found"
                } else {
                    "plugin_rejected"
                },
                m,
            ));
        }
    }
    if status == 404 || status == 405 {
        return Err(AppError::new(
            "studio_unsupported",
            "连接的插件没有总实验室接口，请更新插件并在 AstrBot 重载后重新检查",
        ));
    }
    if !status.is_success() {
        return Err(AppError::plugin_rejected(format!(
            "实验接口 HTTP {status}: {}",
            plugin::truncate(text, 300)
        )));
    }
    let v = parsed.map_err(|_| protocol("实验接口未返回 JSON"))?;
    let d = v.get("data").cloned().unwrap_or(v);
    if route == "list" {
        if !d.is_array() {
            return Err(protocol("实验列表不是数组"));
        }
    } else if !d.is_object() {
        return Err(protocol("实验响应不是对象"));
    }
    Ok(d)
}
async fn call(
    state: &AppState,
    conn: &PluginConn,
    method: reqwest::Method,
    route: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
    seconds: u64,
) -> AppResult<Value> {
    let query = feature_query(query);
    let (status, text) = plugin::plugin_request(
        &state.http,
        conn,
        method,
        &format!("{}/lab/studio/{route}", plugin::PLUGIN_PAGE_PATH),
        &query,
        body,
        Duration::from_secs(seconds),
    )
    .await?;
    decode(status, &text, route)
}
fn feature_query<'a>(query: &[(&'a str, String)]) -> Vec<(&'a str, String)> {
    let mut query: Vec<_> = query
        .iter()
        .filter(|(k, _)| *k != "features")
        .cloned()
        .collect();
    query.push(("features", "studio_v1".into()));
    query
}
pub async fn capabilities(state: &AppState) -> AppResult<Value> {
    let conn = plugin::plugin_conn(state).await?;
    let caps = call(
        state,
        &conn,
        reqwest::Method::GET,
        "capabilities",
        &[],
        None,
        60,
    )
    .await?;
    validate_caps(&caps)?;
    Ok(caps)
}
fn validate_caps(c: &Value) -> AppResult<()> {
    if c["version"] != 1 {
        return Err(protocol("总实验室协议版本不兼容"));
    }
    for key in ["providers", "characters", "free_sizes"] {
        array(c, key)?;
    }
    for key in ["providers", "characters"] {
        if array(c, key)?
            .iter()
            .any(|x| x.as_str().map_or(true, |s| s.trim().is_empty()))
        {
            return Err(protocol(format!("{key} 包含无效项目")));
        }
    }
    if !c["templates"].is_object() || !c["defaults"].is_object() || !c["available"].is_boolean() {
        return Err(protocol("总实验室能力缺少模板、默认值或状态"));
    }
    Ok(())
}
fn require_reference_images(caps: &Value, mode: &str) -> AppResult<()> {
    if mode != "reverse" && caps["reference_images_supported"] != true {
        return Err(AppError::new(
            "studio_reference_images_unsupported",
            "当前插件未支持图包参考图＋完整提示词，请更新并重载薇拉插件后重试；不会改成仅文字构思",
        ));
    }
    Ok(())
}
fn reference_content(request: &Value) -> AppResult<&str> {
    request
        .get("reference_content")
        .map_or(Some("image_text"), Value::as_str)
        .filter(|value| matches!(*value, "image_text" | "text"))
        .ok_or_else(|| AppError::bad_request("reference_content 不合法"))
}
fn needs_reference_images(request: &Value) -> AppResult<bool> {
    Ok(request["mode"] != "reverse" && reference_content(request)? == "image_text")
}
fn require_reference_content(caps: &Value, request: &Value) -> AppResult<()> {
    if request["mode"] == "reverse" {
        return Ok(());
    }
    if reference_content(request)? == "text" {
        if caps["reference_text_supported"] != true {
            return Err(AppError::new(
                "studio_reference_text_unsupported",
                "当前插件未支持显式纯文字参考，请更新并重载薇拉插件后重试；不会改成图文构思",
            ));
        }
        if request.get("reference_images").is_some() {
            return Err(AppError::bad_request("纯文字参考不能附带 reference_images"));
        }
        Ok(())
    } else {
        require_reference_images(caps, field(request, "mode")?)
    }
}
fn validate_query(req: &ReferenceQuery) -> AppResult<()> {
    if req.mode != "draw"
        || !matches!(req.decisions.as_str(), "include" | "all")
        || req.limit.is_some_and(|x| x == 0 || x > 500)
        || req.offset.unwrap_or(0) > 100_000
        || req.search.as_ref().is_some_and(|s| s.len() > 1000)
    {
        return Err(AppError::bad_request("参考范围、模式或分页不合法"));
    }
    Ok(())
}
pub fn load_references(conn: &rusqlite::Connection, req: &ReferenceQuery) -> AppResult<Vec<Value>> {
    validate_query(req)?;
    let refs = creative::local_references_for(conn, &req.scope, req.decisions == "all")?;
    let mut out = Vec::with_capacity(refs.len());
    let mut stmt = conn.prepare("SELECT codex_id FROM codex_entry WHERE entry_id=?1")?;
    for r in refs {
        let codex_id: String = stmt.query_row([&r.entry_id], |row| row.get(0))?;
        let mut v = serde_json::to_value(r)?;
        v["codex_id"] = json!(codex_id);
        v["removed_at"] = Value::Null;
        v["blacklisted"] = json!(false);
        out.push(v);
    }
    Ok(out)
}
pub async fn references(state: &AppState, req: ReferenceQuery) -> AppResult<Value> {
    validate_query(&req)?;
    let r = req.clone();
    let mut items =
        crate::commands::blocking(state.db.clone(), move |c| load_references(c, &r)).await?;
    if let Some(search) = req.search.filter(|s| !s.trim().is_empty()) {
        let s = search.to_lowercase();
        items.retain(|r| {
            format!("{} {} {}", r["title"], r["path"], r["tags"])
                .to_lowercase()
                .contains(&s)
        });
    }
    let total = items.len();
    let items: Vec<_> = items
        .into_iter()
        .skip(req.offset.unwrap_or(0))
        .take(req.limit.unwrap_or(200))
        .collect();
    Ok(json!({"items":items,"total":total}))
}
fn input_query(req: &Value) -> AppResult<ReferenceQuery> {
    Ok(ReferenceQuery {
        mode: field(req, "mode")?.into(),
        decisions: req["reference_decisions"]
            .as_str()
            .unwrap_or("include")
            .into(),
        scope: serde_json::from_value(
            req.get("scope")
                .or_else(|| req.get("reference_scope"))
                .cloned()
                .unwrap_or(json!([])),
        )?,
        search: None,
        offset: None,
        limit: None,
    })
}
fn choose_references(mut pool: Vec<Value>, req: &Value, nonce: &str) -> AppResult<Vec<Value>> {
    let mode = req["reference_mode"].as_str().unwrap_or("all");
    if !matches!(mode, "sample" | "all") {
        return Err(AppError::bad_request("参考方式不合法"));
    }
    if mode == "sample" {
        let n = req.get("reference_count").map_or(Ok(4), |v| {
            v.as_u64()
                .ok_or_else(|| AppError::bad_request("抽样数必须为整数"))
        })? as usize;
        if !(1..=8).contains(&n) {
            return Err(AppError::bad_request("抽样数须为1–8"));
        }
        let priority: Vec<String> =
            serde_json::from_value(req.get("reference_ids").cloned().unwrap_or(json!([])))?;
        if priority.len() > n
            || priority.iter().collect::<HashSet<_>>().len() != priority.len()
            || priority
                .iter()
                .any(|id| !pool.iter().any(|r| r["entry_id"] == *id))
        {
            return Err(AppError::bad_request(
                "重点参考不在当前范围、重复或超过抽样数量",
            ));
        }
        pool.sort_by_key(|r| {
            let id = r["entry_id"].as_str().unwrap_or("");
            (
                priority.iter().position(|x| x == id).unwrap_or(usize::MAX),
                store::hash(format!("{nonce}:{id}").as_bytes()),
            )
        });
        pool.truncate(n);
    }
    if pool.is_empty() {
        return Err(AppError::bad_request(
            "当前决策/分级范围没有可用图包，请调整范围或同步图库",
        ));
    }
    if mode == "sample" && serde_json::to_vec(&pool)?.len() > 2 * 1024 * 1024 {
        return Err(AppError::bad_request(
            "抽样参考超过2 MiB，请减少抽样数量；不会静默截断",
        ));
    }
    Ok(pool)
}
fn prepare_image(path: &str) -> AppResult<Value> {
    let p = Path::new(path);
    if !p.is_absolute() || !p.is_file() || std::fs::metadata(p)?.len() > 32 * 1024 * 1024 {
        return Err(AppError::bad_request(
            "输入必须为不超过32 MiB的本地图片绝对路径",
        ));
    }
    let dims = image::image_dimensions(p)?;
    if u64::from(dims.0) * u64::from(dims.1) > 32_000_000 {
        return Err(AppError::bad_request("输入图片超过3200万像素"));
    }
    let img = crate::reverse::client::prepare_image(p)?;
    if img.width == 0
        || img.height == 0
        || img.bytes.len() > 9_000_000
        || img.original_bytes > 32 * 1024 * 1024
        || u64::from(img.original_width) * u64::from(img.original_height) > 32_000_000
    {
        return Err(AppError::bad_request("无法读取图片或图片超过容量限制"));
    }
    if !matches!(img.mime.as_str(), "image/png" | "image/jpeg" | "image/webp") {
        return Err(AppError::bad_request("仅支持 PNG、JPEG、WebP 图片"));
    }
    Ok(
        json!({"mime":img.mime,"image_base64":crate::reverse::client::image_base64(&img),"sha256":img.sha256,"width":img.original_width,"height":img.original_height,"bytes":img.bytes.len(),"shrunk":img.shrunk}),
    )
}
fn normalize_input(req: &mut Value) -> AppResult<()> {
    let count = req
        .get("count")
        .map_or(Some(3), Value::as_u64)
        .filter(|n| (1..=8).contains(n))
        .ok_or_else(|| AppError::bad_request("张数须为1–8的整数"))?;
    req["count"] = json!(count);
    for (key, default, allowed) in [
        (
            "orientation",
            "portrait",
            &["portrait", "landscape", "square", "auto"][..],
        ),
        ("reference_mode", "all", &["all", "sample"][..]),
        (
            "reference_content",
            "image_text",
            &["image_text", "text"][..],
        ),
        ("reference_decisions", "include", &["include", "all"][..]),
    ] {
        let value = req
            .get(key)
            .map_or(Some(default), Value::as_str)
            .filter(|s| allowed.contains(s))
            .ok_or_else(|| AppError::bad_request(format!("{key} 不合法")))?
            .to_owned();
        req[key] = json!(value);
    }
    for (key, max) in [("character", 1000), ("requirements", 4000)] {
        let text = req
            .get(key)
            .map_or(Some(""), Value::as_str)
            .filter(|s| s.chars().count() <= max)
            .ok_or_else(|| AppError::bad_request(format!("{key} 必须为长度合规的文本")))?
            .to_owned();
        req[key] = json!(text);
    }
    let characters: Vec<String> =
        serde_json::from_value(req.get("characters").cloned().unwrap_or(json!([])))?;
    let characters: Vec<_> = characters
        .into_iter()
        .map(|s| s.trim().to_owned())
        .collect();
    if characters.len() > 6
        || characters
            .iter()
            .any(|s| s.is_empty() || s.chars().count() > 1000)
        || characters.iter().collect::<HashSet<_>>().len() != characters.len()
    {
        return Err(AppError::bad_request("角色列表须为最多6个不重复的非空角色"));
    }
    req["characters"] = json!(characters);
    if req["mode"] == "reverse" {
        // This selector concerns creative references, never the reverse source image.
        req["reference_content"] = json!("image_text");
    }
    Ok(())
}
async fn freeze_reference_input(
    db: std::sync::Arc<crate::db::Db>,
    data_dir: std::path::PathBuf,
    input_id: String,
    request: &Value,
    refs: Vec<Value>,
) -> AppResult<Vec<reference_images::Snapshot>> {
    if !needs_reference_images(request)? {
        // Pure text never queries entry_image, opens files or creates image snapshots.
        return Ok(vec![]);
    }
    let sources =
        crate::commands::blocking(db, move |c| reference_images::sources(c, &refs)).await?;
    tokio::task::spawn_blocking(move || reference_images::freeze(&data_dir, &input_id, sources))
        .await
        .map_err(plugin::join_err)?
}
pub async fn prepare(state: &AppState, mut req: Value) -> AppResult<Value> {
    if !req.is_object() {
        return Err(AppError::bad_request("实验输入必须为对象"));
    }
    let mode = field(&req, "mode")?.to_string();
    if !matches!(mode.as_str(), "draw" | "reverse") {
        return Err(AppError::bad_request("未知实验模式"));
    }
    normalize_input(&mut req)?;
    let count = req["count"].as_u64().unwrap();
    let conn = plugin::plugin_conn(state).await?;
    let caps = call(
        state,
        &conn,
        reqwest::Method::GET,
        "capabilities",
        &[],
        None,
        60,
    )
    .await?;
    validate_caps(&caps)?;
    require_reference_content(&caps, &req)?;
    let id = uuid::Uuid::new_v4().to_string();
    let characters: Vec<String> =
        serde_json::from_value(req.get("characters").cloned().unwrap_or(json!([])))?;
    if characters.len() > 6 {
        return Err(AppError::bad_request("最多6个角色"));
    }
    let mut character = req["character"].as_str().unwrap_or("").trim().to_string();
    if character.is_empty() && characters.is_empty() {
        character = array(&caps, "characters")?
            .iter()
            .filter_map(Value::as_str)
            .max_by_key(|c| store::hash(format!("{id}:{c}").as_bytes()))
            .ok_or_else(|| AppError::bad_request("插件角色池为空，请先配置角色池"))?
            .to_owned();
    }
    if !characters.is_empty() {
        character.clear();
    }
    req["character"] = json!(character);
    req["characters"] = json!(characters);
    req["count"] = json!(count);
    let refs = if mode == "reverse" {
        vec![]
    } else {
        let q = input_query(&req)?;
        let pool =
            crate::commands::blocking(state.db.clone(), move |c| load_references(c, &q)).await?;
        choose_references(pool, &req, &id)?
    };
    let image_snapshots = freeze_reference_input(
        state.db.clone(),
        state.data_dir.clone(),
        id.clone(),
        &req,
        refs.clone(),
    )
    .await?;
    if mode == "reverse" {
        let reverse = req
            .get_mut("reverse")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| AppError::bad_request("缺少反推设置"))?;
        let path = reverse
            .remove("image_path")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| AppError::bad_request("请选择反推原图"))?;
        let img = tokio::task::spawn_blocking(move || prepare_image(&path))
            .await
            .map_err(plugin::join_err)??;
        reverse.insert("image_base64".into(), img["image_base64"].clone());
        reverse.insert("mime".into(), img["mime"].clone());
        req["source_image"] = json!({"sha256":img["sha256"],"width":img["width"],"height":img["height"],"bytes":img["bytes"],"shrunk":img["shrunk"]});
    }
    req["references"] = json!(refs);
    let reference_ids: Vec<_> = refs.iter().map(|r| r["entry_id"].clone()).collect();
    let snapshot = json!({"input_id":id,"plugin_identity":identity(&conn),"request":req,"reference_image_snapshots":image_snapshots,
        "reference_content_wire":caps["reference_text_supported"] == true});
    store::write_once(&store::path(&state.data_dir, "inputs", &id)?, &snapshot)?;
    Ok(
        json!({"input_id":id,"character":character,"characters":characters,"reference_count":refs.len(),"reference_content":req["reference_content"],"reference_image_count":image_snapshots.len(),"reference_ids":reference_ids,"source_image":req.get("source_image")}),
    )
}
fn validate_plan(batch: &Value) -> AppResult<()> {
    store::valid_id(field(batch, "plan_id")?)?;
    let mode = field(batch, "mode")?;
    if !matches!(mode, "draw" | "reverse") {
        return Err(protocol("方案模式不合法"));
    }
    let settings = &batch["settings"];
    let count = settings["count"]
        .as_u64()
        .filter(|n| (1..=8).contains(n))
        .ok_or_else(|| protocol("方案张数不合法"))? as usize;
    for key in ["character", "requirements", "provider_id"] {
        field(settings, key)?;
    }
    if settings["provider_id"].as_str().unwrap().trim().is_empty() {
        return Err(protocol("方案没有提示词模型"));
    }
    let plans = array(batch, "plans")?;
    if plans.len() != if mode == "reverse" { 1 } else { count } {
        return Err(protocol("方案数量与模式/冻结张数不一致"));
    }
    let mut ids = HashSet::new();
    for p in plans {
        let id = field(p, "plan_id")?;
        store::valid_id(id)?;
        if !ids.insert(id) {
            return Err(protocol("批次内方案 ID 重复"));
        }
        for key in ["character", "requirements", "provider_id"] {
            if p[key] != settings[key] {
                return Err(protocol(format!("子方案的 {key} 与冻结设置不一致")));
            }
        }
        if p["free_only"] != true
            || !matches!(
                p["model"].as_str(),
                Some("nai-diffusion-5-full" | "nai-diffusion-5-curated")
            )
            || !p["steps"].as_u64().is_some_and(|n| (1..=28).contains(&n))
            || !matches!(
                p["size"].as_str(),
                Some("832x1216" | "1216x832" | "1024x1024")
            )
            || p["parameters"]["other"] != "0"
            || p["prompt"].as_str().map_or(true, |s| s.trim().is_empty())
        {
            return Err(protocol("方案不满足三档免费尺寸/V5/步数/提示词约束"));
        }
        let orientation = field(settings, "orientation")?;
        let expected = match orientation {
            "portrait" => Some("832x1216"),
            "landscape" => Some("1216x832"),
            "square" => Some("1024x1024"),
            "auto" => None,
            _ => return Err(protocol("方案方向不合法")),
        };
        if expected.is_some_and(|size| p["size"] != size) {
            return Err(protocol("方案尺寸没有遵守指定方向"));
        }
    }
    Ok(())
}
fn validate_plan_for(batch: &Value, request: &Value) -> AppResult<()> {
    validate_plan(batch)?;
    if batch["mode"] != request["mode"] {
        return Err(protocol("返回方案与请求模式不一致"));
    }
    let settings = &batch["settings"];
    for (key, default) in [
        ("provider_id", Value::Null),
        ("character", json!("")),
        ("characters", json!([])),
        ("requirements", json!("")),
        ("count", json!(3)),
        ("orientation", json!("portrait")),
        ("reference_mode", json!("sample")),
        ("reference_decisions", json!("include")),
        ("score_provider_ids", json!([])),
        ("jpeg", json!({"quality":88})),
        ("experiment_id", Value::Null),
    ] {
        if settings.get(key) != Some(request.get(key).unwrap_or(&default)) {
            return Err(protocol(format!("返回方案改变了冻结字段 {key}")));
        }
    }
    let bundles = array(request, "skill_bundles")?;
    let metadata: Vec<_> = bundles
        .iter()
        .map(|v| json!({"id":v["id"],"version":v["version"],"sha256":v["sha256"]}))
        .collect();
    if settings["skill_bundles"] != json!(metadata) {
        return Err(protocol("返回方案的 skill 版本/摘要与请求不一致"));
    }
    if batch["mode"] != "reverse" {
        let content = reference_content(request)?;
        if content == "text" {
            if request.get("reference_images").is_some()
                || settings["reference_content"] != "text"
                || settings["reference_image_count"].as_u64() != Some(0)
                || settings["reference_images"] != json!([])
            {
                return Err(protocol("返回方案未保持纯文字参考及零张参考图"));
            }
        } else if settings.get("reference_content").is_some()
            && settings["reference_content"] != "image_text"
        {
            return Err(protocol("返回方案改变了图文参考模式"));
        }
        if request.get("reference_content").is_some()
            && content == "image_text"
            && request["reference_images"]
                .as_array()
                .map_or(true, Vec::is_empty)
        {
            return Err(protocol("图文参考缺少完整图片清单"));
        }
        if let Some(images) = request.get("reference_images") {
            let count = images
                .as_array()
                .ok_or_else(|| protocol("冻结参考图片不是数组"))?
                .len();
            if settings.get("reference_images") != Some(images)
                || settings["reference_image_count"].as_u64() != Some(count as u64)
            {
                return Err(protocol("返回方案省略或替换了冻结参考图片"));
            }
        }
        let ids: Vec<_> = array(request, "references")?
            .iter()
            .map(|v| v["entry_id"].clone())
            .collect();
        if ids.iter().any(|v| v.as_str().is_none())
            || ids
                .iter()
                .filter_map(Value::as_str)
                .collect::<HashSet<_>>()
                .len()
                != ids.len()
        {
            return Err(protocol("冻结参考 ID 非法或重复"));
        }
        for p in array(batch, "plans")? {
            if p["reference_ids"] != json!(ids) || p["reference_mode"] != settings["reference_mode"]
            {
                return Err(protocol("返回方案省略或替换了冻结参考"));
            }
            let used = array(p, "used_reference_ids")?;
            if used.iter().any(|id| !ids.contains(id))
                || used
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<HashSet<_>>()
                    .len()
                    != used.len()
            {
                return Err(protocol("模型声明的借鉴来源不在实际材料内或重复"));
            }
        }
    }
    Ok(())
}

fn missing_reference_images(response: &Value, manifests: &[Value]) -> AppResult<HashSet<String>> {
    let valid: HashSet<_> = manifests
        .iter()
        .filter_map(|image| image["sha256"].as_str())
        .collect();
    let missing = array(response, "missing")?;
    let mut result = HashSet::new();
    for hash in missing {
        let hash = hash
            .as_str()
            .filter(|hash| valid.contains(hash))
            .ok_or_else(|| protocol("插件返回未知的参考图片摘要"))?;
        if !result.insert(hash.to_owned()) {
            return Err(protocol("插件返回重复的参考图片摘要"));
        }
    }
    Ok(result)
}

async fn sync_reference_images(
    state: &AppState,
    conn: &PluginConn,
    input_id: &str,
    manifests: &[Value],
) -> AppResult<()> {
    let mut seen = HashSet::new();
    let images: Vec<_> = manifests
        .iter()
        .filter(|image| seen.insert(image["sha256"].clone()))
        .map(|image| json!({"sha256":image["sha256"],"mime":image["mime"]}))
        .collect();
    let status = call(
        state,
        conn,
        reqwest::Method::POST,
        "reference_images_status",
        &[],
        Some(&json!({"images":images})),
        60,
    )
    .await?;
    let mut missing = missing_reference_images(&status, manifests)?;
    let pending: Vec<_> = manifests
        .iter()
        .filter(|image| {
            image["sha256"]
                .as_str()
                .is_some_and(|hash| missing.remove(hash))
        })
        .cloned()
        .collect();
    let mut uploads = futures_util::stream::iter(pending.into_iter().map(|image| async move {
        let data_dir = state.data_dir.clone();
        let input_id = input_id.to_owned();
        let metadata = image.clone();
        let (body, byte_count) = tokio::task::spawn_blocking(move || {
            reference_images::upload(&data_dir, &input_id, &metadata)
        })
        .await
        .map_err(plugin::join_err)??;
        let response = call(
            state,
            conn,
            reqwest::Method::POST,
            "reference_image_upload",
            &[],
            Some(&body),
            180,
        )
        .await
        .map_err(|error| {
            AppError::new(
                &error.code,
                format!(
                    "图包 {} 的参考图上传失败：{}",
                    image["entry_id"].as_str().unwrap_or("未知"),
                    error.message
                ),
            )
        })?;
        if response["sha256"] != image["sha256"]
            || response["mime"] != image["mime"]
            || response["bytes"].as_u64() != Some(byte_count)
        {
            return Err(protocol(format!(
                "图包 {} 的参考图片上传响应与冻结文件不一致",
                field(&image, "entry_id")?
            )));
        }
        Ok(())
    }))
    .buffer_unordered(4);
    while let Some(result) = uploads.next().await {
        result?;
    }
    Ok(())
}

/// Use only the immutable input's content choice. Text mode returns before any
/// reference-image filesystem work and never asks the transport to upload images.
fn frozen_reference_payload(
    root: &Path,
    input_id: &str,
    saved: &Value,
    caps: &Value,
) -> AppResult<(Value, Option<Vec<Value>>)> {
    let mut body = saved["request"].clone();
    if body["mode"] == "reverse" {
        body.as_object_mut()
            .ok_or_else(|| protocol("冻结输入损坏"))?
            .remove("reference_content");
        return Ok((body, None));
    }
    require_reference_content(caps, &body)?;
    if reference_content(&body)? == "text" {
        body["reference_content"] = json!("text");
        return Ok((body, None));
    }
    let manifests = reference_images::manifest(root, input_id, saved, array(&body, "references")?)?;
    body["reference_images"] = json!(manifests);
    if saved["reference_content_wire"] == true {
        if caps["reference_text_supported"] != true {
            return Err(AppError::new(
                "studio_reference_text_unsupported",
                "插件不再支持此实验冻结的参考协议，请恢复插件版本后重试；未更改本轮材料",
            ));
        }
        body["reference_content"] = json!("image_text");
    } else {
        // Field presence participates in the plugin's input hash. Inputs prepared
        // against older plugins keep their wire shape even after a plugin upgrade.
        body.as_object_mut().unwrap().remove("reference_content");
    }
    Ok((body, Some(manifests)))
}

pub async fn plan(state: &AppState, req: Value) -> AppResult<Value> {
    let input_id = field(&req, "input_id")?;
    let saved = store::read(&store::path(&state.data_dir, "inputs", input_id)?)?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved, &conn)?;
    let caps = if saved["request"]["mode"] != "reverse" {
        let value = call(
            state,
            &conn,
            reqwest::Method::GET,
            "capabilities",
            &[],
            None,
            60,
        )
        .await?;
        validate_caps(&value)?;
        value
    } else {
        Value::Null
    };
    let data_dir = state.data_dir.clone();
    let frozen_id = input_id.to_owned();
    let reference_query = if saved["request"]["mode"] == "reverse" {
        Value::Null
    } else {
        serde_json::to_value(input_query(&saved["request"])?)?
    };
    let (mut body, manifests) = tokio::task::spawn_blocking(move || {
        frozen_reference_payload(&data_dir, &frozen_id, &saved, &caps)
    })
    .await
    .map_err(plugin::join_err)??;
    if let Some(manifests) = manifests {
        sync_reference_images(state, &conn, input_id, &manifests).await?;
    }
    body["experiment_id"] = json!(input_id);
    for key in [
        "provider_id",
        "brief",
        "multi_brief",
        "score_provider_ids",
        "jpeg",
    ] {
        if let Some(v) = req.get(key) {
            body[key] = v.clone();
        }
    }
    if let Some(reverse) = req.get("reverse").and_then(Value::as_object) {
        for (key, v) in reverse {
            if matches!(
                key.as_str(),
                "prompt"
                    | "separate_prompt"
                    | "use_separate"
                    | "style_mode"
                    | "style_review_provider_id"
                    | "style_top_k"
            ) {
                body["reverse"][key] = v.clone();
            }
        }
    }
    let ids: Vec<String> =
        serde_json::from_value(req.get("skill_ids").cloned().unwrap_or(json!([])))?;
    body["skill_bundles"] = json!(skills::bundles(&state.data_dir, &ids)?
        .iter()
        .map(|s| json!({"id":s.id,"version":s.version,"sha256":s.sha256,"text":s.text}))
        .collect::<Vec<_>>());
    let object = body
        .as_object_mut()
        .ok_or_else(|| protocol("冻结输入损坏"))?;
    object.retain(|key, _| {
        matches!(
            key.as_str(),
            "experiment_id"
                | "mode"
                | "provider_id"
                | "character"
                | "characters"
                | "requirements"
                | "count"
                | "orientation"
                | "reference_mode"
                | "reference_content"
                | "reference_decisions"
                | "references"
                | "reference_images"
                | "style_name"
                | "artist_ratio"
                | "skill_bundles"
                | "brief"
                | "multi_brief"
                | "reverse"
                | "score_provider_ids"
                | "jpeg"
        )
    });
    let batch = call(
        state,
        &conn,
        reqwest::Method::POST,
        "plan",
        &[],
        Some(&body),
        600,
    )
    .await?;
    validate_plan_for(&batch, &body)?;
    let plan_id = field(&batch, "plan_id")?;
    store::write_once(
        &store::path(&state.data_dir, "plans", plan_id)?,
        &json!({"plugin_identity":identity(&conn),"input_id":input_id,"batch":batch,"request":body,"reference_query":reference_query}),
    )?;
    Ok(batch)
}
async fn recheck(state: &AppState, saved: &Value) -> AppResult<()> {
    let req = &saved["request"];
    if req["mode"] == "reverse" {
        return Ok(());
    }
    let q = saved_reference_query(saved)?;
    let pool = crate::commands::blocking(state.db.clone(), move |c| load_references(c, &q)).await?;
    let eligible: HashSet<_> = pool.iter().filter_map(|r| r["entry_id"].as_str()).collect();
    if array(req, "references")?.iter().any(|r| {
        r["entry_id"]
            .as_str()
            .map_or(true, |id| !eligible.contains(id))
    }) {
        return Err(AppError::bad_request(
            "冻结方案的参考已不符合当前范围（被删除、改分级、移出决策范围或拉黑），请重新构思",
        ));
    }
    Ok(())
}
fn saved_reference_query(saved: &Value) -> AppResult<ReferenceQuery> {
    match saved.get("reference_query") {
        Some(q) if !q.is_null() => Ok(serde_json::from_value(q.clone())?),
        _ => input_query(&saved["request"]),
    }
}
fn validate_run(run: &Value) -> AppResult<()> {
    store::valid_id(field(run, "run_id")?)?;
    if !matches!(
        run["status"].as_str(),
        Some("queued" | "running" | "done" | "error" | "cancelled" | "partial")
    ) {
        return Err(protocol("实验运行状态不合法"));
    }
    if run["free_only"] != true || uuid::Uuid::parse_str(field(run, "request_id")?).is_err() {
        return Err(protocol("运行缺少免费模式或有效请求 ID"));
    }
    let batch = &run["plan"];
    validate_plan(batch)?;
    if run["plan_id"] != batch["plan_id"] || run["mode"] != batch["mode"] {
        return Err(protocol("运行与冻结方案身份不一致"));
    }
    let count = batch["settings"]["count"].as_u64().unwrap() as usize;
    let images = array(run, "images")?;
    if images.len() != count {
        return Err(protocol("运行图片数量与冻结张数不一致"));
    }
    for (idx, image) in images.iter().enumerate() {
        let expected = &batch["plans"][if batch["mode"] == "reverse" { 0 } else { idx }];
        if image["idx"].as_u64() != Some(idx as u64)
            || image["plan_id"] != expected["plan_id"]
            || !image["seed"]
                .as_u64()
                .is_some_and(|seed| seed <= u64::from(u32::MAX))
        {
            return Err(protocol("图片编号、种子或对应构思不一致"));
        }
        if !matches!(
            image["status"].as_str(),
            Some("pending" | "running" | "done" | "error" | "cancelled")
        ) {
            return Err(protocol("图片运行状态不合法"));
        }
        if image["status"] == "done" && image["file"].as_str().map_or(true, |s| s.is_empty()) {
            return Err(protocol("已完成图片缺少文件标识"));
        }
        if run["status"] == "done" && image["status"] != "done" {
            return Err(protocol("部分图片未完成，不能标记整个运行成功"));
        }
    }
    Ok(())
}
fn save_run(state: &AppState, conn: &PluginConn, run: &Value) -> AppResult<Value> {
    validate_run(run)?;
    static SAVE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _guard = SAVE_LOCK.lock();
    let path = store::path(&state.data_dir, "runs", field(run, "run_id")?)?;
    let run = if path.exists() {
        let previous = store::read(&path)?;
        check_identity(&previous, conn)?;
        merge_run(&previous["run"], run)?
    } else {
        run.clone()
    };
    store::write(&path, &json!({"plugin_identity":identity(conn),"run":run}))?;
    Ok(run)
}
fn merge_run(previous: &Value, next: &Value) -> AppResult<Value> {
    for key in ["run_id", "plan_id", "request_id", "mode", "plan"] {
        if previous[key] != next[key] {
            return Err(protocol(format!("更新运行改变了冻结字段 {key}")));
        }
    }
    let active = |v: &Value| matches!(v["status"].as_str(), Some("queued" | "running"));
    if !active(previous) && active(next) {
        return Ok(previous.clone());
    }
    let mut merged = next.clone();
    for (idx, image) in array(previous, "images")?.iter().enumerate() {
        if image["seed"] != next["images"][idx]["seed"] {
            return Err(protocol("更新运行改变了图片种子"));
        }
        if matches!(
            image["status"].as_str(),
            Some("done" | "error" | "cancelled")
        ) && matches!(
            next["images"][idx]["status"].as_str(),
            Some("pending" | "running")
        ) {
            merged["images"][idx] = image.clone();
        }
    }
    if previous["cancel_requested"] == true {
        merged["cancel_requested"] = json!(true);
    }
    Ok(merged)
}
fn canonical_start(req: &Value, count: u64) -> AppResult<Value> {
    let object = req
        .as_object()
        .ok_or_else(|| AppError::bad_request("开始参数须为对象"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "plan_id" | "request_id" | "seed"))
    {
        return Err(AppError::bad_request("开始只能指定方案、请求 ID 和种子"));
    }
    store::valid_id(field(req, "plan_id")?)?;
    if uuid::Uuid::parse_str(field(req, "request_id")?).is_err() {
        return Err(AppError::bad_request("出图请求需要 UUID 幂等编号"));
    }
    let seed = req.get("seed").cloned().unwrap_or(Value::Null);
    if !seed.is_null()
        && !seed.as_u64().is_some_and(|n| {
            n.checked_add(count.saturating_sub(1))
                .is_some_and(|last| last <= u64::from(u32::MAX))
        })
    {
        return Err(AppError::bad_request(
            "种子须为有效整数，且加上张数后不能超过32位范围",
        ));
    }
    Ok(json!({"plan_id":req["plan_id"],"request_id":req["request_id"],"seed":seed}))
}
fn validate_started(run: &Value, batch: &Value, request: &Value) -> AppResult<()> {
    validate_run(run)?;
    if run["plan"] != *batch
        || run["plan_id"] != request["plan_id"]
        || run["request_id"] != request["request_id"]
    {
        return Err(protocol("开始响应对应了另一份方案或请求"));
    }
    if let Some(seed) = request["seed"].as_u64() {
        if array(run, "images")?
            .iter()
            .enumerate()
            .any(|(idx, img)| img["seed"].as_u64() != Some(seed + idx as u64))
        {
            return Err(protocol("开始响应改变了指定种子"));
        }
    }
    Ok(())
}
pub async fn start(state: &AppState, req: Value) -> AppResult<Value> {
    let plan_id = field(&req, "plan_id")?;
    let request_id = field(&req, "request_id")?;
    if uuid::Uuid::parse_str(request_id).is_err() {
        return Err(AppError::bad_request("出图请求需要 UUID 幂等编号"));
    }
    let saved = store::read(&store::path(&state.data_dir, "plans", plan_id)?)?;
    validate_plan_for(&saved["batch"], &saved["request"])?;
    let req = canonical_start(&req, saved["batch"]["settings"]["count"].as_u64().unwrap())?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved, &conn)?;
    recheck(state, &saved).await?;
    // Persist the exact idempotency key before any network side effect. An unknown
    // response is recoverable with this same request, never a generated replacement.
    let attempt_id = store::hash(format!("{}:{request_id}", identity(&conn)).as_bytes());
    store::write_once(
        &store::path(&state.data_dir, "starts", &attempt_id)?,
        &json!({"plugin_identity":identity(&conn),"request":req}),
    )?;
    let run = call(
        state,
        &conn,
        reqwest::Method::POST,
        "start",
        &[],
        Some(&req),
        90,
    )
    .await?;
    validate_started(&run, &saved["batch"], &req)?;
    save_run(state, &conn, &run)
}
pub async fn status(state: &AppState, run_id: String, cancel: bool) -> AppResult<Value> {
    store::valid_id(&run_id)?;
    let conn = plugin::plugin_conn(state).await?;
    let path = store::path(&state.data_dir, "runs", &run_id)?;
    if path.exists() {
        check_identity(&store::read(&path)?, &conn)?;
    }
    let run = if cancel {
        call(
            state,
            &conn,
            reqwest::Method::POST,
            "cancel",
            &[],
            Some(&json!({"run_id":run_id})),
            90,
        )
        .await?
    } else {
        call(
            state,
            &conn,
            reqwest::Method::GET,
            "status",
            &[("run_id", run_id.clone())],
            None,
            60,
        )
        .await?
    };
    if run["run_id"] != run_id {
        return Err(protocol("运行身份不匹配"));
    }
    save_run(state, &conn, &run)
}
pub async fn list(state: &AppState, limit: usize) -> AppResult<Value> {
    if !(1..=100).contains(&limit) {
        return Err(AppError::bad_request("列表上限须为1–100"));
    }
    let conn = plugin::plugin_conn(state).await?;
    let value = call(
        state,
        &conn,
        reqwest::Method::GET,
        "list",
        &[("limit", limit.to_string())],
        None,
        60,
    )
    .await?;
    let runs = value.as_array().ok_or_else(|| protocol("列表不是数组"))?;
    for run in runs {
        validate_run(run)?;
    }
    let saved = runs
        .iter()
        .map(|run| save_run(state, &conn, run))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(json!(saved))
}
async fn fetch_image(
    state: &AppState,
    conn: &PluginConn,
    run_id: &str,
    idx: usize,
    format: &str,
    quality: u8,
) -> AppResult<std::path::PathBuf> {
    let ext = if format == "jpeg" { "jpg" } else { "png" };
    let path = state
        .data_dir
        .join("studio/images")
        .join(identity(conn))
        .join(run_id)
        .join(format!("{idx}-{format}-{quality}.{ext}"));
    if path.is_file() {
        return Ok(path);
    }
    let v = call(
        state,
        conn,
        reqwest::Method::GET,
        "image",
        &[
            ("run_id", run_id.into()),
            ("idx", idx.to_string()),
            ("format", format.into()),
            ("quality", quality.to_string()),
            ("full", "1".into()),
        ],
        None,
        90,
    )
    .await?;
    let b64 = field(&v, "data_base64")?;
    if b64.len() > 44 * 1024 * 1024 {
        return Err(protocol("图片响应超过大小限制"));
    }
    let bytes = STANDARD
        .decode(b64)
        .map_err(|_| protocol("图片 Base64 不合法"))?;
    let expected = if format == "jpeg" {
        image::ImageFormat::Jpeg
    } else {
        image::ImageFormat::Png
    };
    if image::guess_format(&bytes)? != expected {
        return Err(protocol("图片实际格式不符合请求"));
    }
    let dims = image::ImageReader::with_format(std::io::Cursor::new(&bytes), expected)
        .into_dimensions()?;
    if dims.0 == 0 || dims.1 == 0 || u64::from(dims.0) * u64::from(dims.1) > 32_000_000 {
        return Err(protocol("图片尺寸超过限制"));
    }
    let bytes = tokio::task::spawn_blocking(move || -> AppResult<Vec<u8>> {
        validate_image_bytes(&bytes, expected)?;
        Ok(bytes)
    })
    .await
    .map_err(plugin::join_err)??;
    store::bytes(&path, &bytes)?;
    Ok(path)
}
fn validate_image_bytes(bytes: &[u8], format: image::ImageFormat) -> AppResult<()> {
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| protocol("图片正文损坏，未缓存为成功结果"))?;
    Ok(())
}
pub async fn image(
    state: &AppState,
    run_id: String,
    idx: usize,
    format: String,
    quality: u8,
) -> AppResult<Value> {
    store::valid_id(&run_id)?;
    if idx >= 8 || !matches!(format.as_str(), "png" | "jpeg") || !(1..=100).contains(&quality) {
        return Err(AppError::bad_request("图片编号、格式或JPEG质量不合法"));
    }
    let saved = store::read(&store::path(&state.data_dir, "runs", &run_id)?)?;
    let item = array(&saved["run"], "images")?
        .get(idx)
        .ok_or_else(|| AppError::not_found("该编号没有图片"))?;
    image_ready(item)?;
    let conn = plugin::plugin_conn(state).await?;
    check_identity(&saved, &conn)?;
    let original = fetch_image(state, &conn, &run_id, idx, "png", 88).await?;
    let path = if format == "jpeg" {
        fetch_image(state, &conn, &run_id, idx, "jpeg", quality).await?
    } else {
        original.clone()
    };
    Ok(
        json!({"path":path,"original_path":original,"format":format,"quality":if format=="jpeg" {Some(quality)} else {None},"original_bytes":std::fs::metadata(&original)?.len(),"encoded_bytes":std::fs::metadata(&path)?.len()}),
    )
}
fn image_ready(item: &Value) -> AppResult<()> {
    match item["status"].as_str() {
        Some("done") => Ok(()),
        Some("pending" | "running") => {
            Err(AppError::new("studio_image_pending", "图片尚未生成完成"))
        }
        Some("error" | "cancelled") => Err(AppError::new(
            "studio_image_failed",
            item["error"]
                .as_str()
                .unwrap_or("该图片生成失败或已取消，没有可读取的图片"),
        )),
        _ => Err(protocol("图片状态无效")),
    }
}
pub async fn compare(state: &AppState, req: Value) -> AppResult<Value> {
    let images = array(&req, "images")?;
    if !(2..=8).contains(&images.len()) {
        return Err(AppError::bad_request("视觉模型每次对比2–8张图"));
    }
    let mut wire = vec![];
    let mut seen = HashSet::new();
    let mut total = 0;
    for img in images {
        let id = field(img, "id")?;
        if id.trim().is_empty() || id.len() > 128 || !seen.insert(id.to_owned()) {
            return Err(AppError::bad_request("图片编号为空、过长或重复"));
        }
        let path = field(img, "path")?.to_string();
        let p = tokio::task::spawn_blocking(move || prepare_image(&path))
            .await
            .map_err(plugin::join_err)??;
        total += p["image_base64"].as_str().unwrap().len();
        if total > 40 * 1024 * 1024 {
            return Err(AppError::bad_request("对比图片总量过大，请减少图片数"));
        }
        wire.push(json!({"id":id,"mime":p["mime"],"data_base64":p["image_base64"]}));
    }
    let conn = plugin::plugin_conn(state).await?;
    let body = json!({"provider_id":field(&req,"provider_id")?,"prompt":req["prompt"].as_str().unwrap_or(""),"images":wire});
    let result = call(
        state,
        &conn,
        reqwest::Method::POST,
        "compare",
        &[],
        Some(&body),
        300,
    )
    .await?;
    validate_comparison(&result, &body)?;
    let id = uuid::Uuid::new_v4().to_string();
    store::write(
        &store::path(&state.data_dir, "comparisons", &id)?,
        &json!({"created_at":chrono::Utc::now().to_rfc3339(),"request":req,"result":result}),
    )?;
    Ok(result)
}
fn validate_comparison(result: &Value, request: &Value) -> AppResult<()> {
    if result["provider_id"] != request["provider_id"]
        || !result["text"].is_string()
        || result["latency_ms"].as_u64().is_none()
    {
        return Err(protocol("画风复审返回的模型、原文或耗时无效"));
    }
    let request_images = array(request, "images")?;
    let ordered_ids: Vec<_> = request_images
        .iter()
        .filter_map(|v| v["id"].as_str())
        .collect();
    let ids: HashSet<_> = ordered_ids.iter().copied().collect();
    if let Some(ranking) = result.get("ranking").and_then(Value::as_array) {
        let reference = ordered_ids
            .first()
            .copied()
            .ok_or_else(|| protocol("画风复审缺少基准图"))?;
        if result["reference_id"].as_str() != Some(reference) {
            return Err(protocol("画风复审返回了错误的基准图"));
        }
        // Empty is a valid unparsed response: the raw model text remains available
        // for diagnosis. A parsed response must contain the complete top 1–3.
        let expected = (ordered_ids.len() - 1).min(3);
        if !ranking.is_empty() && ranking.len() != expected {
            return Err(protocol("画风复审排名数量不完整"));
        }
        let mut seen = HashSet::new();
        for (index, item) in ranking.iter().enumerate() {
            let image_id = field(item, "image_id")?;
            let rank = item["rank"].as_u64();
            let similarity = item["similarity"].as_f64();
            if image_id == reference
                || !ids.contains(image_id)
                || !seen.insert(image_id)
                || rank != Some((index + 1) as u64)
                || !similarity.is_some_and(|score| score.is_finite() && (0.0..=100.0).contains(&score))
                || !item["reason"].is_string()
            {
                return Err(protocol("画风复审含未知、重复或无效的排名项"));
            }
        }
        return Ok(());
    }

    // Read responses from pre-ranking plugins so existing experiment history and
    // staggered desktop/plugin upgrades do not become inaccessible.
    let mut seen = HashSet::new();
    for pair in array(result, "pairs")? {
        let a = field(pair, "left_id")?;
        let b = field(pair, "right_id")?;
        let key = if a < b { (a, b) } else { (b, a) };
        if a == b
            || !ids.contains(a)
            || !ids.contains(b)
            || !seen.insert(key)
            || !pair["reason"].is_string()
            || !pair
                .get("same_style")
                .is_some_and(|v| v.is_boolean() || v.is_null())
        {
            return Err(protocol("画风复审含未知、重复或无效的图片对"));
        }
    }
    Ok(())
}
pub fn workspace_get(root: &Path) -> AppResult<Value> {
    let p = store::path(root, "workspace", "current")?;
    if !p.exists() {
        return Ok(Value::Null);
    }
    store::read(&p)
}
pub fn workspace_save(root: &Path, state: &Value) -> AppResult<()> {
    if !state.is_object() || serde_json::to_vec(state)?.len() > 16 * 1024 * 1024 {
        return Err(AppError::bad_request("实验工作区必须为不超过16 MiB的对象"));
    }
    store::write(&store::path(root, "workspace", "current")?, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn synthetic_plan(mode: &str, count: usize) -> (Value, Value) {
        let request = json!({"mode":mode,"provider_id":"fixture/model","character":"固定角色","characters":[],"requirements":"  沙滩跳舞\n全身  ","count":count,"orientation":"portrait","reference_mode":"all","reference_decisions":"all","references":[{"entry_id":"r1"},{"entry_id":"r2"}],"skill_bundles":[],"score_provider_ids":[],"jpeg":{"quality":88},"experiment_id":"12345678-1234-4234-9234-123456789abc"});
        let mut settings = request.clone();
        settings.as_object_mut().unwrap().remove("references");
        settings.as_object_mut().unwrap().remove("mode");
        let plans:Vec<_>=(0..if mode=="reverse" {1} else {count}).map(|i|json!({"plan_id":format!("child{i}"),"character":request["character"],"requirements":request["requirements"],"provider_id":request["provider_id"],"free_only":true,"model":"nai-diffusion-5-full","steps":28,"size":"832x1216","prompt":format!("scene {i}"),"parameters":{"other":"0"},"reference_mode":"all","reference_ids":["r1","r2"],"used_reference_ids":["r1"]})).collect();
        (
            json!({"plan_id":"batch1","mode":mode,"settings":settings,"plans":plans,"warnings":[]}),
            request,
        )
    }
    fn synthetic_run(mode: &str, count: usize) -> Value {
        let (batch, _) = synthetic_plan(mode, count);
        let images:Vec<_>=(0..count).map(|i|json!({"idx":i,"plan_id":batch["plans"][if mode=="reverse"{0}else{i}]["plan_id"],"seed":100+i,"status":"pending","file":null,"error":null})).collect();
        json!({"run_id":"run1","plan_id":batch["plan_id"],"mode":mode,"plan":batch,"request_id":"12345678-1234-4234-9234-123456789abc","status":"queued","images":images,"free_only":true,"cancel_requested":false})
    }
    #[test]
    fn studio_query_always_negotiates_independent_protocol() {
        assert_eq!(feature_query(&[]), vec![("features", "studio_v1".into())]);
        assert_eq!(
            feature_query(&[("run_id", "run1".into()), ("features", "legacy".into())]),
            vec![("run_id", "run1".into()), ("features", "studio_v1".into())]
        );
    }
    #[test]
    fn preparation_freezes_explicit_defaults_and_normalized_roles() {
        let mut req =
            json!({"mode":"draw","characters":[" A ","B"],"requirements":"  keep newlines\n "});
        normalize_input(&mut req).unwrap();
        assert_eq!(req["reference_mode"], "all");
        assert_eq!(req["count"], 3);
        assert_eq!(req["characters"], json!(["A", "B"]));
        assert_eq!(req["requirements"], "  keep newlines\n ");
        for bad in [
            json!(0),
            json!(9),
            json!(true),
            json!("3"),
            Value::Null,
            json!(2.5),
        ] {
            assert!(normalize_input(&mut json!({"count":bad})).is_err());
        }
        for roles in [json!([" "]), json!(["A", " A"]), json!([null])] {
            assert!(normalize_input(&mut json!({"characters":roles})).is_err());
        }
    }
    #[test]
    fn plan_checks_identity_counts_raw_requirements_and_reference_integrity() {
        for mode in ["draw", "reverse"] {
            let (b, r) = synthetic_plan(mode, 3);
            validate_plan_for(&b, &r).unwrap();
        }
        let (valid, request) = synthetic_plan("draw", 3);
        for key in [
            "character",
            "requirements",
            "provider_id",
            "experiment_id",
            "reference_mode",
            "reference_decisions",
        ] {
            let mut wrong = valid.clone();
            wrong["settings"][key] = json!("changed");
            assert!(validate_plan_for(&wrong, &request).is_err(), "{key}");
        }
        let mut wrong = valid.clone();
        wrong["plans"].as_array_mut().unwrap().pop();
        assert!(validate_plan_for(&wrong, &request).is_err());
        let mut wrong = valid.clone();
        wrong["plans"][1]["plan_id"] = wrong["plans"][0]["plan_id"].clone();
        assert!(validate_plan_for(&wrong, &request).is_err());
        let mut wrong = valid.clone();
        wrong["plans"][1]["reference_ids"] = json!(["r1"]);
        assert!(validate_plan_for(&wrong, &request).is_err());
        let mut wrong = valid.clone();
        wrong["plans"][0]["used_reference_ids"] = json!(["foreign"]);
        assert!(validate_plan_for(&wrong, &request).is_err());
        let mut wrong = valid.clone();
        wrong["plans"][0]["size"] = json!("1024x1024");
        assert!(validate_plan_for(&wrong, &request).is_err());
        let mut wrong = valid.clone();
        wrong["settings"]["skill_bundles"] = json!([{"id":"wrong","version":"v1","sha256":"abc"}]);
        assert!(validate_plan_for(&wrong, &request).is_err());
        // Multirole tags are resolved by the plugin, while original names stay in settings.
        let mut multiple = valid;
        let mut request = request;
        multiple["settings"]["character"] = json!("");
        request["character"] = json!("");
        multiple["settings"]["characters"] = json!(["原角色甲", "原角色乙"]);
        request["characters"] = multiple["settings"]["characters"].clone();
        for p in multiple["plans"].as_array_mut().unwrap() {
            p["character"] = json!("");
            p["characters"] = json!(["resolved tag a", "resolved tag b"]);
        }
        validate_plan_for(&multiple, &request).unwrap();
    }
    #[test]
    fn run_images_are_bound_to_their_actual_concept() {
        for mode in ["draw", "reverse"] {
            validate_run(&synthetic_run(mode, 3)).unwrap();
        }
        let valid = synthetic_run("draw", 3);
        let mut bad = valid.clone();
        bad["images"][1]["plan_id"] = bad["images"][0]["plan_id"].clone();
        assert!(validate_run(&bad).is_err());
        let mut bad = valid.clone();
        bad["status"] = json!("done");
        assert!(validate_run(&bad).is_err());
        let mut bad = valid.clone();
        bad["images"][0]["idx"] = json!(2);
        assert!(validate_run(&bad).is_err());
        let mut bad = valid.clone();
        bad["images"].as_array_mut().unwrap().pop();
        assert!(validate_run(&bad).is_err());
        let request = json!({"plan_id":"batch1","request_id":valid["request_id"],"seed":100});
        validate_started(&valid, &valid["plan"], &request).unwrap();
        let mut wrong_request = request.clone();
        wrong_request["request_id"] = json!("00000000-0000-4000-8000-000000000000");
        assert!(validate_started(&valid, &valid["plan"], &wrong_request).is_err());
        for seed in [json!(true), json!("100"), json!(-1), json!(u32::MAX)] {
            let mut req = request.clone();
            req["seed"] = seed;
            assert!(canonical_start(&req, 3).is_err());
        }
        assert_eq!(canonical_start(&request, 3).unwrap(), request);
    }
    #[test]
    fn failed_images_stop_loading_and_stale_status_cannot_remove_results() {
        assert_eq!(
            image_ready(&json!({"status":"pending"})).unwrap_err().code,
            "studio_image_pending"
        );
        for status in ["error", "cancelled"] {
            assert_eq!(
                image_ready(&json!({"status":status,"error":"failed"}))
                    .unwrap_err()
                    .code,
                "studio_image_failed"
            );
        }
        image_ready(&json!({"status":"done"})).unwrap();
        let mut known = synthetic_run("draw", 3);
        let stale = known.clone();
        known["status"] = json!("running");
        known["cancel_requested"] = json!(true);
        known["images"][0]["status"] = json!("done");
        known["images"][0]["file"] = json!("x.png");
        let merged = merge_run(&known, &stale).unwrap();
        assert_eq!(merged["images"][0]["status"], "done");
        assert_eq!(merged["cancel_requested"], true);
        known["status"] = json!("error");
        assert_eq!(merge_run(&known, &stale).unwrap(), known);
    }
    #[test]
    fn preserved_reference_query_does_not_widen_at_start() {
        let q = ReferenceQuery {
            mode: "draw".into(),
            decisions: "all".into(),
            scope: vec![vec!["chosen".into()]],
            search: None,
            offset: None,
            limit: None,
        };
        let saved =
            json!({"reference_query":q,"request":{"mode":"draw","reference_decisions":"all"}});
        let loaded = saved_reference_query(&saved).unwrap();
        assert_eq!(loaded.scope, q.scope);
        assert_eq!(loaded.decisions, "all");
    }
    #[test]
    fn vision_ranking_uses_first_image_and_valid_candidates() {
        let request = json!({"provider_id":"fixture/model","images":[{"id":"a"},{"id":"b"},{"id":"c"},{"id":"d"}]});
        let result = json!({"provider_id":"fixture/model","text":"raw","latency_ms":3,"reference_id":"a","ranking":[
            {"rank":1,"image_id":"b","similarity":91,"reason":"closest"},
            {"rank":2,"image_id":"c","similarity":82.5,"reason":"second"},
            {"rank":3,"image_id":"d","similarity":70,"reason":"third"}
        ],"pairs":[]});
        validate_comparison(&result, &request).unwrap();
        let mut bad = result.clone();
        bad["reference_id"] = json!("b");
        assert!(validate_comparison(&bad, &request).is_err());
        let mut bad = result.clone();
        bad["ranking"][2]["image_id"] = json!("b");
        assert!(validate_comparison(&bad, &request).is_err());
        let mut bad = result.clone();
        bad["ranking"][1]["rank"] = json!(3);
        assert!(validate_comparison(&bad, &request).is_err());
        let mut bad = result.clone();
        bad["ranking"][0]["similarity"] = json!(101);
        assert!(validate_comparison(&bad, &request).is_err());
        let mut bad = result;
        bad["provider_id"] = json!("another/model");
        assert!(validate_comparison(&bad, &request).is_err());
        let unparsed = json!({"provider_id":"fixture/model","text":"raw","latency_ms":3,"reference_id":"a","ranking":[],"pairs":[]});
        validate_comparison(&unparsed, &request).unwrap();
    }
    #[test]
    fn legacy_visual_pairs_remain_readable_during_upgrade() {
        let request = json!({"provider_id":"fixture/model","images":[{"id":"a"},{"id":"b"}]});
        let result = json!({"provider_id":"fixture/model","text":"raw","latency_ms":3,"pairs":[{"left_id":"a","right_id":"b","same_style":null,"reason":"uncertain"}]});
        validate_comparison(&result, &request).unwrap();
    }
    #[test]
    fn actual_plugin_wire_fixtures() {
        let folder = std::env::var("STUDIO_FIXTURE_DIR")
            .unwrap_or_else(|_| format!("{}/tests/fixtures/studio", env!("CARGO_MANIFEST_DIR")));
        let read =
            |name: &str| store::read(&Path::new(&folder).join(format!("{name}.json"))).unwrap();
        validate_caps(&read("capabilities")).unwrap();
        for name in ["plan_draw", "plan_reverse"] {
            let batch = read(name);
            validate_plan(&batch).unwrap();
            let mut request = batch["settings"].clone();
            request["mode"] = batch["mode"].clone();
            request["references"] = json!(array(&batch["plans"][0], "reference_ids")
                .unwrap()
                .iter()
                .map(|id| json!({"entry_id":id}))
                .collect::<Vec<_>>());
            validate_plan_for(&batch, &request).unwrap();
        }
        validate_run(&read("run")).unwrap();
        validate_comparison(
            &read("compare"),
            &json!({"provider_id":"synthetic/creative","images":[{"id":"a"},{"id":"b"}]}),
        )
        .unwrap();
        let image = read("image");
        let data = STANDARD
            .decode(field(&image, "data_base64").unwrap())
            .unwrap();
        assert_eq!(image::guess_format(&data).unwrap(), image::ImageFormat::Png);
        validate_image_bytes(&data, image::ImageFormat::Png).unwrap();
        assert!(validate_image_bytes(&data[..33], image::ImageFormat::Png).is_err());
    }
    #[test]
    #[ignore = "requires plugin-generated synthetic reference-content wire fixtures"]
    fn actual_reference_content_wire_fixtures() {
        let folder = std::env::var("STUDIO_REFERENCE_CONTENT_FIXTURE_DIR").unwrap();
        let read =
            |name: &str| store::read(&Path::new(&folder).join(format!("{name}.json"))).unwrap();
        validate_caps(&read("capabilities")).unwrap();
        for name in [
            "draw_text_all",
            "draw_text_sample",
            "draw_image_text_all",
            "draw_image_text_sample",
            "legacy_image_text",
            "legacy_text",
        ] {
            let batch = read(&format!("plan_{name}"));
            let request = read(&format!("request_{name}"));
            validate_plan_for(&batch, &request).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        }
    }
    #[test]
    fn malformed_success_is_not_an_empty_list() {
        assert!(decode(reqwest::StatusCode::OK, "{\"data\":{}}", "list").is_err());
        assert_eq!(
            decode(
                reqwest::StatusCode::NOT_FOUND,
                "{\"status\":\"error\",\"message\":\"这张图片尚未生成\"}",
                "image"
            )
            .unwrap_err()
            .code,
            "studio_image_pending"
        );
        assert_eq!(
            decode(reqwest::StatusCode::NOT_FOUND, "not found", "capabilities")
                .unwrap_err()
                .code,
            "studio_unsupported"
        );
    }
    #[test]
    fn sampling_is_shared_by_nonce_and_all_never_truncates() {
        let pool: Vec<_> = (0..20)
            .map(|i| json!({"entry_id":format!("x{i}"),"tags":"synthetic"}))
            .collect();
        let req = json!({"reference_mode":"sample","reference_count":4});
        assert_eq!(
            choose_references(pool.clone(), &req, "same").unwrap(),
            choose_references(pool.clone(), &req, "same").unwrap()
        );
        assert_eq!(
            choose_references(pool.clone(), &json!({"reference_mode":"all"}), "same")
                .unwrap()
                .len(),
            20
        );
        assert!(choose_references(
            pool,
            &json!({"reference_ids":["missing"],"reference_mode":"sample"}),
            "x"
        )
        .is_err());
    }
    #[test]
    fn full_reference_material_over_old_request_budget_remains_exact() {
        let pool: Vec<_> = (0..300)
            .map(|i| json!({
                "entry_id":format!("full-{i}"), "tags":"完整构图".repeat(1500),
                "negative":format!("negative-{i}"),
                "character_prompts":[{"label":"role","prompt":format!("role-{i}"),"negative":"blur"}],
                "content_hash":format!("hash-{i}"), "decision":"include", "rating":"safe"
            }))
            .collect();
        assert!(serde_json::to_vec(&pool).unwrap().len() > 4 * 1024 * 1024);
        let req = json!({"reference_mode":"all", "reference_count":1,"reference_ids":["not-a-full-mode-filter"]});
        let selected = choose_references(pool.clone(), &req, "same").unwrap();
        assert_eq!(
            selected, pool,
            "every body and source survives full selection"
        );
        let dir = tempfile::tempdir().unwrap();
        let p = store::path(dir.path(), "inputs", "full-reference").unwrap();
        let snapshot = json!({"request":{"reference_mode":"all", "references":selected}});
        store::write_once(&p, &snapshot).unwrap();
        assert_eq!(store::read(&p).unwrap(), snapshot);
        assert_eq!(
            choose_references(
                pool,
                &json!({"reference_mode":"sample", "reference_count":4}),
                "same"
            )
            .unwrap()
            .len(),
            4
        );
        let large_single = vec![json!({"entry_id":"large", "tags":"x".repeat(2 * 1024 * 1024)})];
        assert!(
            choose_references(large_single, &json!({"reference_mode":"sample"}), "same").is_err()
        );
    }
    #[test]
    fn bad_image_rejected_before_network() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.png");
        std::fs::write(&p, b"not image").unwrap();
        assert!(prepare_image(p.to_str().unwrap()).is_err());
    }
    #[test]
    fn new_creative_input_requires_image_support_but_legacy_reverse_remains_available() {
        for caps in [
            json!({}),
            json!({"reference_images_supported":false}),
            json!({"reference_images_supported":"true"}),
        ] {
            for mode in ["draw"] {
                assert_eq!(
                    require_reference_images(&caps, mode).unwrap_err().code,
                    "studio_reference_images_unsupported"
                );
            }
            require_reference_images(&caps, "reverse").unwrap();
        }
        require_reference_images(&json!({"reference_images_supported":true}), "draw").unwrap();
    }
    #[test]
    fn reference_content_defaults_and_capabilities_never_silently_switch_modes() {
        let image_caps = json!({"reference_images_supported":true});
        let text_caps = json!({"reference_text_supported":true});
        for mode in ["draw"] {
            let mut request = json!({"mode":mode});
            normalize_input(&mut request).unwrap();
            assert_eq!(request["reference_content"], "image_text");
            assert!(needs_reference_images(&request).unwrap());
            require_reference_content(&image_caps, &request).unwrap();
            assert!(require_reference_content(&text_caps, &request).is_err());
            request["reference_content"] = json!("text");
            assert!(!needs_reference_images(&request).unwrap());
            require_reference_content(&text_caps, &request).unwrap();
            for caps in [
                image_caps.clone(),
                json!({}),
                json!({"reference_text_supported":"true"}),
            ] {
                assert_eq!(
                    require_reference_content(&caps, &request).unwrap_err().code,
                    "studio_reference_text_unsupported"
                );
            }
            for images in [json!([]), Value::Null, json!({})] {
                request["reference_images"] = images;
                assert!(require_reference_content(&text_caps, &request).is_err());
            }
        }
        for content in [json!("unknown"), json!(""), json!(false), Value::Null] {
            let mut request = json!({"mode":"draw","reference_content":content});
            assert!(normalize_input(&mut request).is_err());
            assert!(reference_content(&request).is_err());
        }
    }
    #[tokio::test]
    async fn pure_text_prepare_restore_and_retry_never_require_an_image_table_or_files() {
        let temp = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(crate::db::Db::open_memory().unwrap());
        // Any accidental image SQL query fails, even before opening a source file.
        db.with(|c| {
            c.execute_batch("DROP TABLE entry_image")?;
            Ok(())
        })
        .unwrap();
        let pool:Vec<Value> = (0..12).map(|n| json!({"entry_id":format!("synthetic{n}"),
            "tags":format!("  complete main {n}\n"),"negative_prompt":"complete negative",
            "character_prompts":[{"prompt":"complete character","negative_prompt":"exact character negative"}]})).collect();
        for mode in ["draw"] {
            for reference_mode in ["all", "sample"] {
                let id = format!("{mode}-{reference_mode}");
                let mut request = json!({"mode":mode,"reference_content":"text",
                    "reference_mode":reference_mode,"reference_count":4});
                normalize_input(&mut request).unwrap();
                let refs = choose_references(pool.clone(), &request, &id).unwrap();
                assert_eq!(refs.len(), if reference_mode == "all" { 12 } else { 4 });
                let images = freeze_reference_input(
                    db.clone(),
                    temp.path().into(),
                    id.clone(),
                    &request,
                    refs.clone(),
                )
                .await
                .unwrap();
                assert!(images.is_empty());
                request["references"] = json!(refs);
                let saved = json!({"request":request,"reference_image_snapshots":images});
                let path = store::path(temp.path(), "inputs", &id).unwrap();
                store::write_once(&path, &saved).unwrap();
                let restored = store::read(&path).unwrap();
                // Later UI edits cannot replace the mode or text in the immutable snapshot.
                request["reference_content"] = json!("image_text");
                request["references"] = json!([]);
                for _ in 0..2 {
                    let (body, uploads) = frozen_reference_payload(
                        temp.path(),
                        &id,
                        &restored,
                        &json!({"reference_text_supported":true}),
                    )
                    .unwrap();
                    assert_eq!(body["reference_content"], "text");
                    assert_eq!(body["references"], json!(refs));
                    assert_eq!(body["reference_mode"], reference_mode);
                    assert!(body.get("reference_images").is_none() && uploads.is_none());
                }
                assert!(!temp.path().join("studio/reference_images").exists());
                // The same missing-image input remains an error in image+text mode.
                assert!(
                    freeze_reference_input(db.clone(), temp.path().into(), id, &request, refs)
                        .await
                        .is_err()
                );
            }
        }
    }
    #[test]
    fn text_response_requires_explicit_mode_zero_count_and_empty_manifest() {
        for mode in ["draw"] {
            let (mut batch, mut request) = synthetic_plan(mode, 3);
            request["reference_content"] = json!("text");
            batch["settings"]["reference_content"] = json!("text");
            batch["settings"]["reference_image_count"] = json!(0);
            batch["settings"]["reference_images"] = json!([]);
            validate_plan_for(&batch, &request).unwrap();
            for key in [
                "reference_content",
                "reference_image_count",
                "reference_images",
            ] {
                let mut bad = batch.clone();
                bad["settings"].as_object_mut().unwrap().remove(key);
                assert!(validate_plan_for(&bad, &request).is_err(), "{key}");
            }
            for (key, value) in [
                ("reference_content", json!("image_text")),
                ("reference_image_count", json!(1)),
                ("reference_image_count", json!("0")),
                ("reference_images", json!([{"entry_id":"r1"}])),
                ("reference_images", json!({})),
            ] {
                let mut bad = batch.clone();
                bad["settings"][key] = value;
                assert!(validate_plan_for(&bad, &request).is_err());
            }
            request["reference_images"] = json!([]);
            assert!(validate_plan_for(&batch, &request).is_err());
        }
    }
    #[test]
    fn image_content_uses_legacy_wire_only_when_plugin_lacks_new_capability() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("studio/reference_images/input1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let bytes = output.into_inner();
        let sha = store::hash(&bytes);
        std::fs::write(dir.join(format!("{sha}.png")), &bytes).unwrap();
        let mut saved = json!({"request":{"mode":"draw","reference_content":"image_text",
            "references":[{"entry_id":"r1","tags":"exact text"}]},
            "reference_image_snapshots":[{"entry_id":"r1","ordinal":0,"mime":"image/png",
                "sha256":sha,"raw_tag":"exact image tags","bytes":bytes.len()}]});
        for explicit in [true, false] {
            if !explicit {
                saved["request"]
                    .as_object_mut()
                    .unwrap()
                    .remove("reference_content");
            }
            for new_plugin in [false, true] {
                let caps = json!({"reference_images_supported":true,"reference_text_supported":new_plugin});
                let (body, uploads) =
                    frozen_reference_payload(temp.path(), "input1", &saved, &caps).unwrap();
                assert!(body.get("reference_content").is_none());
                assert_eq!(body["reference_images"], json!(uploads.unwrap()));
                assert_eq!(body["reference_images"][0]["sha256"], sha);
            }
        }
        saved["request"]["reference_content"] = json!("image_text");
        saved["reference_content_wire"] = json!(true);
        let path = store::path(temp.path(), "inputs", "input1").unwrap();
        store::write_once(&path, &saved).unwrap();
        let restored = store::read(&path).unwrap();
        let caps = json!({"reference_images_supported":true,"reference_text_supported":true});
        let (body, _) = frozen_reference_payload(temp.path(), "input1", &restored, &caps).unwrap();
        assert_eq!(body["reference_content"], "image_text");
        assert_eq!(
            frozen_reference_payload(
                temp.path(),
                "input1",
                &restored,
                &json!({"reference_images_supported":true})
            )
            .unwrap_err()
            .code,
            "studio_reference_text_unsupported"
        );
        assert_eq!(
            frozen_reference_payload(temp.path(), "input1", &store::read(&path).unwrap(), &caps)
                .unwrap()
                .0,
            body
        );
        assert_eq!(store::read(&path).unwrap(), restored);
        saved
            .as_object_mut()
            .unwrap()
            .remove("reference_image_snapshots");
        assert_eq!(
            frozen_reference_payload(
                temp.path(),
                "input1",
                &saved,
                &json!({"reference_images_supported":true,"reference_text_supported":true})
            )
            .unwrap_err()
            .code,
            "studio_reference_images_required"
        );
    }
    #[test]
    fn reverse_source_image_is_unaffected_by_reference_content() {
        let mut request = json!({"mode":"reverse","reference_content":"text",
            "reverse":{"image_base64":"original fixture bytes","mime":"image/png"}});
        let original = request["reverse"].clone();
        normalize_input(&mut request).unwrap();
        assert!(!needs_reference_images(&request).unwrap());
        require_reference_content(&Value::Null, &request).unwrap();
        let (body, uploads) = frozen_reference_payload(
            Path::new("/nonexistent-unused-root"),
            "reverse-input",
            &json!({"request":request}),
            &Value::Null,
        )
        .unwrap();
        assert!(body.get("reference_content").is_none() && uploads.is_none());
        assert_eq!(body["reverse"], original);
    }
    #[test]
    fn image_manifest_echo_cannot_drop_reorder_or_replace_any_reference() {
        let (mut batch, mut request) = synthetic_plan("draw", 3);
        let images = json!([
            {"entry_id":"r1","ordinal":0,"mime":"image/png","sha256":"1".repeat(64),"raw_tag":"exact tags"},
            {"entry_id":"r2","ordinal":0,"mime":"image/jpeg","sha256":"2".repeat(64),"raw_tag":null},
        ]);
        request["reference_images"] = images.clone();
        batch["settings"]["reference_images"] = images.clone();
        batch["settings"]["reference_image_count"] = json!(2);
        validate_plan_for(&batch, &request).unwrap();
        request["reference_content"] = json!("image_text");
        validate_plan_for(&batch, &request).unwrap(); // old plugin omits only content echo
        batch["settings"]["reference_content"] = json!("image_text");
        validate_plan_for(&batch, &request).unwrap();
        let mut bad = batch.clone();
        bad["settings"]["reference_content"] = json!("text");
        assert!(validate_plan_for(&bad, &request).is_err());
        let mut bad = batch.clone();
        bad["settings"]["reference_image_count"] = json!(1);
        assert!(validate_plan_for(&bad, &request).is_err());
        for key in ["entry_id", "ordinal", "mime", "sha256", "raw_tag"] {
            let mut bad = batch.clone();
            bad["settings"]["reference_images"][0][key] = json!("altered");
            assert!(validate_plan_for(&bad, &request).is_err());
        }
        let mut bad = batch.clone();
        bad["settings"]["reference_images"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert!(validate_plan_for(&bad, &request).is_err());
        bad["settings"]
            .as_object_mut()
            .unwrap()
            .remove("reference_images");
        assert!(validate_plan_for(&bad, &request).is_err());
        // Existing text-only records are still valid history; the new plan
        // entry point refuses to create new plans from such legacy inputs.
        let (old_batch, old_request) = synthetic_plan("draw", 3);
        validate_plan_for(&old_batch, &old_request).unwrap();
    }
    #[test]
    fn image_cache_status_rejects_malformed_or_foreign_hashes() {
        let manifests = vec![json!({"sha256":"first"}), json!({"sha256":"second"})];
        assert_eq!(
            missing_reference_images(&json!({"missing":["second"]}), &manifests).unwrap(),
            HashSet::from(["second".into()])
        );
        assert!(missing_reference_images(&json!({"missing":[]}), &manifests)
            .unwrap()
            .is_empty());
        for invalid in [
            json!({}),
            json!({"missing":{}}),
            json!({"missing":[null]}),
            json!({"missing":["foreign"]}),
            json!({"missing":["first","first"]}),
        ] {
            assert!(missing_reference_images(&invalid, &manifests).is_err());
        }
    }
    #[test]
    fn experimental_all_scope_never_mutates_decisions_or_admits_wrong_ratings() {
        let db = crate::db::Db::open_memory().unwrap();
        db.with(|c| {
            c.execute_batch("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES('nai5_community_pack','string','synthetic','1','[]');")?;
            for (id,rating,decision,removed,tags) in [
                ("included","safe","include",None,"synthetic scenery"),
                ("excluded","safe","exclude",None,"synthetic scenery"),
                ("undecided","safe","undecided",None,"synthetic scenery"),
                ("unrated","unknown","include",None,"synthetic scenery"),
                ("removed","safe","include",Some("2026-01-01"),"synthetic scenery"),
                ("blocked","safe","include",None,"artist:blocked_artist, synthetic scenery"),
            ] {
                c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,'nai5_community_pack',0,?1,'[\"fixture\"]',?2,?3,'1','1',?4)",rusqlite::params![id,tags,rating,removed])?;
                c.execute("INSERT INTO library_membership(entry_id,library,decision,decided_at,note) VALUES(?1,'pack',?2,'now','handmade')",rusqlite::params![id,decision])?;
            }
            // A never-touched entry has no membership row; it is implicitly undecided.
            c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release) VALUES('untouched','nai5_community_pack',0,'untouched','[\"fixture\"]','synthetic scenery','safe','1','1')",[])?;
            c.execute("INSERT INTO artist_blacklist(name,added_at) VALUES('blocked_artist','now')",[])?;
            let snapshot=|c:&rusqlite::Connection|->AppResult<Vec<(String,String,String)>> {
                let mut s=c.prepare("SELECT entry_id,decision,note FROM library_membership ORDER BY entry_id")?;
                let rows=s.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<Result<Vec<_>,_>>()?; Ok(rows)
            };
            let before=snapshot(c)?;
            let mut q=ReferenceQuery{mode:"draw".into(),decisions:"include".into(),scope:vec![],search:None,offset:None,limit:None};
            assert_eq!(load_references(c,&q)?.len(),1);
            q.decisions="all".into(); let rows=load_references(c,&q)?;
            assert_eq!(rows.len(),4); assert!(rows.iter().any(|r|r["decision"]=="exclude"));
            assert!(rows.iter().any(|r|r["entry_id"]=="untouched"&&r["decision"]=="undecided"));
            assert!(rows.iter().all(|r|r["codex_id"]=="nai5_community_pack"&&r["rating"]=="safe"));
            assert!(!rows.iter().any(|r|r["entry_id"]=="unrated"), "非 safe 行即使已加入也不进参考");
            q.mode="reverse".into(); assert!(load_references(c,&q).is_err(), "参考只服务 /画");
            assert_eq!(snapshot(c)?,before); assert_eq!(creative::local_references(c,&[])?.len(),1);
            Ok(())
        }).unwrap();
    }
}
