//! 钥匙串封装：service 固定 `nai5-codex-lab`，account 取 `webui_password` / `library_token` /
//! `provider:<provider_id>`（CONTRACT §1 secret_*）。keyring 3 在 macOS 走 Keychain，Linux 走内核 keyutils。
//! 取不到条目时 `get` 返回 None，其余错误映射为 AppError::secret。

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::sync::Mutex;

pub const SERVICE: &str = "nai5-codex-lab";

/// 进程内缓存（v0.2.2）：macOS 对未签名的开发版二进制每次重编译都视为新程序，钥匙串 ACL 记不住"始终允许"，
/// 每读一次就弹一次授权框。缓存后每个 account 每次启动最多读一次钥匙串；set/delete 同步更新缓存。
static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn cache_get(account: &str) -> Option<Option<String>> {
    CACHE.lock().ok()?.as_ref()?.get(account).cloned()
}

fn cache_put(account: &str, value: Option<String>) {
    if let Ok(mut guard) = CACHE.lock() {
        guard.get_or_insert_with(HashMap::new).insert(account.to_string(), value);
    }
}

const ALLOWED: &[&str] = &["webui_password", "library_token"];

pub fn validate_account(account: &str) -> AppResult<()> {
    if ALLOWED.contains(&account) || account.starts_with("provider:") {
        Ok(())
    } else {
        Err(AppError::bad_request(format!("未知钥匙串 account: {account}")))
    }
}

fn entry(account: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, account)
}

pub fn set(account: &str, value: &str) -> AppResult<()> {
    validate_account(account)?;
    entry(account)?.set_password(value)?;
    cache_put(account, Some(value.to_string()));
    Ok(())
}

/// 取不到条目（含无法打开钥匙串会话的 NoEntry）→ None；其余错误 → secret。命中进程内缓存时不碰钥匙串。
pub fn get(account: &str) -> AppResult<Option<String>> {
    if let Some(cached) = cache_get(account) {
        return Ok(cached);
    }
    let got = match entry(account).and_then(|e| e.get_password()) {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::secret(e.to_string())),
    }?;
    cache_put(account, got.clone());
    Ok(got)
}

pub fn has(account: &str) -> AppResult<bool> {
    Ok(get(account)?.is_some())
}

pub fn delete(account: &str) -> AppResult<()> {
    let r = match entry(account).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::secret(e.to_string())),
    };
    if r.is_ok() {
        cache_put(account, None);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_roundtrip_without_keychain() {
        cache_put("unit:test", Some("v".into()));
        assert_eq!(cache_get("unit:test"), Some(Some("v".into())));
        cache_put("unit:test", None);
        assert_eq!(cache_get("unit:test"), Some(None));
        assert_eq!(cache_get("unit:missing"), None, "未缓存的 account 返回 None 以便真正去读钥匙串");
    }
}

pub fn provider_account(provider_id: i64) -> String {
    format!("provider:{provider_id}")
}
