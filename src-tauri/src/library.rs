//! 词典页数据层：书目、条目分页（CONTRACT §1 `list_entries`：三态计数按当前筛选，dup_count 按
//! `artist_parse.signature` 分组——一次 `GROUP BY signature` 子查询 LEFT JOIN 进来，不逐行相关子查询（v0.1.1 #17），
//! keyword 对 title/tags 不区分大小写 LIKE，path_prefix 用 json_extract，
//! batch 用 json_each），条目详情，三态决策 + `membership_audit`，撤销，用户备注，黑名单，
//! sync_log / app_event 读取。同步永不触碰这里写的决策表（设计 §0 铁律 2）。

use std::collections::HashSet;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use serde_json::{json, Value};

use crate::artist;
use crate::db::{json_str_vec, json_value, now_iso, Db};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::sync::upstream::library_of;

const AUDIT_UNDONE_KEY: &str = "undone_audit_ids";

// ---------- 书目 ----------

pub fn list_books(conn: &Connection) -> AppResult<Vec<Book>> {
    let mut stmt = conn.prepare(
        "SELECT b.codex_id, b.type, b.title, b.version, b.author, b.entry_count, b.tree_json, b.update_filters_json,
            (SELECT COUNT(*) FROM codex_entry e WHERE e.codex_id = b.codex_id AND e.removed_at IS NULL),
            (SELECT COUNT(*) FROM codex_entry e JOIN library_membership m ON m.entry_id = e.entry_id
               WHERE e.codex_id = b.codex_id AND e.removed_at IS NULL AND m.decision = 'include'),
            (SELECT COUNT(*) FROM codex_entry e JOIN library_membership m ON m.entry_id = e.entry_id
               WHERE e.codex_id = b.codex_id AND e.removed_at IS NULL AND m.decision = 'exclude')
         FROM codex_book b ORDER BY b.codex_id",
    )?;
    let rows = stmt.query_map([], |r| {
        let total: i64 = r.get(8)?;
        let include: i64 = r.get(9)?;
        let exclude: i64 = r.get(10)?;
        let tree_json: String = r.get(6)?;
        let filters_json: Option<String> = r.get(7)?;
        let entry_count: Option<i64> = r.get(5)?;
        Ok(Book {
            codex_id: r.get(0)?,
            r#type: r.get(1)?,
            title: r.get(2)?,
            version: r.get(3)?,
            author: r.get(4)?,
            entry_count: entry_count.unwrap_or(total),
            tree: serde_json::from_str(&tree_json).unwrap_or_default(),
            update_filters: filters_json.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            counts: BookCounts { include, exclude, undecided: total - include - exclude, total },
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ---------- 条目分页 ----------

struct Filter {
    sql: String,
    args: Vec<Box<dyn ToSql>>,
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn build_filter(q: &EntryQuery, with_decision: bool) -> Filter {
    let mut sql = String::from("e.codex_id = ? AND e.removed_at IS NULL");
    let mut args: Vec<Box<dyn ToSql>> = vec![Box::new(q.codex_id.clone())];
    // 前端约定：键缺失 = 不筛；显式空数组 = 一个都不选 → 0 条
    if let Some(r) = q.rating.as_ref() {
        if r.is_empty() {
            sql.push_str(" AND 0");
        } else {
            sql.push_str(&format!(" AND e.rating IN ({})", vec!["?"; r.len()].join(",")));
            for x in r {
                args.push(Box::new(x.as_str().to_string()));
            }
        }
    }
    if with_decision {
        if let Some(d) = q.decision.as_ref() {
            if d.is_empty() {
                sql.push_str(" AND 0");
            } else {
                sql.push_str(&format!(" AND COALESCE(m.decision, 'undecided') IN ({})", vec!["?"; d.len()].join(",")));
                for x in d {
                    args.push(Box::new(x.as_str().to_string()));
                }
            }
        }
    }
    if let Some(b) = q.batch.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND e.update_batches_json IS NOT NULL AND EXISTS (SELECT 1 FROM json_each(e.update_batches_json) WHERE value = ?)");
        args.push(Box::new(b.clone()));
    }
    if let Some(r) = q.first_seen_release.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND e.first_seen_release = ?");
        args.push(Box::new(r.clone()));
    }
    if let Some(p) = q.path_prefix.as_ref() {
        for (i, seg) in p.iter().enumerate() {
            sql.push_str(&format!(" AND json_extract(e.path_json, '$[{i}]') = ?"));
            args.push(Box::new(seg.clone()));
        }
    }
    if let Some(k) = q.keyword.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sql.push_str(" AND (e.title LIKE ? ESCAPE '\\' OR e.tags LIKE ? ESCAPE '\\')");
        let pat = format!("%{}%", like_escape(k));
        args.push(Box::new(pat.clone()));
        args.push(Box::new(pat));
    }
    if q.multi_image_only.unwrap_or(false) {
        sql.push_str(" AND (SELECT COUNT(*) FROM entry_image i WHERE i.entry_id = e.entry_id) > 1");
    }
    if q.dup_only.unwrap_or(false) {
        sql.push_str(" AND COALESCE(d.n, 0) >= 2");
    }
    Filter { sql, args }
}

/// `d` = 每个非空 signature 的未删除条目数（一次 GROUP BY，三条查询共用）；signature 为空的条目 `d.n` 为 NULL。
const ENTRY_FROM: &str = "FROM codex_entry e
    LEFT JOIN library_membership m ON m.entry_id = e.entry_id
    LEFT JOIN artist_parse p ON p.entry_id = e.entry_id
    LEFT JOIN (SELECT p2.signature AS signature, COUNT(*) AS n
               FROM artist_parse p2 JOIN codex_entry e2 ON e2.entry_id = p2.entry_id
               WHERE e2.removed_at IS NULL AND p2.signature IS NOT NULL AND p2.signature != ''
               GROUP BY p2.signature) d ON d.signature = p.signature";

/// v0.1.2（CONTRACT §7）：把 `sort` / `sort_dir` 变成 ORDER BY 片段。
/// 未知 `sort` 或 `sort_dir` → `bad_request`；缺省方向见 `EntrySort::default_desc`。
/// 三个可能为 NULL 的键（`m.decided_at` / `i0.downloaded_at` / `r.published_at`）先按 `IS NULL` 升序，
/// 让没有值的条目无论方向都排最后；最后一律补 `e.book_order ASC` 作稳定键。
fn order_clause(q: &EntryQuery) -> AppResult<String> {
    let sort = match q.sort.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => EntrySort::BookOrder,
        Some(s) => EntrySort::parse(s).ok_or_else(|| {
            let allowed = EntrySort::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(" | ");
            AppError::bad_request(format!("未知 sort: {s}（可选 {allowed}）"))
        })?,
    };
    let desc = match q.sort_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => sort.default_desc(),
        Some("asc") => false,
        Some("desc") => true,
        Some(d) => return Err(AppError::bad_request(format!("未知 sort_dir: {d}（可选 asc | desc）"))),
    };
    let dir = if desc { "DESC" } else { "ASC" };
    let key = match sort {
        EntrySort::BookOrder => return Ok(format!("e.book_order {dir}")),
        EntrySort::Title => format!("e.title COLLATE NOCASE {dir}"),
        EntrySort::DecidedAt => format!("(m.decided_at IS NULL) ASC, m.decided_at {dir}"),
        EntrySort::SyncedAt => format!("(i0.downloaded_at IS NULL) ASC, i0.downloaded_at {dir}"),
        EntrySort::FirstSeen => format!("(r.published_at IS NULL) ASC, r.published_at {dir}"),
    };
    Ok(format!("{key}, e.book_order ASC"))
}

pub fn list_entries(conn: &Connection, q: &EntryQuery) -> AppResult<EntryPage> {
    if q.codex_id.is_empty() {
        return Err(AppError::bad_request("codex_id 不能为空"));
    }
    let limit = if q.limit <= 0 { 100 } else { q.limit.min(2000) };
    let offset = q.offset.max(0);
    let full = build_filter(q, true);
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) {ENTRY_FROM} WHERE {}", full.sql),
        params_from_iter(full.args.iter().map(|b| b.as_ref())),
        |r| r.get(0),
    )?;
    let base = build_filter(q, false);
    let mut counts = DecisionCounts::default();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT COALESCE(m.decision, 'undecided'), COUNT(*) {ENTRY_FROM} WHERE {} GROUP BY 1",
            base.sql
        ))?;
        let rows = stmt.query_map(params_from_iter(base.args.iter().map(|b| b.as_ref())), |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (d, n) = row?;
            match d.as_str() {
                "include" => counts.include = n,
                "exclude" => counts.exclude = n,
                _ => counts.undecided += n,
            }
        }
    }
    let order = order_clause(q)?;
    // i0 / r 都是一对一（entry_image 主键 (entry_id, ordinal)，codex_release 主键 release_id），不会让行数膨胀；
    // 只有 items 查询需要它们（取主图路径 + 排序键），COUNT / counts 两条查询不带。
    let sql = format!(
        "SELECT e.entry_id, e.codex_id, e.book_order, e.title, e.tags, e.rating, e.update_batches_json, e.path_json,
                COALESCE(m.decision, 'undecided'), i0.thumb_path, i0.local_path, e.image_w, e.image_h,
                (SELECT COUNT(*) FROM entry_image i WHERE i.entry_id = e.entry_id),
                COALESCE(d.n, 1),
                e.is_new
         {ENTRY_FROM}
         LEFT JOIN entry_image i0 ON i0.entry_id = e.entry_id AND i0.ordinal = 0
         LEFT JOIN codex_release r ON r.release_id = e.first_seen_release
         WHERE {} ORDER BY {order} LIMIT ? OFFSET ?",
        full.sql
    );
    let mut args: Vec<&dyn ToSql> = full.args.iter().map(|b| b.as_ref()).collect();
    args.push(&limit);
    args.push(&offset);
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(params_from_iter(args), |r| {
            let tags: String = r.get(4)?;
            let rating: String = r.get(5)?;
            let decision: String = r.get(8)?;
            Ok(EntryCard {
                entry_id: r.get(0)?,
                codex_id: r.get(1)?,
                book_order: r.get(2)?,
                title: r.get(3)?,
                tags_first_line: first_line(&tags, 120),
                rating: Rating::parse(&rating).unwrap_or(Rating::Safe),
                batches: json_str_vec(r.get(6)?),
                path: json_str_vec(r.get(7)?),
                decision: Decision::parse(&decision).unwrap_or(Decision::Undecided),
                thumb_path: r.get(9)?,
                image_path: r.get(10)?,
                image_w: r.get(11)?,
                image_h: r.get(12)?,
                image_count: r.get(13)?,
                dup_count: r.get(14)?,
                is_new: r.get::<_, i64>(15)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EntryPage { total, counts, items })
}

pub fn first_line(tags: &str, max_chars: usize) -> String {
    let line = tags.lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
    line.chars().take(max_chars).collect()
}

// ---------- 详情 ----------

pub fn load_entry(conn: &Connection, entry_id: &str) -> AppResult<Option<Entry>> {
    conn.query_row(
        "SELECT entry_id, codex_id, book_order, entry_version, title, path_json, tags, negative, character_prompts_json, note, rating, is_new,
                update_batches_json, asset_rev, image_w, image_h, first_seen_release, last_seen_release, removed_at
         FROM codex_entry WHERE entry_id = ?1",
        params![entry_id],
        |r| {
            let rating: String = r.get(10)?;
            let cp: Option<String> = r.get(8)?;
            Ok(Entry {
                entry_id: r.get(0)?,
                codex_id: r.get(1)?,
                book_order: r.get(2)?,
                entry_version: r.get(3)?,
                title: r.get(4)?,
                path: json_str_vec(r.get(5)?),
                tags: r.get(6)?,
                negative: r.get(7)?,
                character_prompts: cp.and_then(|s| serde_json::from_str(&s).ok()),
                note: r.get(9)?,
                rating: Rating::parse(&rating).unwrap_or(Rating::Safe),
                is_new: r.get::<_, i64>(11)? != 0,
                update_batches: json_str_vec(r.get(12)?),
                asset_rev: r.get(13)?,
                image_w: r.get(14)?,
                image_h: r.get(15)?,
                first_seen_release: r.get(16)?,
                last_seen_release: r.get(17)?,
                removed_at: r.get(18)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn load_images(conn: &Connection, entry_id: &str) -> AppResult<Vec<EntryImage>> {
    let mut stmt = conn.prepare(
        "SELECT ordinal, image_file, original_file, raw_tag, local_path, thumb_path, original_local_path, bytes, error
         FROM entry_image WHERE entry_id = ?1 ORDER BY ordinal",
    )?;
    let rows = stmt.query_map(params![entry_id], |r| {
        Ok(EntryImage {
            ordinal: r.get(0)?,
            image_file: r.get(1)?,
            original_file: r.get(2)?,
            raw_tag: r.get(3)?,
            local_path: r.get(4)?,
            thumb_path: r.get(5)?,
            original_local_path: r.get(6)?,
            bytes: r.get(7)?,
            error: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn load_parse(conn: &Connection, entry_id: &str) -> AppResult<Option<ArtistParse>> {
    conn.query_row(
        "SELECT artists_json, auxiliary_json, negative_tail_json, signature, parse_warnings FROM artist_parse WHERE entry_id = ?1",
        params![entry_id],
        |r| {
            let artists: String = r.get(0)?;
            let aux: String = r.get(1)?;
            let neg: String = r.get(2)?;
            let warnings: Option<String> = r.get(4)?;
            Ok(ArtistParse {
                artists: serde_json::from_str(&artists).unwrap_or_default(),
                auxiliary: serde_json::from_str(&aux).unwrap_or_default(),
                negative_tail: serde_json::from_str(&neg).unwrap_or_default(),
                signature: r.get(3)?,
                warnings: warnings.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_entry(conn: &Connection, entry_id: &str) -> AppResult<EntryDetail> {
    let entry = load_entry(conn, entry_id)?.ok_or_else(|| AppError::not_found(format!("条目不存在: {entry_id}")))?;
    let images = load_images(conn, entry_id)?;
    let membership = conn
        .query_row(
            "SELECT decision, note, decided_at FROM library_membership WHERE entry_id = ?1",
            params![entry_id],
            |r| {
                let d: String = r.get(0)?;
                Ok(Membership { decision: Decision::parse(&d).unwrap_or(Decision::Undecided), member_note: r.get(1)?, decided_at: r.get(2)? })
            },
        )
        .optional()?;
    let audit = {
        let mut stmt = conn.prepare("SELECT id, from_decision, to_decision, at, via FROM membership_audit WHERE entry_id = ?1 ORDER BY id DESC LIMIT 100")?;
        let rows = stmt.query_map(params![entry_id], |r| {
            let from: Option<String> = r.get(1)?;
            let to: String = r.get(2)?;
            let via: String = r.get(4)?;
            Ok(AuditRow {
                id: r.get(0)?,
                from_decision: from.and_then(|s| Decision::parse(&s)),
                to_decision: Decision::parse(&to).unwrap_or(Decision::Undecided),
                at: r.get(3)?,
                via: DecisionVia::parse(&via),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let parse = load_parse(conn, entry_id)?;
    let siblings = match parse.as_ref().map(|p| p.signature.as_str()).filter(|s| !s.is_empty()) {
        Some(sig) => {
            let mut stmt = conn.prepare(
                "SELECT e.entry_id, e.codex_id, e.title, COALESCE(m.decision, 'undecided')
                 FROM artist_parse p JOIN codex_entry e ON e.entry_id = p.entry_id
                 LEFT JOIN library_membership m ON m.entry_id = e.entry_id
                 WHERE p.signature = ?1 AND e.entry_id != ?2 AND e.removed_at IS NULL ORDER BY e.codex_id, e.book_order",
            )?;
            let rows = stmt.query_map(params![sig, entry_id], |r| {
                let d: String = r.get(3)?;
                Ok(Sibling { entry_id: r.get(0)?, codex_id: r.get(1)?, title: r.get(2)?, decision: Decision::parse(&d).unwrap_or(Decision::Undecided) })
            })?
            .collect::<Result<Vec<_>, _>>()?;
            rows
        }
        None => vec![],
    };
    Ok(EntryDetail { entry, images, membership, audit, parse, siblings })
}

// ---------- 决策 ----------

fn current_decision(conn: &Connection, entry_id: &str) -> AppResult<Option<Decision>> {
    Ok(conn
        .query_row("SELECT decision FROM library_membership WHERE entry_id = ?1", params![entry_id], |r| r.get::<_, String>(0))
        .optional()?
        .and_then(|s| Decision::parse(&s)))
}

fn write_decision(conn: &Connection, entry_id: &str, codex_id: &str, from: Option<Decision>, to: Decision, via: DecisionVia, now: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO library_membership(entry_id, library, decision, note, decided_at) VALUES (?1, ?2, ?3, NULL, ?4)
         ON CONFLICT(entry_id) DO UPDATE SET decision = excluded.decision, decided_at = excluded.decided_at",
        params![entry_id, library_of(codex_id), to.as_str(), now],
    )?;
    conn.execute(
        "INSERT INTO membership_audit(entry_id, from_decision, to_decision, at, via) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![entry_id, from.map(|d| d.as_str()), to.as_str(), now, via.as_str()],
    )?;
    Ok(())
}

pub fn set_decision(db: &Db, entry_ids: &[String], decision: Decision, via: DecisionVia) -> AppResult<i64> {
    let now = now_iso();
    let changed = db.with(|conn| {
        let tx = conn.transaction()?;
        let mut changed = 0i64;
        for id in entry_ids {
            let codex_id: Option<String> = tx
                .query_row("SELECT codex_id FROM codex_entry WHERE entry_id = ?1", params![id], |r| r.get(0))
                .optional()?;
            let Some(codex_id) = codex_id else { continue };
            let from = current_decision(&tx, id)?;
            if from == Some(decision) || (from.is_none() && decision == Decision::Undecided) {
                continue;
            }
            write_decision(&tx, id, &codex_id, from, decision, via, &now)?;
            changed += 1;
        }
        tx.commit()?;
        Ok(changed)
    })?;
    if via == DecisionVia::Batch || entry_ids.len() > 1 {
        db.log_event("info", "decision", &format!("批量决策 {} → {}", entry_ids.len(), decision.as_str()), json!({ "changed": changed, "via": via.as_str() }));
    }
    Ok(changed)
}

pub fn set_member_note(conn: &Connection, entry_id: &str, note: &str) -> AppResult<()> {
    let codex_id: String = conn
        .query_row("SELECT codex_id FROM codex_entry WHERE entry_id = ?1", params![entry_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("条目不存在: {entry_id}")))?;
    let note_val: Option<&str> = if note.trim().is_empty() { None } else { Some(note) };
    conn.execute(
        "INSERT INTO library_membership(entry_id, library, decision, note, decided_at) VALUES (?1, ?2, 'undecided', ?3, ?4)
         ON CONFLICT(entry_id) DO UPDATE SET note = excluded.note",
        params![entry_id, library_of(&codex_id), note_val, now_iso()],
    )?;
    Ok(())
}

/// 撤销最近一条 `via != 'undo'` 且尚未被撤销过的审计记录；恢复 `from_decision`（NULL 视为 undecided），写一条 `via='undo'`。
pub fn undo_decision(db: &Db) -> AppResult<Option<UndoResult>> {
    db.with(|conn| {
        let tx = conn.transaction()?;
        let undone: Vec<i64> = crate::db::app_state_get(&tx, AUDIT_UNDONE_KEY)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let undone_set: HashSet<i64> = undone.iter().copied().collect();
        let mut stmt = tx.prepare("SELECT id, entry_id, from_decision FROM membership_audit WHERE via != 'undo' ORDER BY id DESC LIMIT 200")?;
        let candidates: Vec<(i64, String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        let Some((id, entry_id, from)) = candidates.into_iter().find(|(id, _, _)| !undone_set.contains(id)) else {
            tx.commit()?;
            return Ok(None);
        };
        let restored = from.as_deref().and_then(Decision::parse).unwrap_or(Decision::Undecided);
        let codex_id: String = tx.query_row("SELECT codex_id FROM codex_entry WHERE entry_id = ?1", params![entry_id], |r| r.get(0))?;
        let current = current_decision(&tx, &entry_id)?;
        write_decision(&tx, &entry_id, &codex_id, current, restored, DecisionVia::Undo, &now_iso())?;
        let mut list = undone;
        list.push(id);
        if list.len() > 500 {
            let cut = list.len() - 500;
            list.drain(..cut);
        }
        crate::db::app_state_set(&tx, AUDIT_UNDONE_KEY, &serde_json::to_string(&list)?)?;
        tx.commit()?;
        Ok(Some(UndoResult { entry_id, restored }))
    })
}

// ---------- 黑名单 ----------

pub fn blacklist_list(conn: &Connection) -> AppResult<Vec<BlacklistItem>> {
    let mut stmt = conn.prepare("SELECT name, reason, added_at FROM artist_blacklist ORDER BY added_at DESC, name")?;
    let rows = stmt.query_map([], |r| Ok(BlacklistItem { name: r.get(0)?, reason: r.get(1)?, added_at: r.get(2)? }))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn blacklist_add(conn: &Connection, name: &str, reason: &str) -> AppResult<()> {
    let canon = artist::canonical_artist_tag_name(name);
    if canon.is_empty() {
        return Err(AppError::bad_request("画师名为空"));
    }
    let reason: Option<&str> = if reason.trim().is_empty() { None } else { Some(reason) };
    conn.execute(
        "INSERT INTO artist_blacklist(name, reason, added_at) VALUES (?1, ?2, ?3) ON CONFLICT(name) DO UPDATE SET reason = excluded.reason",
        params![canon, reason, now_iso()],
    )?;
    Ok(())
}

pub fn blacklist_remove(conn: &Connection, name: &str) -> AppResult<()> {
    let canon = artist::canonical_artist_tag_name(name);
    conn.execute("DELETE FROM artist_blacklist WHERE name = ?1", params![canon])?;
    Ok(())
}

pub fn blacklist_set(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT name FROM artist_blacklist")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<HashSet<_>, _>>()?)
}

// ---------- 日志 ----------

pub fn sync_logs(conn: &Connection, limit: i64) -> AppResult<Vec<SyncLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, started_at, finished_at, kind, release_id, added, changed, images_ok, images_failed, failures_json
         FROM sync_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit.clamp(1, 500)], |r| {
        let failures: Option<String> = r.get(9)?;
        Ok(SyncLog {
            id: r.get(0)?,
            started_at: r.get(1)?,
            finished_at: r.get(2)?,
            kind: r.get(3)?,
            release_id: r.get(4)?,
            added: r.get(5)?,
            changed: r.get(6)?,
            images_ok: r.get(7)?,
            images_failed: r.get(8)?,
            failures: failures.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn events_recent(conn: &Connection, limit: i64) -> AppResult<Vec<AppEvent>> {
    let mut stmt = conn.prepare("SELECT id, at, level, area, message, data_json FROM app_event ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit.clamp(1, 1000)], |r| {
        Ok(AppEvent { id: r.get(0)?, at: r.get(1)?, level: r.get(2)?, area: r.get(3)?, message: r.get(4)?, data: json_value(r.get(5)?) })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn app_info_release(conn: &Connection) -> AppResult<Option<String>> {
    crate::db::app_state_get(conn, "latest_release")
}

pub fn value_or_null(v: Option<Value>) -> Value {
    v.unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::upsert::{ensure_release, upsert_book};
    use crate::sync::upstream::parse_book;

    fn seeded() -> Db {
        let db = Db::open_memory().unwrap();
        let book = parse_book(include_str!("sync/fixtures/book_v1.json")).unwrap();
        db.with(|c| {
            ensure_release(c, "r-1", "p", "h", "f", "codex_raw/r-1")?;
            upsert_book(c, "r-1", &book, None, "t")?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn q(codex: &str) -> EntryQuery {
        EntryQuery { codex_id: codex.into(), limit: 50, ..Default::default() }
    }

    #[test]
    fn list_entries_filters_and_counts() {
        let db = seeded();
        db.with(|c| {
            let page = list_entries(c, &q("artist_nai5_personal"))?;
            assert_eq!(page.total, 3);
            assert_eq!(page.counts.undecided, 3);
            assert_eq!(page.items[0].entry_id, "artist_nai5_personal_demo_0001");
            assert_eq!(page.items[2].image_count, 2);
            assert_eq!(page.items[2].batches, vec!["2026.9.5", "2026.9.6"]);
            assert_eq!(page.items[2].tags_first_line, "<artist>");
            let mut kw = q("artist_nai5_personal");
            kw.keyword = Some("INKWELL".into());
            assert_eq!(list_entries(c, &kw)?.total, 1);
            let mut batch = q("artist_nai5_personal");
            batch.batch = Some("2026.9.6".into());
            assert_eq!(list_entries(c, &batch)?.total, 1);
            let mut prefix = q("artist_nai5_personal");
            prefix.path_prefix = Some(vec!["单画师词典".into(), "作者甲(无原图)".into()]);
            assert_eq!(list_entries(c, &prefix)?.total, 2);
            let mut multi = q("artist_nai5_personal");
            multi.multi_image_only = Some(true);
            assert_eq!(list_entries(c, &multi)?.total, 1);
            let mut safe = q("artist_nai5_personal");
            safe.rating = Some(vec![Rating::Safe]);
            assert_eq!(list_entries(c, &safe)?.total, 3);
            let mut none = q("artist_nai5_personal");
            none.rating = Some(vec![]);
            let page = list_entries(c, &none)?;
            assert_eq!(page.total, 0, "显式空数组 = 一个都不选");
            assert_eq!(page.counts.undecided, 0);
            let mut dup = q("artist_nai5_personal");
            dup.dup_only = Some(true);
            assert_eq!(list_entries(c, &dup)?.total, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn decisions_audit_undo_and_counts() {
        let db = seeded();
        let ids = vec!["artist_nai5_personal_demo_0001".to_string(), "artist_nai5_personal_demo_0002".to_string()];
        assert_eq!(set_decision(&db, &ids, Decision::Include, DecisionVia::Batch).unwrap(), 2);
        assert_eq!(set_decision(&db, &ids, Decision::Include, DecisionVia::Click).unwrap(), 0);
        assert_eq!(set_decision(&db, &ids[..1], Decision::Exclude, DecisionVia::Key).unwrap(), 1);
        db.with(|c| {
            let mut dq = q("artist_nai5_personal");
            dq.decision = Some(vec![Decision::Include]);
            let page = list_entries(c, &dq)?;
            assert_eq!(page.total, 1);
            // counts 不受 decision 筛选影响
            assert_eq!(page.counts.include, 1);
            assert_eq!(page.counts.exclude, 1);
            assert_eq!(page.counts.undecided, 1);
            let books = list_books(c)?;
            assert_eq!(books[0].counts.include, 1);
            assert_eq!(books[0].counts.total, 3);
            assert_eq!(books[0].tree.len(), 2);
            assert_eq!(books[0].update_filters.len(), 3);
            let detail = get_entry(c, "artist_nai5_personal_demo_0001")?;
            assert_eq!(detail.audit.len(), 2);
            assert_eq!(detail.audit[0].from_decision, Some(Decision::Include));
            assert_eq!(detail.membership.as_ref().unwrap().decision, Decision::Exclude);
            set_member_note(c, "artist_nai5_personal_demo_0001", "备注")?;
            assert_eq!(get_entry(c, "artist_nai5_personal_demo_0001")?.membership.unwrap().member_note.as_deref(), Some("备注"));
            Ok(())
        })
        .unwrap();
        // 撤销：0001 exclude → include；再撤销：0002 include → undecided；再撤销：0001 include → undecided；再无可撤销
        let u1 = undo_decision(&db).unwrap().unwrap();
        assert_eq!((u1.entry_id.as_str(), u1.restored), ("artist_nai5_personal_demo_0001", Decision::Include));
        let u2 = undo_decision(&db).unwrap().unwrap();
        assert_eq!((u2.entry_id.as_str(), u2.restored), ("artist_nai5_personal_demo_0002", Decision::Undecided));
        let u3 = undo_decision(&db).unwrap().unwrap();
        assert_eq!((u3.entry_id.as_str(), u3.restored), ("artist_nai5_personal_demo_0001", Decision::Undecided));
        assert!(undo_decision(&db).unwrap().is_none());
        db.with(|c| {
            let d = get_entry(c, "artist_nai5_personal_demo_0001")?;
            assert_eq!(d.membership.as_ref().unwrap().decision, Decision::Undecided);
            assert_eq!(d.membership.as_ref().unwrap().member_note.as_deref(), Some("备注"), "撤销不能丢备注");
            assert!(d.audit.iter().filter(|a| a.via == DecisionVia::Undo).count() == 2);
            Ok(())
        })
        .unwrap();
        // 撤销之后的新决策优先被撤销
        set_decision(&db, &ids[1..], Decision::Exclude, DecisionVia::Click).unwrap();
        let u4 = undo_decision(&db).unwrap().unwrap();
        assert_eq!(u4.entry_id, "artist_nai5_personal_demo_0002");
    }

    fn ids(page: &EntryPage) -> Vec<&str> {
        page.items.iter().map(|i| i.entry_id.as_str()).collect()
    }

    fn sorted(c: &Connection, sort: &str, dir: Option<&str>) -> AppResult<Vec<String>> {
        let mut query = q("artist_nai5_personal");
        query.sort = Some(sort.into());
        query.sort_dir = dir.map(|d| d.to_string());
        Ok(ids(&list_entries(c, &query)?).into_iter().map(String::from).collect())
    }

    const E1: &str = "artist_nai5_personal_demo_0001";
    const E2: &str = "artist_nai5_personal_demo_0002";
    const E3: &str = "artist_nai5_personal_combo_v1_0001";

    #[test]
    fn sort_synced_at_puts_undownloaded_last() {
        let db = seeded();
        db.with(|c| {
            // E3 主图先落地，E1 后落地，E2 没下载；E3 的第二张图（ordinal 1）更晚落地，不能影响排序
            c.execute("UPDATE entry_image SET downloaded_at = '2026-09-01T00:00:00Z' WHERE entry_id = ?1 AND ordinal = 0", params![E3])?;
            c.execute("UPDATE entry_image SET downloaded_at = '2026-09-02T00:00:00Z' WHERE entry_id = ?1 AND ordinal = 0", params![E1])?;
            c.execute("UPDATE entry_image SET downloaded_at = '2026-09-03T00:00:00Z' WHERE entry_id = ?1 AND ordinal = 1", params![E3])?;
            assert_eq!(sorted(c, "synced_at", Some("asc"))?, vec![E3, E1, E2], "升序：未下载排最后");
            assert_eq!(sorted(c, "synced_at", Some("desc"))?, vec![E1, E3, E2], "降序：未下载仍排最后");
            assert_eq!(sorted(c, "synced_at", None)?, sorted(c, "synced_at", Some("asc"))?, "synced_at 缺省升序");
            let page = list_entries(c, &EntryQuery { sort: Some("synced_at".into()), ..q("artist_nai5_personal") })?;
            assert_eq!(page.total, 3, "多张图的条目不能因为 JOIN 而重复计数");
            assert_eq!(page.items.len(), 3);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn sort_first_seen_newest_release_first_local_last() {
        let db = seeded();
        db.with(|c| {
            // r-2 比 r-1 晚发布；E2 首见于 r-2，E3 是本地条目（没有 release 行）
            ensure_release(c, "r-1", "2026-09-01T00:00:00Z", "h", "f", "codex_raw/r-1")?;
            ensure_release(c, "r-2", "2026-09-05T00:00:00Z", "h2", "f", "codex_raw/r-2")?;
            c.execute("UPDATE codex_entry SET first_seen_release = 'r-2' WHERE entry_id = ?1", params![E2])?;
            c.execute("UPDATE codex_entry SET first_seen_release = 'local' WHERE entry_id = ?1", params![E3])?;
            assert_eq!(sorted(c, "first_seen", Some("desc"))?, vec![E2, E1, E3], "降序：新 release 在前，local 最后");
            assert_eq!(sorted(c, "first_seen", Some("asc"))?, vec![E1, E2, E3], "升序：local 仍最后");
            assert_eq!(sorted(c, "first_seen", None)?, sorted(c, "first_seen", Some("desc"))?, "first_seen 缺省降序");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn sort_decided_at_and_title_defaults() {
        let db = seeded();
        set_decision(&db, &[E3.to_string()], Decision::Include, DecisionVia::Click).unwrap();
        set_decision(&db, &[E1.to_string()], Decision::Exclude, DecisionVia::Click).unwrap();
        db.with(|c| {
            // E3 先决策、E1 后决策、E2 未决策
            c.execute("UPDATE library_membership SET decided_at = '2026-09-01T00:00:00Z' WHERE entry_id = ?1", params![E3])?;
            c.execute("UPDATE library_membership SET decided_at = '2026-09-02T00:00:00Z' WHERE entry_id = ?1", params![E1])?;
            assert_eq!(sorted(c, "decided_at", None)?, vec![E1, E3, E2], "decided_at 缺省降序，未决策最后");
            assert_eq!(sorted(c, "decided_at", Some("asc"))?, vec![E3, E1, E2], "升序：未决策仍最后");
            // title 缺省升序、不区分大小写：inkwell < lumen… < 作者丙…
            assert_eq!(sorted(c, "title", None)?, vec![E1, E2, E3]);
            assert_eq!(sorted(c, "title", Some("desc"))?, vec![E3, E2, E1]);
            // book_order 缺省升序；显式 desc 反转
            assert_eq!(sorted(c, "book_order", None)?, vec![E1, E2, E3]);
            assert_eq!(sorted(c, "book_order", Some("desc"))?, vec![E3, E2, E1]);
            assert_eq!(sorted(c, "book_order", None)?, ids(&list_entries(c, &q("artist_nai5_personal"))?), "sort 缺省 = book_order");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn sort_rejects_unknown_values() {
        let db = seeded();
        db.with(|c| {
            let err = sorted(c, "book_order", Some("up")).unwrap_err();
            assert_eq!(err.code, "bad_request", "{err}");
            let err = sorted(c, "random", None).unwrap_err();
            assert_eq!(err.code, "bad_request", "{err}");
            assert!(err.message.contains("first_seen"), "错误信息列出可选值: {err}");
            // 空串视为缺省，不报错
            assert_eq!(sorted(c, "", Some(""))?, vec![E1, E2, E3]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn blacklist_is_canonical() {
        let db = seeded();
        db.with(|c| {
            blacklist_add(c, " Zeta Tan ", "太糊")?;
            let items = blacklist_list(c)?;
            assert_eq!(items[0].name, "zeta_tan");
            assert_eq!(items[0].reason.as_deref(), Some("太糊"));
            blacklist_remove(c, "zeta tan")?;
            assert!(blacklist_list(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
