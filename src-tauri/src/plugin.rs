//! 插件（astrbot_plugin_vivian）HTTP 传输层，导出推送（§3.4 / §8.2）与实验室（§9.4）共用。
//! v0.1.4 时这些辅助函数住在 `export.rs`，v0.2.0 搬到这里，行为不变：
//! - `plugin_conn`：settings 里的地址与用户名 + 钥匙串里的密码（必需）与 `library_token`（可缺，缺则发空头），
//!   钥匙串是阻塞调用（macOS Keychain 可能弹窗），放 `spawn_blocking`。
//! - `login`：明文先试，仅 401 回退旧版 MD5；进程共享 JWT，合并并发登录并对失败冷却。
//! - `plugin_request`：带 `Authorization: Bearer` 与 `X-Library-Token` 的请求，401 自动重登一次，返回 (状态码, 正文)；
//!   连不上 / 超时 → `network`，登录被拒 → `plugin_rejected`，非 2xx 由调用方按各自语义处理。
//! - `plugin_data`：AstrBot 把插件返回包在 `{ status, message, data }` 里时取 `data`。
//! - `PluginSession`（v0.2.3）：一段会话里复用同一个 JWT（反推批次的插件端点每题一次请求，不必每题登录），
//!   首次请求时登录、401 重登一次；`plugin_request` 与它共用 `send_once`。
//! - `plugin_providers`（§10.1）：GET `{PLUGIN_PAGE_PATH}/providers` → AstrBot 里配置好的 provider id 列表。

use std::time::{Duration, Instant};
use std::sync::{Arc, OnceLock};
use std::collections::HashMap;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::secrets;
use crate::state::{load_settings, AppState};

#[cfg(test)]
#[path = "plugin_auth_tests.rs"]
mod auth_tests;

/// 插件页面路由的公共前缀；库路由是 `{PLUGIN_PAGE_PATH}/library/...`，实验室是 `{PLUGIN_PAGE_PATH}/lab/...`。
pub(crate) const PLUGIN_PAGE_PATH: &str = "/api/plug/astrbot_plugin_vivian/page";

/// 老版 AstrBot（≤4.26）的登录页在浏览器里先把密码 md5 再发，服务器拿收到的串直接和存的 md5 比；
/// 4.27 起服务器自己算哈希（md5 或 pbkdf2），客户端必须发明文。`login` 先发明文，401 再退回发 md5(明文)。
pub(crate) fn password_for_login(raw: &str) -> String {
    use md5::{Digest, Md5};
    hex::encode(Md5::digest(raw.trim().as_bytes()))
}

/// 登录顺序（§6 v0.2.1 裁定）：先发明文（AstrBot ≥4.27 服务器端哈希），401 再发 md5(明文)（老版页面预哈希）。
/// 两次都 401 才报错，错误信息取最后一次响应。
async fn login(http: &reqwest::Client, base: &str, username: &str, password: &str) -> AppResult<String> {
    let plain = password.trim().to_string();
    let candidates = [plain.clone(), password_for_login(password)];
    let mut last_err: Option<AppError> = None;
    for (i, candidate) in candidates.iter().enumerate() {
        match login_once(http, base, username, candidate).await {
            Ok(token) => return Ok(token),
            Err(e) if e.code == "plugin_rejected" && e.message.contains("401") && i + 1 < candidates.len() => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::plugin_rejected("登录失败".to_string())))
}

async fn login_once(http: &reqwest::Client, base: &str, username: &str, password_wire: &str) -> AppResult<String> {
    let url = format!("{base}/api/auth/login");
    let resp = http
        .post(&url)
        .json(&json!({ "username": username, "password": password_wire }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::network(format!("登录 {url}: {e}")))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(AppError::plugin_rejected(format!("登录失败 HTTP {status}: {}", truncate(&body.to_string(), 500))));
    }
    body.pointer("/data/token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::plugin_rejected(format!("登录响应没有 token: {}", truncate(&body.to_string(), 500))))
}

/// 响应正文只取前 n 个字符进错误信息（§3.4「前 500 字」）。
pub(crate) fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub(crate) fn join_err(e: tokio::task::JoinError) -> AppError {
    AppError::new("internal", e.to_string())
}

/// 推送 / 状态查询 / 实验室共用的连接信息：settings 里的地址与用户名，钥匙串里的密码（必需）与 library_token（可缺，缺则发空头）。
#[derive(Clone)]
pub(crate) struct PluginConn {
    pub base: String,
    pub username: String,
    pub password: String,
    pub library_token: Option<String>,
}

pub(crate) async fn plugin_conn(state: &AppState) -> AppResult<PluginConn> {
    plugin_conn_with_library(state, true).await
}

pub(crate) async fn plugin_conn_with_library(state: &AppState, library: bool) -> AppResult<PluginConn> {
    let db = state.db.clone();
    let settings = tokio::task::spawn_blocking(move || load_settings(&db)).await.map_err(join_err)??;
    let base = settings.plugin_base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err(AppError::bad_request("settings.plugin_base_url 为空"));
    }
    // 钥匙串是阻塞调用（macOS Keychain 可能弹窗），不在 async 线程上跑（#9）
    let (password, library_token) = tokio::task::spawn_blocking(move || -> AppResult<(String, Option<String>)> {
        let password = secrets::get("webui_password")?.ok_or_else(|| AppError::secret("钥匙串里没有 webui_password"))?;
        Ok((password, if library { secrets::get("library_token")? } else { None }))
    })
    .await
    .map_err(join_err)??;
    Ok(PluginConn { base, username: settings.webui_username, password, library_token })
}

/// 发一次带 `Authorization: Bearer` 与 `X-Library-Token` 的请求（不登录、不重登），`plugin_request` 与 `PluginSession` 共用。
/// `query` 由 reqwest 做 URL 编码，为空时 URL 不带 `?`；连不上 / 超时 → `network`。
async fn send_once(
    http: &reqwest::Client,
    conn: &PluginConn,
    token: &str,
    method: reqwest::Method,
    url: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
    timeout: Duration,
) -> AppResult<(reqwest::StatusCode, String)> {
    let mut req = http
        .request(method, url)
        .bearer_auth(token)
        .timeout(timeout);
    if let Some(library_token) = &conn.library_token {
        req = req.header("X-Library-Token", library_token);
    }
    if !query.is_empty() {
        req = req.query(query);
    }
    if let Some(b) = body {
        req = req.header(reqwest::header::CONTENT_TYPE, "application/json").json(b);
    }
    let resp = req.send().await.map_err(|e| AppError::network(format!("请求 {url}: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    Ok((status, text))
}

/// 带 JWT 与 `X-Library-Token` 的插件请求（§3.4）：先登录，401 自动重登一次；返回 (HTTP 状态, 响应正文)。
/// `query` 由 reqwest 做 URL 编码（GET 路由的 `run_id=` 等），为空时 URL 不带 `?`。
/// 连不上 / 超时 → `network`；登录被拒 → `plugin_rejected`。响应非 2xx 由调用方按各自语义处理。
pub(crate) async fn plugin_request(
    http: &reqwest::Client,
    conn: &PluginConn,
    method: reqwest::Method,
    path: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
    timeout: Duration,
) -> AppResult<(reqwest::StatusCode, String)> {
    PluginSession::new(conn.clone()).request(http, method, path, query, body, timeout).await
}

#[derive(Default)]
struct AuthState {
    token: Option<String>,
    failure: Option<(Instant, AppError)>,
}
type AuthSlot = Arc<tokio::sync::Mutex<AuthState>>;

fn shared_auth(conn: &PluginConn) -> AuthSlot {
    use sha2::{Digest, Sha256};
    // Length-delimited identity, never retain/log a plaintext password in the map key.
    let key = hex::encode(Sha256::digest(serde_json::to_vec(&(&conn.base, &conn.username, &conn.password)).unwrap()));
    static CACHE: OnceLock<parking_lot::Mutex<HashMap<String, AuthSlot>>> = OnceLock::new();
    let mut slots = CACHE.get_or_init(Default::default).lock();
    if let Some(slot) = slots.get(&key) { return slot.clone(); }
    if slots.len() >= 16 { slots.clear(); }
    slots.entry(key).or_default().clone()
}

/// 一段会话（一个反推批次）里复用同一个 JWT（v0.2.3，§10.1）：首次 `request` 时登录，之后都带同一 token；
/// 收到 401 重登一次再重发。多个端点并行调用时共享同一份 token：谁先发现过期谁去重登，其它人拿到的已是新 token
/// （`refresh` 里比对「我用的那个」与「当前存的」，不同就直接用新的，不会并发登录多次）。
pub struct PluginSession {
    conn: PluginConn,
    token: AuthSlot,
}

impl PluginSession {
    pub(crate) fn new(conn: PluginConn) -> Self {
        let token = shared_auth(&conn);
        PluginSession { conn, token }
    }

    /// 插件地址（去尾斜杠），`request_json` 里 `base_url` 用。
    pub fn base(&self) -> &str {
        &self.conn.base
    }

    async fn token(&self, http: &reqwest::Client) -> AppResult<String> {
        self.acquire_token(http, None).await
    }

    /// `stale` 是本次请求用过、被 401 的 token；别人已经换过就直接用新的，否则自己登录一次。
    async fn refresh(&self, http: &reqwest::Client, stale: &str) -> AppResult<String> {
        self.acquire_token(http, Some(stale)).await
    }

    async fn acquire_token(&self, http: &reqwest::Client, stale: Option<&str>) -> AppResult<String> {
        let mut guard = self.token.lock().await;
        if let Some(t) = guard.token.as_ref().filter(|t| Some(t.as_str()) != stale) {
            return Ok(t.clone());
        }
        guard.token = None;
        if let Some((until, error)) = &guard.failure {
            if *until > Instant::now() { return Err(error.clone()); }
        }
        match login(http, &self.conn.base, &self.conn.username, &self.conn.password).await {
            Ok(t) => { guard.token = Some(t.clone()); guard.failure = None; Ok(t) }
            Err(e) => {
                let seconds = if e.code == "network" { 5 } else { 60 };
                let error = AppError::new(&e.code, format!("{}；登录已暂停 {seconds} 秒后可重试", e.message));
                guard.failure = Some((Instant::now() + Duration::from_secs(seconds), error.clone()));
                Err(error)
            }
        }
    }

    /// 语义同 `plugin_request`（JWT + `X-Library-Token`，401 重登一次，返回 (状态, 正文)），只是 token 跨调用复用。
    pub async fn request(
        &self,
        http: &reqwest::Client,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        timeout: Duration,
    ) -> AppResult<(reqwest::StatusCode, String)> {
        let url = format!("{}{}", self.conn.base, path);
        let token = self.token(http).await?;
        let (status, text) = send_once(http, &self.conn, &token, method.clone(), &url, query, body, timeout).await?;
        if status.as_u16() != 401 {
            return Ok((status, text));
        }
        let token = self.refresh(http, &token).await?;
        let response = send_once(http, &self.conn, &token, method, &url, query, body, timeout).await?;
        if response.0.as_u16() == 401 {
            let mut guard = self.token.lock().await;
            if guard.token.as_deref() == Some(&token) {
                guard.token = None;
                guard.failure = Some((Instant::now() + Duration::from_secs(60), AppError::plugin_rejected("重新登录后仍为 HTTP 401；暂停登录 60 秒，请检查 WebUI 用户名、密码或接口权限")));
            }
        }
        Ok(response)
    }
}

/// §10.1 `plugin_providers`：GET `{PLUGIN_PAGE_PATH}/providers`（JWT；路由不核对 token，但照发 `X-Library-Token` 无妨）
/// → AstrBot 里配置好的 provider id 列表。连不上 / 超时 → `network`；地址 / 密码未配置 → `bad_request` / `secret`；
/// 非 2xx → `plugin_rejected`（含 404 = 插件还没升到带该路由的版本）。
pub(crate) async fn plugin_providers(state: &AppState) -> AppResult<Vec<String>> {
    let conn = plugin_conn(state).await?;
    let path = format!("{PLUGIN_PAGE_PATH}/providers");
    let (status, text) = plugin_request(&state.http, &conn, reqwest::Method::GET, &path, &[], None, Duration::from_secs(30)).await?;
    if !status.is_success() {
        return Err(rejected(status, &text));
    }
    let parsed: Value = serde_json::from_str(&text).map_err(|e| AppError::plugin_rejected(format!("响应不是 JSON: {e}: {}", truncate(&text, 200))))?;
    if let Some(e) = plugin_error(&parsed) {
        return Err(e);
    }
    parse_provider_ids(&parsed).ok_or_else(|| AppError::plugin_rejected(format!("插件返回的 providers 形状不对: {}", truncate(&text, 200))))
}

/// `providers` 响应 → id 列表。契约形状是 `data` 为字符串数组；也容忍纯数组、`{ providers: [...] }`、
/// 以及元素为 `{ id | provider_id, ... }` 对象。空串 id 丢掉；元素既不是字符串也不是带 id 的对象 → None。
pub(crate) fn parse_provider_ids(parsed: &Value) -> Option<Vec<String>> {
    let payload = plugin_payload(parsed);
    let arr = match payload {
        Value::Array(a) => a,
        Value::Object(o) => o.get("providers")?.as_array()?,
        _ => return None,
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let id = match item {
            Value::String(s) => s.trim().to_string(),
            Value::Object(o) => o.get("id").or_else(|| o.get("provider_id"))?.as_str()?.trim().to_string(),
            _ => return None,
        };
        if !id.is_empty() {
            out.push(id);
        }
    }
    Some(out)
}

/// AstrBot 路由可能把插件返回包在 `{ status, message, data }` 里，也可能原样返回：取 `data` 对象，否则整个响应。
pub(crate) fn plugin_data(parsed: &Value) -> &Value {
    match parsed.get("data") {
        Some(d) if d.is_object() => d,
        _ => parsed,
    }
}

/// 同 `plugin_data`，但 `data` 是数组时也拆包（实验室 `run/list` 返回 `LabRunSummary[]`）。
pub(crate) fn plugin_payload(parsed: &Value) -> &Value {
    match parsed.get("data") {
        Some(d) if d.is_object() || d.is_array() => d,
        _ => parsed,
    }
}

/// 插件（或 AstrBot 路由层）以 2xx 返回 `{ status: "error", message }` 时，把 message 直接暴露出来，
/// 而不是让上层报"响应里没有 xxx 字段"。「未找到该路由」是 AstrBot 分发器在插件没重载时的回答，附上提示。
pub(crate) fn plugin_error(parsed: &Value) -> Option<AppError> {
    let status = parsed.get("status").and_then(Value::as_str)?;
    if status != "error" {
        return None;
    }
    let msg = parsed.get("message").and_then(Value::as_str).unwrap_or("").trim();
    let msg = if msg.is_empty() { "未知错误".to_string() } else { msg.to_string() };
    let hint = if msg.contains("未找到该路由") { "（插件未重载或版本过旧：请在 WebUI 里重载薇拉插件）" } else { "" };
    Some(AppError::plugin_rejected(format!("插件返回错误：{msg}{hint}")))
}

/// 非 2xx 统一成 `plugin_rejected`，message 带 `HTTP <code>: <正文前 500 字>`（§3.4 第 3 条）。
pub(crate) fn rejected(status: reqwest::StatusCode, text: &str) -> AppError {
    AppError::plugin_rejected(format!("HTTP {status}: {}", truncate(text, 500)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #10：md5 候选永远是 md5(明文)，32 位 hex 也不例外（明文候选由 `login` 先试）。
    #[test]
    fn login_password_is_always_md5() {
        assert_eq!(password_for_login("astrbot"), "77b90590a8945a7d36c963981a307dc9");
        assert_eq!(password_for_login("  astrbot \n"), "77b90590a8945a7d36c963981a307dc9", "首尾空白照旧去掉");
        let looks_hashed = "77B90590A8945A7D36C963981A307DC9";
        assert_eq!(password_for_login(looks_hashed), "a9b37c6abdbe365d20e24447fccc56a5");
        assert_ne!(password_for_login(looks_hashed), looks_hashed.to_lowercase(), "不再把 32 位 hex 当已哈希原样发");
    }

    #[test]
    fn payload_unwraps_astrbot_envelope_for_objects_and_arrays() {
        let wrapped_obj = json!({ "status": "ok", "data": { "ok": true } });
        assert_eq!(plugin_data(&wrapped_obj), &json!({ "ok": true }));
        assert_eq!(plugin_payload(&wrapped_obj), &json!({ "ok": true }));
        let wrapped_arr = json!({ "status": "ok", "data": [1, 2] });
        assert_eq!(plugin_data(&wrapped_arr), &wrapped_arr, "plugin_data 只拆对象（v0.1.4 行为不变）");
        assert_eq!(plugin_payload(&wrapped_arr), &json!([1, 2]));
        let bare = json!([{ "run_id": "r1" }]);
        assert_eq!(plugin_payload(&bare), &bare);
        assert_eq!(truncate("héllo", 2), "hé");
    }

    /// §10.1 `providers`：契约形状（`data` 为字符串数组）之外也容忍纯数组、`{ providers }`、带 id 的对象；坏形状 → None。
    #[test]
    fn provider_ids_parse_contract_shape_and_tolerated_variants() {
        assert_eq!(parse_provider_ids(&json!({ "status": "ok", "message": "", "data": ["gpt-4o", " claude ", ""] })).unwrap(), ["gpt-4o", "claude"]);
        assert_eq!(parse_provider_ids(&json!(["a", "b"])).unwrap(), ["a", "b"]);
        assert_eq!(parse_provider_ids(&json!({ "status": "ok", "data": { "providers": ["a"] } })).unwrap(), ["a"]);
        assert_eq!(parse_provider_ids(&json!({ "providers": [{ "id": "x", "type": "openai" }, { "provider_id": "y" }] })).unwrap(), ["x", "y"]);
        assert_eq!(parse_provider_ids(&json!({ "status": "ok", "data": [] })).unwrap(), Vec::<String>::new());
        assert!(parse_provider_ids(&json!({ "status": "error", "message": "no", "data": null })).is_none());
        assert!(parse_provider_ids(&json!([1, 2])).is_none());
        assert!(parse_provider_ids(&json!([{ "name": "no id" }])).is_none());
        assert!(parse_provider_ids(&json!("gpt")).is_none());
    }
}
