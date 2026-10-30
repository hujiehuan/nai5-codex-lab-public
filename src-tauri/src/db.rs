//! SQLite 封装：打开连接（PRAGMA foreign_keys/WAL/synchronous=NORMAL，CONTRACT §0）、
//! 按 `schema_version` 应用 migrations/ 下的 DDL、`app_state` KV、`app_event` 写入、
//! `VACUUM INTO` 备份（设计 §6：先写 .tmp 再 rename，按份数保留）。
//! 连接放在 `parking_lot::Mutex` 里，调用方用 `with()` 短时持锁，不在锁内做网络 IO。

use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 迁移清单：新增列时加 `00N_*.sql`，不改已发布的版本。
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/001_init.sql")),
    (2, include_str!("../migrations/002_entry_image_sha256.sql")),
    (3, include_str!("../migrations/003_lab_run.sql")),
    (4, include_str!("../migrations/004_reverse_provider_kind.sql")),
    (5, include_str!("../migrations/005_reverse_run_render.sql")),
    (6, include_str!("../migrations/006_image_embedding.sql")),
    (7, include_str!("../migrations/007_reverse_run_style.sql")),
    (8, include_str!("../migrations/008_nax_artist_library.sql")),
];
/// 当前 schema 版本（= MIGRATIONS 最后一项）。
pub const SCHEMA_VERSION: i64 = 8;

#[cfg(test)]
#[path = "migration_integrity_tests.rs"]
mod migration_integrity_tests;

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub struct Db {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Db> {
        let conn = Connection::open(path)?;
        Self::init_conn(&conn)?;
        Ok(Db { conn: Mutex::new(conn), path: path.to_path_buf() })
    }

    /// 测试用内存库（WAL 对内存库无效，其余 PRAGMA 照常）。
    pub fn open_memory() -> AppResult<Db> {
        let conn = Connection::open_in_memory()?;
        Self::init_conn(&conn)?;
        Ok(Db { conn: Mutex::new(conn), path: PathBuf::from(":memory:") })
    }

    fn init_conn(conn: &Connection) -> AppResult<()> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
        )?;
        migrate(conn)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 短时持锁执行一段同步 SQL。
    pub fn with<R>(&self, f: impl FnOnce(&mut Connection) -> AppResult<R>) -> AppResult<R> {
        let mut guard = self.conn.lock();
        f(&mut guard)
    }

    pub fn app_state_get(&self, key: &str) -> AppResult<Option<String>> {
        self.with(|c| app_state_get(c, key))
    }

    pub fn app_state_set(&self, key: &str, value: &str) -> AppResult<()> {
        self.with(|c| app_state_set(c, key, value))
    }

    /// 写 `app_event` 并同步打一条 tracing 日志（CONTRACT §0 日志要求）。
    pub fn log_event(&self, level: &str, area: &str, message: &str, data: Value) {
        match level {
            "error" => tracing::error!(area, data = %data, "{message}"),
            "warn" => tracing::warn!(area, data = %data, "{message}"),
            _ => tracing::info!(area, data = %data, "{message}"),
        }
        let r = self.with(|c| {
            c.execute(
                "INSERT INTO app_event(at, level, area, message, data_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![now_iso(), level, area, message, data.to_string()],
            )?;
            Ok(())
        });
        if let Err(e) = r {
            tracing::error!("app_event 写入失败: {e}");
        }
    }

    /// `VACUUM INTO backups/library-YYYYMMDD-HHMMSS[-suffix].db`，保留最近 `keep` 份同后缀的备份。
    pub fn backup_into(&self, backups_dir: &Path, suffix: Option<&str>, keep: usize) -> AppResult<PathBuf> {
        std::fs::create_dir_all(backups_dir)?;
        let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let name = match suffix {
            Some(s) => format!("library-{stamp}-{s}.db"),
            None => format!("library-{stamp}.db"),
        };
        let final_path = backups_dir.join(&name);
        let tmp_path = backups_dir.join(format!("{name}.tmp"));
        if tmp_path.exists() {
            std::fs::remove_file(&tmp_path)?;
        }
        self.with(|c| {
            let tmp = tmp_path.to_string_lossy().to_string();
            c.execute("VACUUM INTO ?1", params![tmp])?;
            Ok(())
        })?;
        std::fs::rename(&tmp_path, &final_path)?;
        prune_backups(backups_dir, suffix, keep)?;
        Ok(final_path)
    }
}

/// 只保留最近 `keep` 份：有后缀的只看带该后缀的文件，无后缀的只看 `library-<stamp>.db`。
fn prune_backups(dir: &Path, suffix: Option<&str>, keep: usize) -> AppResult<()> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .filter(|n| n.starts_with("library-") && n.ends_with(".db"))
        .filter(|n| {
            let body = &n["library-".len()..n.len() - ".db".len()];
            match suffix {
                Some(s) => body.ends_with(&format!("-{s}")),
                None => body.len() == 15 && !body[15..].contains('-'),
            }
        })
        .collect();
    names.sort();
    if names.len() > keep {
        for n in &names[..names.len() - keep] {
            let _ = std::fs::remove_file(dir.join(n));
        }
    }
    Ok(())
}

/// 最近一份每日备份的日期（`library-YYYYMMDD-HHMMSS.db`），用于判断今天是否已备份。
pub fn latest_daily_backup_stamp(dir: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .filter(|n| n.starts_with("library-") && n.ends_with(".db") && n.len() == "library-YYYYMMDD-HHMMSS.db".len())
        .collect();
    names.sort();
    names.pop().map(|n| n["library-".len().."library-".len() + 8].to_string())
}

pub fn migrate(conn: &Connection) -> AppResult<()> {
    apply_migrations(conn, MIGRATIONS)
}

/// 每个版本在一个事务里执行 DDL 并写版本号；任一语句失败整版回滚，版本号不动（v0.1.1 #6）。
fn apply_migrations(conn: &Connection, migrations: &[(i64, &str)]) -> AppResult<()> {
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;
    let mut current: i64 = if has_table {
        conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0))?
    } else {
        0
    };
    for (version, sql) in migrations {
        if *version > current {
            // 出错时 `tx` 被 drop → ROLLBACK；DDL 在 SQLite 里也是事务性的。
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            if *version > 1 {
                tx.execute("UPDATE schema_version SET version = ?1", params![version])?;
            }
            tx.commit()?;
            current = *version;
            tracing::info!(version, "schema migrated");
        }
    }
    Ok(())
}

pub fn app_state_get(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM app_state WHERE key = ?1", params![key], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten())
}

pub fn app_state_set(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO app_state(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// JSON 文本列 → Vec<String>（NULL / 解析失败 → 空）。
pub fn json_str_vec(s: Option<String>) -> Vec<String> {
    s.and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default()
}

pub fn json_value(s: Option<String>) -> Value {
    s.and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null)
}

pub fn map_db<T>(r: rusqlite::Result<T>) -> AppResult<T> {
    r.map_err(|e| AppError::db(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_applies_all_versions_once() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            let v: i64 = c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
            assert_eq!(v, SCHEMA_VERSION);
            migrate(c)?; // 幂等
            let n: i64 = c.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            assert_eq!(fk, 1);
            // 002：entry_image.sha256
            let has_sha: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('entry_image') WHERE name = 'sha256'", [], |r| r.get(0))?;
            assert_eq!(has_sha, 1);
            // 003：lab_run 五列
            let lab_cols: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('lab_run') WHERE name IN ('run_id','mode','created_at','status','json')", [], |r| r.get(0))?;
            assert_eq!(lab_cols, 5, "003_lab_run.sql 应建出 lab_run(run_id, mode, created_at, status, json)");
            // 004：reverse_provider.kind / plugin_provider_id
            let kind_cols: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('reverse_provider') WHERE name IN ('kind','plugin_provider_id')", [], |r| r.get(0))?;
            assert_eq!(kind_cols, 2, "004_reverse_provider_kind.sql 应加出 reverse_provider(kind, plugin_provider_id)");
            let kind_default: String = c.query_row("SELECT dflt_value FROM pragma_table_info('reverse_provider') WHERE name = 'kind'", [], |r| r.get(0))?;
            assert_eq!(kind_default, "'openai'");
            // 005：reverse_run 的 truth_kind / render_* / 泄漏两列
            let render_cols: i64 = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('reverse_run') WHERE name IN ('truth_kind','render_file','render_local','render_seed','render_prompt','quality_score','quality_notes','similarity_score','similarity_reason','render_error','leak_appearance','leak_banned')",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(render_cols, 12, "005_reverse_run_render.sql 应加出 12 列");
            let tk_default: String = c.query_row("SELECT dflt_value FROM pragma_table_info('reverse_run') WHERE name = 'truth_kind'", [], |r| r.get(0))?;
            assert_eq!(tk_default, "'tags'");
            // 006：image_embedding 七列 + reverse_run.render_artist
            let emb_cols: i64 = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('image_embedding') WHERE name IN ('entry_id','ordinal','model','dim','vec','image_sha256','computed_at')",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(emb_cols, 7, "006_image_embedding.sql 应建出 image_embedding 七列");
            let pk_cols: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('image_embedding') WHERE pk > 0", [], |r| r.get(0))?;
            assert_eq!(pk_cols, 3, "主键 (entry_id, ordinal, model)");
            let has_artist: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('reverse_run') WHERE name = 'render_artist'", [], |r| r.get(0))?;
            assert_eq!(has_artist, 1, "006 应给 reverse_run 加 render_artist");
            // 006 §12.6：reverse_run 再加六列（都可空，无默认值）
            let align_cols: i64 = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('reverse_run') WHERE name IN ('render_artist_source','render_size','render_size_source','plugin_dropped','plugin_would_reject','render_candidates') AND \"notnull\" = 0 AND dflt_value IS NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(align_cols, 6, "006 应给 reverse_run 加 §12.6 的六列（可空、无默认）");
            // 007 §12.7：reverse_run 再加五列（都可空，无默认值；score 是 REAL）
            let style_cols: i64 = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('reverse_run') WHERE name IN ('render_style_entry_id','render_style_title','render_style_score','render_style_decision','render_style_fallback') AND \"notnull\" = 0 AND dflt_value IS NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(style_cols, 5, "007_reverse_run_style.sql 应给 reverse_run 加 §12.7 的五列（可空、无默认）");
            let score_type: String = c.query_row("SELECT type FROM pragma_table_info('reverse_run') WHERE name = 'render_style_score'", [], |r| r.get(0))?;
            assert_eq!(score_type, "REAL");
            Ok(())
        })
        .unwrap();
        db.app_state_set("k", "v").unwrap();
        assert_eq!(db.app_state_get("k").unwrap().as_deref(), Some("v"));
        assert_eq!(db.app_state_get("missing").unwrap(), None);
    }

    /// v7 升级仅扩展画师库视图，NAX、原词典和本地画师各自保留来源与用户决策。
    #[test]
    fn migration_008_adds_nax_to_artist_view_and_preserves_decisions() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        apply_migrations(&conn, &MIGRATIONS[..7]).unwrap();
        for (book, entry, removed) in [
            ("artist_nai5_personal", "synthetic_original", None),
            ("nax_v5_artist", "synthetic_nax", None),
            ("local_artist", "synthetic_local", None),
            ("nai5_community_pack", "synthetic_pack", None),
            ("nax_v5_artist", "synthetic_removed", Some("2026-01-01")),
        ] {
            conn.execute("INSERT OR IGNORE INTO codex_book(codex_id,type,title,version,tree_json) VALUES(?1,'string','SYNTHETIC ONLY','fixture','[]')", [book]).unwrap();
            conn.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,?2,0,'SYNTHETIC ONLY','[]','artist:synthetic_fixture','safe','fixture','fixture',?3)", params![entry, book, removed]).unwrap();
            conn.execute("INSERT INTO library_membership(entry_id,library,decision,note,decided_at) VALUES(?1,?2,'include','保留备注🧪','2026-01-01T00:00:00Z')", params![entry, if book == "nai5_community_pack" { "pack" } else { "artist" }]).unwrap();
            conn.execute("INSERT INTO membership_audit(entry_id,from_decision,to_decision,at,via) VALUES(?1,NULL,'include','2026-01-01T00:00:00Z','fixture')", [entry]).unwrap();
        }
        let memberships = || {
            conn.prepare("SELECT entry_id || '|' || library || '|' || decision || '|' || note || '|' || decided_at FROM library_membership ORDER BY entry_id").unwrap()
                .query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
        };
        let before = memberships();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM artist_library", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        migrate(&conn).unwrap();
        let visible = conn.prepare("SELECT entry_id FROM artist_library ORDER BY entry_id").unwrap().query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(visible, ["synthetic_local", "synthetic_nax", "synthetic_original"]);
        assert_eq!(memberships(), before);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM membership_audit", [], |r| r.get::<_, i64>(0)).unwrap(), 5);
        assert!(!conn.prepare("PRAGMA foreign_key_check").unwrap().exists([]).unwrap());
        migrate(&conn).unwrap();
        assert_eq!(memberships(), before, "重复迁移也保留所有三态与备注");
    }

    /// 004（§10.1）：v3 库升级后老的端点行自动成 kind='openai'、plugin_provider_id 为 NULL；新行可以写 plugin。
    #[test]
    fn migration_004_upgrades_v3_rows_to_openai() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn, &MIGRATIONS[..3]).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 3, "先停在 v3");
        let has_kind: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('reverse_provider') WHERE name = 'kind'", [], |r| r.get(0)).unwrap();
        assert_eq!(has_kind, 0);
        conn.execute(
            "INSERT INTO reverse_provider(name, base_url, model, max_tokens, enabled, created_at) VALUES ('old', 'https://x/v1', 'm', 1100, 1, '2026-09-06T00:00:00Z')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let (kind, pid): (String, Option<String>) = conn
            .query_row("SELECT kind, plugin_provider_id FROM reverse_provider WHERE name = 'old'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((kind.as_str(), pid), ("openai", None), "老行默认 openai");
        conn.execute(
            "INSERT INTO reverse_provider(name, base_url, model, max_tokens, enabled, created_at, kind, plugin_provider_id) VALUES ('p', '', '', 1100, 1, '2026-09-07T00:00:00Z', 'plugin', 'gpt')",
            [],
        )
        .unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM reverse_provider WHERE kind = 'plugin' AND plugin_provider_id = 'gpt'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        migrate(&conn).unwrap(); // 幂等：不会再 ALTER 一次
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// 005（§11.4）：v4 库升级后老 run 行 truth_kind 默认 'tags'、本地图片题改成 'none'、有指标的老行把泄漏数抄进 leak_* 两列，
    /// render_* 全空；新列可写；再 migrate 幂等。
    #[test]
    fn migration_005_upgrades_v4_runs() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn, &MIGRATIONS[..4]).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 4, "先停在 v4");
        let has_tk: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('reverse_run') WHERE name = 'truth_kind'", [], |r| r.get(0)).unwrap();
        assert_eq!(has_tk, 0);
        conn.execute_batch(
            "INSERT INTO reverse_batch(batch_id, created_at, template_id, ruleset_id, provider_ids_json, question_count, status) VALUES ('b-1', '2026-09-06T00:00:00Z', 1, 1, '[1]', 2, 'done');
             INSERT INTO reverse_run(run_id, batch_id, created_at, source_kind, source_ref, image_ordinal, image_sha256, template_id, ruleset_id, provider_id, request_json, status)
               VALUES (1, 'b-1', '2026-09-06T00:00:00Z', 'pack_image', 'e1', 0, 'x', 1, 1, 1, '{}', 'ok');
             INSERT INTO reverse_run(run_id, batch_id, created_at, source_kind, source_ref, image_ordinal, image_sha256, template_id, ruleset_id, provider_id, request_json, status)
               VALUES (2, 'b-1', '2026-09-06T00:00:00Z', 'local_file', '/tmp/a.png', NULL, 'y', 1, 1, 1, '{}', 'ok');
             INSERT INTO reverse_metric(run_id, truth_json, truth_eval_json, expected_miss_json, hit, missed, extra, expected_hit, precision, recall, f1, appearance_leak, banned_leak, ruleset_drop, contradiction)
               VALUES (1, '[]', '[]', '[]', 1, 0, 0, 0, 1.0, 1.0, 1.0, 2, 1, 0, 0);",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let (tk1, la1, lb1, rf1): (String, i64, i64, Option<String>) = conn
            .query_row("SELECT truth_kind, leak_appearance, leak_banned, render_file FROM reverse_run WHERE run_id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap();
        assert_eq!((tk1.as_str(), la1, lb1, rf1), ("tags", 2, 1, None), "老的图包题：tags，泄漏数从 reverse_metric 抄过来，render 列为空");
        let (tk2, la2, lb2): (String, i64, i64) = conn
            .query_row("SELECT truth_kind, leak_appearance, leak_banned FROM reverse_run WHERE run_id = 2", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((tk2.as_str(), la2, lb2), ("none", 0, 0), "老的本地图片题：none");
        conn.execute(
            "UPDATE reverse_run SET truth_kind = 'prose', render_file = 'lab_rerender_1.png', render_local = '/x/labruns/reverse/1.jpg', render_seed = 42, render_prompt = 'p',
               quality_score = 7.5, quality_notes = '[\"ok\"]', similarity_score = 8, similarity_reason = '构图一致', render_error = NULL WHERE run_id = 1",
            [],
        )
        .unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM reverse_run WHERE truth_kind = 'prose' AND render_seed = 42 AND similarity_score = 8", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        migrate(&conn).unwrap(); // 幂等：不会再 ALTER 一次
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// 006（§12.1 / §12.5）：v5 库升级后老 run 行 render_artist 为 NULL；image_embedding 可写可读、主键去重；再 migrate 幂等。
    #[test]
    fn migration_006_upgrades_v5_with_embeddings() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn, &MIGRATIONS[..5]).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 5, "先停在 v5");
        let has_tbl: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'image_embedding'", [], |r| r.get(0)).unwrap();
        assert_eq!(has_tbl, 0);
        conn.execute_batch(
            "INSERT INTO reverse_batch(batch_id, created_at, template_id, ruleset_id, provider_ids_json, question_count, status) VALUES ('b-1', '2026-09-07T00:00:00Z', 1, 1, '[1]', 1, 'done');
             INSERT INTO reverse_run(run_id, batch_id, created_at, source_kind, source_ref, image_ordinal, image_sha256, template_id, ruleset_id, provider_id, request_json, status)
               VALUES (1, 'b-1', '2026-09-07T00:00:00Z', 'pack_image', 'e1', 0, 'x', 1, 1, 1, '{}', 'ok');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let artist: Option<String> = conn.query_row("SELECT render_artist FROM reverse_run WHERE run_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(artist, None, "老行 render_artist 为空");
        let (src, size, reject, cands): (Option<String>, Option<String>, Option<i64>, Option<String>) = conn
            .query_row("SELECT render_artist_source, render_size, plugin_would_reject, render_candidates FROM reverse_run WHERE run_id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap();
        assert_eq!((src, size, reject, cands), (None, None, None, None), "老行 §12.6 六列为 NULL（读时 plugin_would_reject 当 0）");
        conn.execute("UPDATE reverse_run SET plugin_would_reject = 1, render_candidates = '[{\"file\":\"a.png\"}]' WHERE run_id = 1", []).unwrap();
        conn.execute("UPDATE reverse_run SET render_artist = 'artist:foo' WHERE run_id = 1", []).unwrap();
        let blob: Vec<u8> = vec![0, 0, 128, 63, 0, 0, 0, 64]; // [1.0, 2.0] f32 LE
        conn.execute(
            "INSERT INTO image_embedding(entry_id, ordinal, model, dim, vec, image_sha256, computed_at) VALUES ('e1', 0, 'm', 2, ?1, 'sha', '2026-09-07T00:00:00Z')",
            params![blob],
        )
        .unwrap();
        // 同主键再插报唯一冲突（UPSERT 由 style::store 负责）
        assert!(conn
            .execute(
                "INSERT INTO image_embedding(entry_id, ordinal, model, dim, vec, image_sha256, computed_at) VALUES ('e1', 0, 'm', 2, X'00', NULL, 't')",
                [],
            )
            .is_err());
        let (dim, back): (i64, Vec<u8>) = conn.query_row("SELECT dim, vec FROM image_embedding WHERE entry_id = 'e1' AND ordinal = 0 AND model = 'm'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((dim, back), (2, vec![0, 0, 128, 63, 0, 0, 0, 64]));
        migrate(&conn).unwrap(); // 幂等：不会再 ALTER 一次
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// 007（§12.7）：v6 库升级后老 run 行五列为 NULL（读时 render_style_fallback 当 0）；新列可写；再 migrate 幂等。
    #[test]
    fn migration_007_upgrades_v6_runs() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn, &MIGRATIONS[..6]).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 6, "先停在 v6");
        let has_col: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('reverse_run') WHERE name LIKE 'render_style_%'", [], |r| r.get(0)).unwrap();
        assert_eq!(has_col, 0);
        conn.execute_batch(
            "INSERT INTO reverse_batch(batch_id, created_at, template_id, ruleset_id, provider_ids_json, question_count, status) VALUES ('b-1', '2026-09-07T00:00:00Z', 1, 1, '[1]', 1, 'done');
             INSERT INTO reverse_run(run_id, batch_id, created_at, source_kind, source_ref, image_ordinal, image_sha256, template_id, ruleset_id, provider_id, request_json, status, render_artist, render_artist_source)
               VALUES (1, 'b-1', '2026-09-07T00:00:00Z', 'pack_image', 'e1', 0, 'x', 1, 1, 1, '{}', 'ok', 'artist:foo', 'given');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let (eid, title, score, decision, fallback): (Option<String>, Option<String>, Option<f64>, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT render_style_entry_id, render_style_title, render_style_score, render_style_decision, render_style_fallback FROM reverse_run WHERE run_id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!((eid, title, score, decision, fallback), (None, None, None, None, None), "老行 §12.7 五列为 NULL");
        let (artist, src): (Option<String>, Option<String>) = conn.query_row("SELECT render_artist, render_artist_source FROM reverse_run WHERE run_id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((artist.as_deref(), src.as_deref()), (Some("artist:foo"), Some("given")), "老列不动");
        conn.execute(
            "UPDATE reverse_run SET render_style_entry_id = 'e9', render_style_title = 'T', render_style_score = 0.83, render_style_decision = 'include', render_style_fallback = 0, render_artist_source = 'style' WHERE run_id = 1",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM reverse_run WHERE render_style_entry_id = 'e9' AND render_style_score > 0.8 AND render_style_fallback = 0 AND render_artist_source = 'style'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        migrate(&conn).unwrap(); // 幂等：不会再 ALTER 一次
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn failed_migration_rolls_back_whole_version() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            let next = SCHEMA_VERSION + 1;
            let bad = [(next, "CREATE TABLE half_done(x INTEGER); INSERT INTO no_such_table VALUES (1);")];
            assert!(apply_migrations(c, &bad).is_err());
            let v: i64 = c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
            assert_eq!(v, SCHEMA_VERSION, "失败的迁移不应推进版本号");
            let n: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'half_done'", [], |r| r.get(0))?;
            assert_eq!(n, 0, "失败的迁移不应留下半张表");
            // 连接仍可用，且好的迁移能正常推进
            let good = [(next, "CREATE TABLE fully_done(x INTEGER);")];
            apply_migrations(c, &good)?;
            let v: i64 = c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
            assert_eq!(v, next);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn backup_writes_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("library.db")).unwrap();
        let p1 = db.backup_into(&dir.path().join("backups"), Some("presync"), 1).unwrap();
        assert!(p1.exists());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let p2 = db.backup_into(&dir.path().join("backups"), Some("presync"), 1).unwrap();
        assert!(p2.exists());
        assert!(!p1.exists(), "older presync backup should be pruned");
        let daily = db.backup_into(&dir.path().join("backups"), None, 30).unwrap();
        assert!(daily.exists());
        assert!(p2.exists(), "daily prune must not touch presync backups");
        assert!(latest_daily_backup_stamp(&dir.path().join("backups")).is_some());
    }
}
