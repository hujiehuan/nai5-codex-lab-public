//! 条目 upsert（设计 §1.3 第 4 步）：新 id 插入；已有 id 按 `Comparable` 比较，任一不同先把旧行
//! （含 images[]）整行快照进 `codex_entry_history`，再原地 UPDATE 且 `entry_version+1`；
//! `isNew / updateBatches / last_seen_release / book_order` 无条件更新不计版本；
//! `images[]` 逐项 upsert 进 `entry_image`（只写 image_file/original_file/raw_tag，不碰 local_path/asset_rev）；
//! 消失的旧 id 打 `removed_at`，≥10% 则跳过并告警；画师词典条目顺手写 `artist_parse`。

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::artist;
use crate::error::AppResult;
use crate::models::CharacterPrompt;

use super::upstream::{character_prompts_json, is_artist_book, Comparable, NormImage, UpstreamBook};

#[derive(Debug, Default, Clone)]
pub struct UpsertStats {
    pub added: u64,
    pub changed: u64,
    pub unchanged: u64,
    pub removed: u64,
    pub missing: u64,
    pub removed_skipped: bool,
}

pub fn ensure_release(conn: &Connection, release_id: &str, published_at: &str, content_hash: &str, fetched_at: &str, raw_dir: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO codex_release(release_id, published_at, content_hash, fetched_at, raw_dir) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(release_id) DO UPDATE SET published_at=excluded.published_at, content_hash=excluded.content_hash, fetched_at=excluded.fetched_at, raw_dir=excluded.raw_dir",
        params![release_id, published_at, content_hash, fetched_at, raw_dir],
    )?;
    Ok(())
}

pub fn upsert_book_row(conn: &Connection, book: &UpstreamBook, file_sha256: Option<&str>, release_id: Option<&str>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO codex_book(codex_id, type, title, version, author, entry_count, tree_json, update_filters_json, file_sha256, release_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(codex_id) DO UPDATE SET type=excluded.type, title=excluded.title, version=excluded.version, author=excluded.author,
           entry_count=excluded.entry_count, tree_json=excluded.tree_json, update_filters_json=excluded.update_filters_json,
           file_sha256=excluded.file_sha256, release_id=excluded.release_id",
        params![
            book.id,
            book.r#type,
            book.title,
            book.version,
            book.author,
            book.entry_count,
            book.tree.to_string(),
            book.update_filters.as_ref().map(|v| v.to_string()),
            file_sha256,
            release_id,
        ],
    )?;
    Ok(())
}

struct LocalRow {
    entry_version: i64,
    comparable: Comparable,
    snapshot: Value,
}

fn load_local(conn: &Connection, entry_id: &str) -> AppResult<Option<LocalRow>> {
    let row = conn
        .query_row(
            "SELECT entry_version, title, path_json, tags, negative, character_prompts_json, note, rating, is_new, update_batches_json,
                    asset_rev, image_w, image_h, first_seen_release, last_seen_release, removed_at, book_order, codex_id
             FROM codex_entry WHERE entry_id = ?1",
            params![entry_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, Option<i64>>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                    r.get::<_, String>(13)?,
                    r.get::<_, String>(14)?,
                    r.get::<_, Option<String>>(15)?,
                    r.get::<_, i64>(16)?,
                    r.get::<_, String>(17)?,
                ))
            },
        )
        .optional()?;
    let Some((
        entry_version,
        title,
        path_json,
        tags,
        negative,
        cp_json,
        note,
        rating,
        is_new,
        batches_json,
        asset_rev,
        image_w,
        image_h,
        first_seen,
        last_seen,
        removed_at,
        book_order,
        codex_id,
    )) = row
    else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT ordinal, image_file, original_file, raw_tag FROM entry_image WHERE entry_id = ?1 ORDER BY ordinal")?;
    let images: Vec<(i64, NormImage)> = stmt
        .query_map(params![entry_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                NormImage { path: r.get(1)?, original: r.get(2)?, raw_tag: r.get(3)? },
            ))
        })?
        .collect::<Result<_, _>>()?;
    let cp: Option<Vec<CharacterPrompt>> = cp_json.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let path: Vec<String> = serde_json::from_str(&path_json).unwrap_or_default();
    let comparable = Comparable {
        asset_rev: asset_rev.clone(),
        tags: tags.clone(),
        negative: negative.clone(),
        character_prompts: character_prompts_json(&cp),
        title: title.clone(),
        path: path.clone(),
        rating: rating.clone(),
        note: note.clone(),
        images: images.iter().map(|(_, i)| i.clone()).collect(),
    };
    let snapshot = json!({
        "entry": {
            "entry_id": entry_id, "codex_id": codex_id, "book_order": book_order, "entry_version": entry_version,
            "title": title, "path": path, "tags": tags, "negative": negative, "character_prompts": cp, "note": note,
            "rating": rating, "is_new": is_new != 0, "update_batches": batches_json.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
            "asset_rev": asset_rev, "image_w": image_w, "image_h": image_h,
            "first_seen_release": first_seen, "last_seen_release": last_seen, "removed_at": removed_at,
        },
        "images": images.iter().map(|(o, i)| json!({"ordinal": o, "path": i.path, "original": i.original, "raw_tag": i.raw_tag})).collect::<Vec<_>>(),
    });
    Ok(Some(LocalRow { entry_version, comparable, snapshot }))
}

fn upsert_images(conn: &Connection, entry_id: &str, images: &[NormImage]) -> AppResult<()> {
    for (ordinal, img) in images.iter().enumerate() {
        conn.execute(
            "INSERT INTO entry_image(entry_id, ordinal, image_file, original_file, raw_tag) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(entry_id, ordinal) DO UPDATE SET
               original_file = excluded.original_file,
               raw_tag = excluded.raw_tag,
               local_path = CASE WHEN excluded.image_file = entry_image.image_file THEN entry_image.local_path ELSE NULL END,
               thumb_path = CASE WHEN excluded.image_file = entry_image.image_file THEN entry_image.thumb_path ELSE NULL END,
               etag = CASE WHEN excluded.image_file = entry_image.image_file THEN entry_image.etag ELSE NULL END,
               bytes = CASE WHEN excluded.image_file = entry_image.image_file THEN entry_image.bytes ELSE NULL END,
               sha256 = CASE WHEN excluded.image_file = entry_image.image_file THEN entry_image.sha256 ELSE NULL END,
               original_local_path = CASE WHEN excluded.original_file IS entry_image.original_file THEN entry_image.original_local_path ELSE NULL END,
               image_file = excluded.image_file",
            params![entry_id, ordinal as i64, img.path, img.original, img.raw_tag],
        )?;
    }
    conn.execute("DELETE FROM entry_image WHERE entry_id = ?1 AND ordinal >= ?2", params![entry_id, images.len() as i64])?;
    Ok(())
}

pub fn write_artist_parse(conn: &Connection, entry_id: &str, tags: &str, path0: Option<&str>) -> AppResult<()> {
    let p = artist::parse_entry_tags(tags, path0);
    conn.execute(
        "INSERT INTO artist_parse(entry_id, artists_json, auxiliary_json, negative_tail_json, signature, parse_warnings) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(entry_id) DO UPDATE SET artists_json=excluded.artists_json, auxiliary_json=excluded.auxiliary_json,
           negative_tail_json=excluded.negative_tail_json, signature=excluded.signature, parse_warnings=excluded.parse_warnings",
        params![
            entry_id,
            serde_json::to_string(&p.artists)?,
            serde_json::to_string(&p.auxiliary)?,
            serde_json::to_string(&p.negative_tail)?,
            p.signature,
            if p.warnings.is_empty() { None } else { Some(serde_json::to_string(&p.warnings)?) },
        ],
    )?;
    Ok(())
}

/// 一本书的全部条目 upsert，整体在一个事务里。
pub fn upsert_book(conn: &mut Connection, release_id: &str, book: &UpstreamBook, file_sha256: Option<&str>, now: &str) -> AppResult<UpsertStats> {
    let tx = conn.transaction()?;
    let mut stats = UpsertStats::default();
    upsert_book_row(&tx, book, file_sha256, Some(release_id))?;

    let existing: HashSet<String> = {
        let mut stmt = tx.prepare("SELECT entry_id FROM codex_entry WHERE codex_id = ?1 AND removed_at IS NULL")?;
        let ids: HashSet<String> = stmt.query_map(params![book.id], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
        ids
    };
    let mut seen: HashSet<String> = HashSet::with_capacity(book.entries.len());
    let is_artist = is_artist_book(&book.id);

    for (order, e) in book.entries.iter().enumerate() {
        seen.insert(e.id.clone());
        let path_json = serde_json::to_string(&e.path)?;
        let cp_json = character_prompts_json(&e.character_prompts);
        let batches_json = e.update_batches.as_ref().map(|b| serde_json::to_string(b)).transpose()?;
        match load_local(&tx, &e.id)? {
            None => {
                tx.execute(
                    "INSERT INTO codex_entry(entry_id, codex_id, book_order, entry_version, title, path_json, tags, negative, character_prompts_json, note,
                       rating, is_new, update_batches_json, asset_rev, image_w, image_h, first_seen_release, last_seen_release, removed_at)
                     VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16, NULL)",
                    params![
                        e.id, book.id, order as i64, e.title, path_json, e.tags, e.negative, cp_json, e.note, e.rating,
                        e.is_new as i64, batches_json, e.asset_rev, e.image_w, e.image_h, release_id
                    ],
                )?;
                stats.added += 1;
            }
            Some(local) => {
                if local.comparable != e.comparable() {
                    tx.execute(
                        "INSERT OR REPLACE INTO codex_entry_history(entry_id, entry_version, snapshot_json, replaced_at) VALUES (?1, ?2, ?3, ?4)",
                        params![e.id, local.entry_version, local.snapshot.to_string(), now],
                    )?;
                    tx.execute(
                        "UPDATE codex_entry SET entry_version = entry_version + 1, codex_id = ?2, book_order = ?3, title = ?4, path_json = ?5, tags = ?6,
                           negative = ?7, character_prompts_json = ?8, note = ?9, rating = ?10, is_new = ?11, update_batches_json = ?12,
                           asset_rev = ?13, image_w = ?14, image_h = ?15, last_seen_release = ?16, removed_at = NULL
                         WHERE entry_id = ?1",
                        params![
                            e.id, book.id, order as i64, e.title, path_json, e.tags, e.negative, cp_json, e.note, e.rating,
                            e.is_new as i64, batches_json, e.asset_rev, e.image_w, e.image_h, release_id
                        ],
                    )?;
                    stats.changed += 1;
                } else {
                    tx.execute(
                        "UPDATE codex_entry SET book_order = ?2, is_new = ?3, update_batches_json = ?4, last_seen_release = ?5, removed_at = NULL WHERE entry_id = ?1",
                        params![e.id, order as i64, e.is_new as i64, batches_json, release_id],
                    )?;
                    stats.unchanged += 1;
                }
            }
        }
        upsert_images(&tx, &e.id, &e.images)?;
        if is_artist {
            write_artist_parse(&tx, &e.id, &e.tags, e.path.first().map(|s| s.as_str()))?;
        }
    }

    let missing: Vec<&String> = existing.iter().filter(|id| !seen.contains(*id)).collect();
    stats.missing = missing.len() as u64;
    if !missing.is_empty() {
        // NAX pagination isn't an atomic release: retain missing entries and all decisions.
        if book.id == super::nax::BOOK_ID || missing.len() * 10 >= existing.len() {
            stats.removed_skipped = true;
        } else {
            for id in missing {
                tx.execute("UPDATE codex_entry SET removed_at = ?2 WHERE entry_id = ?1", params![id, now])?;
                stats.removed += 1;
            }
        }
    }
    tx.commit()?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::sync::upstream::parse_book;

    const FIXTURE: &str = include_str!("fixtures/book_v1.json");
    const FIXTURE_V2: &str = include_str!("fixtures/book_v2.json");

    fn setup() -> Db {
        let db = Db::open_memory().unwrap();
        db.with(|c| ensure_release(c, "r-1", "2026-09-06T00:00:00Z", "hash1", "2026-09-06T01:00:00Z", "codex_raw/r-1")).unwrap();
        db.with(|c| ensure_release(c, "r-2", "2026-09-07T00:00:00Z", "hash2", "2026-09-07T01:00:00Z", "codex_raw/r-2")).unwrap();
        db
    }

    #[test]
    fn nax_reordering_updates_and_missing_entries_preserve_decisions() {
        let db = setup();
        let mut book = parse_book(FIXTURE).unwrap();
        book.id = super::super::nax::BOOK_ID.into();
        book.entries = (0..11).map(|i| {
            let mut entry = book.entries[0].clone();
            entry.id = super::super::nax::entry_id(&format!("example_{i}"));
            entry.tags = format!("artist:example_{i}");
            entry.title = format!("example_{i}");
            entry
        }).collect();
        let retained_id = book.entries[0].id.clone();
        let updated_id = book.entries[1].id.clone();
        db.with(|c| {
            upsert_book(c, "r-1", &book, Some("old"), "2026-10-03T00:00:00Z")?;
            c.execute("INSERT INTO library_membership(entry_id,library,decision,note,decided_at) VALUES(?1,'artist','include','keep me','2026-10-03T00:00:00Z')", params![retained_id])?;
            c.execute("UPDATE entry_image SET local_path='/tmp/nax.webp',asset_rev='old-image' WHERE entry_id=?1", params![updated_id])?;
            Ok(())
        }).unwrap();
        // Less than the normal 10% removal threshold: NAX must still retain it.
        book.entries.remove(0);
        book.entries[0].note = Some("updated source settings".into());
        book.entries.reverse();
        let stats = db.with(|c| upsert_book(c, "r-2", &book, Some("new"), "2026-10-03T01:00:00Z")).unwrap();
        assert_eq!(stats.removed, 0);
        assert_eq!(stats.missing, 1);
        assert!(stats.removed_skipped);
        assert_eq!(stats.changed, 1);
        db.with(|c| {
            let (decision, note, removed): (String, String, Option<String>) = c.query_row("SELECT m.decision,m.note,e.removed_at FROM codex_entry e JOIN library_membership m ON m.entry_id=e.entry_id WHERE e.entry_id=?1", params![retained_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            assert_eq!((decision.as_str(), note.as_str(), removed), ("include", "keep me", None));
            let path: String = c.query_row("SELECT local_path FROM entry_image WHERE entry_id=?1 AND ordinal=0", params![updated_id], |r| r.get(0))?;
            assert_eq!(path, "/tmp/nax.webp");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn first_sync_inserts_everything_with_images_and_parse() {
        let db = setup();
        let book = parse_book(FIXTURE).unwrap();
        let stats = db.with(|c| upsert_book(c, "r-1", &book, Some("sha-a"), "2026-09-06T01:00:00Z")).unwrap();
        assert_eq!(stats.added, 3);
        assert_eq!(stats.changed, 0);
        db.with(|c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM codex_entry", [], |r| r.get(0))?;
            assert_eq!(n, 3);
            let imgs: i64 = c.query_row("SELECT COUNT(*) FROM entry_image", [], |r| r.get(0))?;
            assert_eq!(imgs, 4);
            let raw: Option<String> = c.query_row(
                "SELECT raw_tag FROM entry_image WHERE entry_id='artist_nai5_personal_combo_v1_0001' AND ordinal=0",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(raw.as_deref(), Some("solo, smile"));
            let sig: String = c.query_row("SELECT signature FROM artist_parse WHERE entry_id='artist_nai5_personal_demo_0001'", [], |r| r.get(0))?;
            assert_eq!(sig, "inkwell@1");
            let bare: String = c.query_row("SELECT artists_json FROM artist_parse WHERE entry_id='artist_nai5_personal_demo_0002'", [], |r| r.get(0))?;
            assert!(bare.contains("lumen brook fieldnote"));
            let order: i64 = c.query_row("SELECT book_order FROM codex_entry WHERE entry_id='artist_nai5_personal_combo_v1_0001'", [], |r| r.get(0))?;
            assert_eq!(order, 2);
            let sha: String = c.query_row("SELECT file_sha256 FROM codex_book WHERE codex_id='artist_nai5_personal'", [], |r| r.get(0))?;
            assert_eq!(sha, "sha-a");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn second_sync_versions_changed_entries_and_marks_removed() {
        let db = setup();
        let v1 = parse_book(FIXTURE).unwrap();
        db.with(|c| upsert_book(c, "r-1", &v1, Some("sha-a"), "2026-09-06T01:00:00Z")).unwrap();
        // 模拟用户已下载主图：local_path/asset_rev 不能被 upsert 清掉（文件名未变）
        db.with(|c| {
            c.execute(
                "UPDATE entry_image SET local_path='/x/a.png', asset_rev='28620c7a6fe3f640' WHERE entry_id='artist_nai5_personal_demo_0001' AND ordinal=0",
                [],
            )?;
            c.execute(
                "INSERT INTO library_membership(entry_id, library, decision, note, decided_at) VALUES ('artist_nai5_personal_demo_0001','artist','include','keep','2026-09-06T02:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let v2 = parse_book(FIXTURE_V2).unwrap();
        let stats = db.with(|c| upsert_book(c, "r-2", &v2, Some("sha-b"), "2026-09-07T01:00:00Z")).unwrap();
        // v2: 0001 只改了 isNew/updateBatches（不计版本）；combo 条目 characterPrompts 被删（计版本）；0002 消失；新增 0003
        assert_eq!(stats.added, 1);
        assert_eq!(stats.changed, 1);
        assert_eq!(stats.unchanged, 1);
        assert_eq!(stats.missing, 1);
        assert!(stats.removed_skipped, "1/3 消失 ≥ 10%，必须跳过自动打标");
        db.with(|c| {
            let v: i64 = c.query_row("SELECT entry_version FROM codex_entry WHERE entry_id='artist_nai5_personal_demo_0001'", [], |r| r.get(0))?;
            assert_eq!(v, 1);
            let batches: String = c.query_row("SELECT update_batches_json FROM codex_entry WHERE entry_id='artist_nai5_personal_demo_0001'", [], |r| r.get(0))?;
            assert!(batches.contains("2026.9.7"));
            let v: i64 = c.query_row("SELECT entry_version FROM codex_entry WHERE entry_id='artist_nai5_personal_combo_v1_0001'", [], |r| r.get(0))?;
            assert_eq!(v, 2);
            let hist: i64 = c.query_row("SELECT COUNT(*) FROM codex_entry_history WHERE entry_id='artist_nai5_personal_combo_v1_0001' AND entry_version=1", [], |r| r.get(0))?;
            assert_eq!(hist, 1);
            let snap: String = c.query_row("SELECT snapshot_json FROM codex_entry_history WHERE entry_id='artist_nai5_personal_combo_v1_0001'", [], |r| r.get(0))?;
            assert!(snap.contains("quillmark"), "快照必须含旧 characterPrompts");
            let removed: Option<String> = c.query_row("SELECT removed_at FROM codex_entry WHERE entry_id='artist_nai5_personal_demo_0002'", [], |r| r.get(0))?;
            assert!(removed.is_none());
            let lp: Option<String> = c.query_row("SELECT local_path FROM entry_image WHERE entry_id='artist_nai5_personal_demo_0001' AND ordinal=0", [], |r| r.get(0))?;
            assert_eq!(lp.as_deref(), Some("/x/a.png"));
            let dec: String = c.query_row("SELECT decision FROM library_membership WHERE entry_id='artist_nai5_personal_demo_0001'", [], |r| r.get(0))?;
            assert_eq!(dec, "include");
            // combo 第二张图被上游删掉 → entry_image 只剩 1 行
            let n: i64 = c.query_row("SELECT COUNT(*) FROM entry_image WHERE entry_id='artist_nai5_personal_combo_v1_0001'", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            let last: String = c.query_row("SELECT last_seen_release FROM codex_entry WHERE entry_id='artist_nai5_personal_demo_0001'", [], |r| r.get(0))?;
            assert_eq!(last, "r-2");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn small_disappearance_marks_removed_and_reappearance_clears_it() {
        let db = setup();
        // 造 20 条，再去掉 1 条（5%）→ 打 removed_at；再回来 → 清掉
        let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let tpl = v["entries"][0].clone();
        let mut entries = Vec::new();
        for i in 0..20 {
            let mut e = tpl.clone();
            e["id"] = serde_json::Value::String(format!("e{i}"));
            e["image"] = serde_json::Value::String(format!("e{i}.png"));
            e["images"] = serde_json::json!([{"path": format!("e{i}.png"), "original": format!("e{i}.png")}]);
            entries.push(e);
        }
        v["entries"] = serde_json::Value::Array(entries.clone());
        let full = parse_book(&v.to_string()).unwrap();
        db.with(|c| upsert_book(c, "r-1", &full, None, "t1")).unwrap();
        v["entries"] = serde_json::Value::Array(entries[1..].to_vec());
        let less = parse_book(&v.to_string()).unwrap();
        let s = db.with(|c| upsert_book(c, "r-2", &less, None, "t2")).unwrap();
        assert_eq!(s.removed, 1);
        assert!(!s.removed_skipped);
        let removed: Option<String> = db
            .with(|c| Ok(c.query_row("SELECT removed_at FROM codex_entry WHERE entry_id='e0'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(removed.as_deref(), Some("t2"));
        let s2 = db.with(|c| upsert_book(c, "r-2", &full, None, "t3")).unwrap();
        assert_eq!(s2.unchanged, 20);
        let removed: Option<String> = db
            .with(|c| Ok(c.query_row("SELECT removed_at FROM codex_entry WHERE entry_id='e0'", [], |r| r.get(0))?))
            .unwrap();
        assert!(removed.is_none());
    }

    /// 所长法典：无图条目入库不产生 entry_image 行，图库页照样列出；决策记在 pack 下；完整池推送不带它。
    #[test]
    fn codex_book_lists_imageless_entries_and_stays_out_of_full_pool() {
        use crate::library;
        use crate::models::{Decision, DecisionVia, EntryQuery};
        let db = setup();
        let raw = r#"{"id":"suozhang_nai5","title":"示例常规法典","version":"2026.10.27","author":"示例编者",
          "tree":[{"name":"各式场景","count":2,"children":[]}],
          "entries":[
            {"id":"suozhang_nai5-0001","title":"网球扣杀","path":["各式场景"],"tags":"1girl, tennis","isNew":true,
             "image":"suozhang_nai5-0001.jpg","imageWidth":753,"imageHeight":1100,"updateBatches":["2026.10.27"]},
            {"id":"suozhang_nai5-0002","title":"编纂者常用画师组","path":["各式场景"],"tags":"artist:x","isNew":false,"image":null}
          ]}"#;
        let book = parse_book(raw).unwrap();
        let stats = db.with(|c| upsert_book(c, "r-1", &book, Some("sha"), "2026-10-28T00:00:00Z")).unwrap();
        assert_eq!(stats.added, 2);
        db.with(|c| {
            let ty: String = c.query_row("SELECT type FROM codex_book WHERE codex_id='suozhang_nai5'", [], |r| r.get(0))?;
            assert_eq!(ty, "codex");
            let images: i64 = c.query_row("SELECT COUNT(*) FROM entry_image WHERE entry_id LIKE 'suozhang_nai5-%'", [], |r| r.get(0))?;
            assert_eq!(images, 1, "只有有图的那条有 entry_image");
            let page = library::list_entries(c, &EntryQuery { codex_id: "suozhang_nai5".into(), ..Default::default() })?;
            assert_eq!(page.total, 2);
            assert_eq!(page.items[1].image_count, 0);
            assert!(page.items[1].image_path.is_none());
            Ok(())
        })
        .unwrap();
        library::set_decision(&db, &["suozhang_nai5-0001".to_string()], Decision::Include, DecisionVia::Click).unwrap();
        let lib: String = db
            .with(|c| Ok(c.query_row("SELECT library FROM library_membership WHERE entry_id='suozhang_nai5-0001'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(lib, "pack");
        assert!(!crate::export::FULL_POOL_CODEXES.contains(&"suozhang_nai5"), "法典不进完整池推送");
    }
}
