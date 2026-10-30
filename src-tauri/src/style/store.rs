//! 画风向量的数据层（CONTRACT §12.1）：`image_embedding` 表（vec 为 f32 小端 blob）、当前模型名 `app_state('style_model')`
//! （缺省 `clip-vit-base-patch32`）、画师词典与 NAX V5 的均值缓存 `app_state('style_mean:artist-sources-v2:<model>')`（base64 的 f32 小端，索引变化时重算）、
//! 待算清单（当前模型缺向量或 `image_sha256` 与 `entry_image.sha256` 不一致的展示图）、计数，以及完整池导出用的 f16 base64。

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::{app_state_get, app_state_set, now_iso};
use crate::error::AppResult;

use super::model::MODEL_NAME;

pub const STYLE_MODEL_KEY: &str = "style_model";

pub fn mean_key(model: &str) -> String {
    // 候选范围增加 NAX V5，首次检索重算均值；各来源已有的原始向量继续复用。
    format!("style_mean:artist-sources-v2:{model}")
}

/// 当前模型名：`app_state('style_model')`，缺 / 空 → 缺省。
pub fn current_model(conn: &Connection) -> AppResult<String> {
    let id = app_state_get(conn, STYLE_MODEL_KEY)?.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| MODEL_NAME.to_string());
    super::catalog::get(&id)?;
    Ok(id)
}

// ---------- 编码 ----------

/// f32 → 小端字节（`image_embedding.vec`）。
pub fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// 小端字节 → f32；长度不是 4 的倍数 → None。
pub fn blob_to_vec(b: &[u8]) -> Option<Vec<f32>> {
    if b.len() % 4 != 0 {
        return None;
    }
    Some(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// base64(f32 小端)：均值缓存用。
pub fn f32_base64(v: &[f32]) -> String {
    base64::engine::general_purpose::STANDARD.encode(vec_to_blob(v))
}

pub fn f32_from_base64(s: &str) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s.trim().as_bytes()).ok()?;
    blob_to_vec(&bytes)
}

/// base64(float16 小端 × dim)：完整池导出的 `vec` / `embedding.mean`（§12.4）。
pub fn f16_base64(v: &[f32]) -> String {
    let mut out = Vec::with_capacity(v.len() * 2);
    for x in v {
        out.extend_from_slice(&half::f16::from_f32(*x).to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(out)
}

/// 测试与对拍用：base64(float16 小端) → f32。
pub fn f16_from_base64(s: &str) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s.trim().as_bytes()).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    Some(bytes.chunks_exact(2).map(|c| half::f16::from_le_bytes([c[0], c[1]]).to_f32()).collect())
}

// ---------- 向量行 ----------

/// 写入 / 覆盖一条向量（同主键覆盖）。
pub fn upsert_vector(conn: &Connection, entry_id: &str, ordinal: i64, model: &str, vec: &[f32], image_sha256: Option<&str>, computed_at: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO image_embedding(entry_id, ordinal, model, dim, vec, image_sha256, computed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(entry_id, ordinal, model) DO UPDATE SET dim = excluded.dim, vec = excluded.vec, image_sha256 = excluded.image_sha256, computed_at = excluded.computed_at",
        params![entry_id, ordinal, model, vec.len() as i64, vec_to_blob(vec), image_sha256, computed_at],
    )?;
    Ok(())
}

pub fn get_vector(conn: &Connection, entry_id: &str, ordinal: i64, model: &str) -> AppResult<Option<Vec<f32>>> {
    let blob: Option<Vec<u8>> = conn
        .query_row("SELECT vec FROM image_embedding WHERE entry_id = ?1 AND ordinal = ?2 AND model = ?3", params![entry_id, ordinal, model], |r| r.get(0))
        .optional()?;
    Ok(blob.and_then(|b| blob_to_vec(&b)))
}

/// 删掉该模型的全部向量与均值缓存（`style_index_rebuild(force=true)`）。返回删掉的行数。
pub fn delete_model(conn: &Connection, model: &str) -> AppResult<usize> {
    let n = conn.execute("DELETE FROM image_embedding WHERE model = ?1", params![model])?;
    conn.execute("DELETE FROM app_state WHERE key = ?1", params![mean_key(model)])?;
    Ok(n)
}

/// 该模型下画风词典展示图（ordinal=0、条目未下架）的向量。旧图包向量保留在库里，但不再参与检索与均值。
pub fn all_vectors(conn: &Connection, model: &str) -> AppResult<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare(
        "SELECT v.entry_id, v.vec FROM image_embedding v JOIN codex_entry e ON e.entry_id = v.entry_id
         WHERE v.model = ?1 AND v.ordinal = 0 AND e.removed_at IS NULL AND e.codex_id IN ('artist_nai5_personal','nax_v5_artist') ORDER BY e.codex_id, e.book_order",
    )?;
    let rows = stmt.query_map(params![model], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().filter_map(|(id, b)| blob_to_vec(&b).map(|v| (id, v))).collect())
}

/// 已索引数：当前模型、画风词典、ordinal=0、条目未下架。
pub fn count_indexed(conn: &Connection, model: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM image_embedding v JOIN codex_entry e ON e.entry_id = v.entry_id WHERE v.model = ?1 AND v.ordinal = 0 AND e.removed_at IS NULL AND e.codex_id IN ('artist_nai5_personal','nax_v5_artist')",
        params![model],
        |r| r.get(0),
    )?)
}

/// 应索引数：画风词典中 ordinal=0 且已下载（`local_path` 非空）、条目未下架的展示图。
pub fn count_total(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM entry_image i JOIN codex_entry e ON e.entry_id = i.entry_id WHERE i.ordinal = 0 AND i.local_path IS NOT NULL AND e.removed_at IS NULL AND e.codex_id IN ('artist_nai5_personal','nax_v5_artist')",
        [],
        |r| r.get(0),
    )?)
}

/// 待算的一张展示图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingImage {
    pub entry_id: String,
    pub ordinal: i64,
    pub local_path: String,
    pub sha256: Option<String>,
}

/// §12.2：当前模型缺向量、或 `image_sha256` 与 `entry_image.sha256` 不一致（图片换了）的展示图；ordinal=0、已下载、未下架。
/// `IS NOT` 把 NULL 也比进去：两边都 NULL（002 之前下载且未补哈希）算一致，不会每次都重算。
pub fn pending(conn: &Connection, model: &str) -> AppResult<Vec<PendingImage>> {
    let mut stmt = conn.prepare(
        "SELECT i.entry_id, i.ordinal, i.local_path, i.sha256
         FROM entry_image i JOIN codex_entry e ON e.entry_id = i.entry_id
         LEFT JOIN image_embedding v ON v.entry_id = i.entry_id AND v.ordinal = i.ordinal AND v.model = ?1
         WHERE i.ordinal = 0 AND i.local_path IS NOT NULL AND e.removed_at IS NULL AND e.codex_id IN ('artist_nai5_personal','nax_v5_artist')
           AND (v.entry_id IS NULL OR v.image_sha256 IS NOT i.sha256)
         ORDER BY e.codex_id, e.book_order",
    )?;
    let rows = stmt
        .query_map(params![model], |r| Ok(PendingImage { entry_id: r.get(0)?, ordinal: r.get(1)?, local_path: r.get(2)?, sha256: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------- 均值 ----------

/// 一组向量的逐维均值；空 → None；维度不一致的跳过（按第一条的维度）。
pub fn mean_of<'a>(vectors: impl IntoIterator<Item = &'a Vec<f32>>) -> Option<Vec<f32>> {
    let mut acc: Option<Vec<f64>> = None;
    let mut n = 0usize;
    for v in vectors {
        match acc.as_mut() {
            None => {
                acc = Some(v.iter().map(|x| *x as f64).collect());
                n = 1;
            }
            Some(a) => {
                if a.len() != v.len() {
                    continue;
                }
                for (s, x) in a.iter_mut().zip(v) {
                    *s += *x as f64;
                }
                n += 1;
            }
        }
    }
    acc.map(|a| a.into_iter().map(|s| (s / n as f64) as f32).collect())
}

/// 仅画风词典的均值缓存；旧的全库均值不复用。没有 / 坏 → None。
pub fn cached_mean(conn: &Connection, model: &str) -> AppResult<Option<Vec<f32>>> {
    Ok(app_state_get(conn, &mean_key(model))?.and_then(|s| f32_from_base64(&s)).filter(|v| !v.is_empty()))
}

/// 从库里全部向量重算均值并写缓存；没有向量 → 删缓存、返回 None。
pub fn recompute_mean(conn: &Connection, model: &str) -> AppResult<Option<Vec<f32>>> {
    let all = all_vectors(conn, model)?;
    match mean_of(all.iter().map(|(_, v)| v)) {
        Some(m) => {
            app_state_set(conn, &mean_key(model), &f32_base64(&m))?;
            Ok(Some(m))
        }
        None => {
            conn.execute("DELETE FROM app_state WHERE key = ?1", params![mean_key(model)])?;
            Ok(None)
        }
    }
}

/// 缓存优先，没有就重算（索引非空时必然有值）。
pub fn ensure_mean(conn: &Connection, model: &str) -> AppResult<Option<Vec<f32>>> {
    match cached_mean(conn, model)? {
        Some(m) => Ok(Some(m)),
        None => recompute_mean(conn, model),
    }
}

/// 测试与索引共用：写一条向量并顺手让均值缓存失效（由 `recompute_mean` 在批次结束时重算）。
pub fn upsert_now(conn: &Connection, entry_id: &str, ordinal: i64, model: &str, vec: &[f32], image_sha256: Option<&str>) -> AppResult<()> {
    upsert_vector(conn, entry_id, ordinal, model, vec, image_sha256, &now_iso())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO codex_release(release_id, published_at, content_hash, fetched_at, raw_dir) VALUES ('r-1','p','h','f','d');
             INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('artist_nai5_personal','artist','画师','1','[]');
             INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
               VALUES ('a1','artist_nai5_personal',0,'a1','[]','artist:foo','safe','r-1','r-1');
             INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
               VALUES ('a2','artist_nai5_personal',1,'a2','[]','artist:bar','safe','r-1','r-1');
             INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release, removed_at)
               VALUES ('a3','artist_nai5_personal',2,'a3','[]','artist:gone','safe','r-1','r-1','2026-09-01T00:00:00Z');
             INSERT INTO entry_image(entry_id, ordinal, image_file, local_path, sha256) VALUES ('a1', 0, 'a1.png', '/img/a1.png', 'sha-a1');
             INSERT INTO entry_image(entry_id, ordinal, image_file, local_path, sha256) VALUES ('a1', 1, 'a1b.png', '/img/a1b.png', 'sha-a1b');
             INSERT INTO entry_image(entry_id, ordinal, image_file, local_path) VALUES ('a2', 0, 'a2.png', '/img/a2.png');
             INSERT INTO entry_image(entry_id, ordinal, image_file, local_path, sha256) VALUES ('a3', 0, 'a3.png', '/img/a3.png', 'sha-a3');",
        )
        .unwrap();
    }

    /// blob / base64 编码来回：f32 小端、f16 小端（精度到 1e-3）、坏长度 → None。
    #[test]
    fn blob_and_base64_round_trips() {
        let v = vec![1.0f32, -0.5, 0.25, 3.0e-3];
        let blob = vec_to_blob(&v);
        assert_eq!(blob.len(), 16);
        assert_eq!(&blob[..4], &[0, 0, 128, 63], "1.0 的小端字节");
        assert_eq!(blob_to_vec(&blob).unwrap(), v);
        assert!(blob_to_vec(&blob[..5]).is_none());
        assert_eq!(f32_from_base64(&f32_base64(&v)).unwrap(), v);
        assert!(f32_from_base64("not base64!").is_none());
        let back = f16_from_base64(&f16_base64(&v)).unwrap();
        assert_eq!(back.len(), 4);
        for (a, b) in back.iter().zip(&v) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
        assert_eq!(f16_base64(&[1.0]), "ADw=", "1.0 的 f16 小端是 00 3C");
        assert_eq!(f16_base64(&[]), "");
        assert_eq!(mean_of(Vec::<&Vec<f32>>::new().into_iter()), None);
        assert_eq!(mean_of([vec![1.0, 2.0], vec![3.0, 4.0], vec![9.0]].iter()), Some(vec![2.0, 3.0]), "维度不一致的跳过");
    }

    #[test]
    fn nax_joins_artist_index_and_invalidates_previous_artist_mean() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            seed(c);
            for (book, id, removed) in [
                ("nax_v5_artist", "n1", None),
                ("nax_v5_artist", "n_removed", Some("2026-01-01")),
                ("local_artist", "local1", None),
                ("nai5_community_pack", "pack1", None),
            ] {
                c.execute("INSERT OR IGNORE INTO codex_book(codex_id,type,title,version,tree_json) VALUES(?1,'string','SYNTHETIC ONLY','fixture','[]')", [book])?;
                c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,?2,0,'SYNTHETIC ONLY','[]','artist:synthetic_fixture','safe','r-1','r-1',?3)", params![id, book, removed])?;
                c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file,local_path) VALUES(?1,0,'fixture.png','/synthetic/fixture.png')", [id])?;
            }
            c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file,local_path) VALUES('n1',1,'fixture2.png','/synthetic/fixture2.png')", [])?;
            let model = MODEL_NAME;
            assert_eq!(count_total(c)?, 3);
            assert_eq!(pending(c, model)?.iter().map(|j| j.entry_id.as_str()).collect::<Vec<_>>(), ["a1", "a2", "n1"]);
            for (id, vector) in [("a1", [1.0, 0.0]), ("a2", [0.0, 1.0]), ("n1", [0.0, 2.0]), ("n_removed", [99.0, 99.0]), ("local1", [99.0, 99.0]), ("pack1", [99.0, 99.0])] {
                upsert_now(c, id, 0, model, &vector, if id == "a1" { Some("sha-a1") } else { None })?;
            }
            upsert_now(c, "n1", 1, model, &[99.0, 99.0], None)?;
            app_state_set(c, &format!("style_mean:artist-only-v1:{model}"), &f32_base64(&[0.5, 0.5]))?;
            assert_eq!(cached_mean(c, model)?, None, "新增来源不能复用旧词典的均值");
            assert_eq!(ensure_mean(c, model)?, Some(vec![1.0 / 3.0, 1.0]));
            assert_eq!(count_indexed(c, model)?, 3);
            assert!(pending(c, model)?.is_empty(), "原始向量仍复用，无需重算已索引图");
            assert_eq!(all_vectors(c, model)?.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["a1", "a2", "n1"]);
            assert_eq!(get_vector(c, "local1", 0, model)?, Some(vec![99.0, 99.0]), "范围外向量保留在原表");
            Ok(())
        }).unwrap();
    }

    #[test]
    fn artist_only_index_ignores_pack_vectors_and_previous_all_pool_mean() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            seed(c);
            c.execute_batch(
                "INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('nai5_community_pack','pack','图包','1','[]');
                 INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                   VALUES ('p1','nai5_community_pack',0,'p1','[]','artist:pack, sunlight','safe','r-1','r-1');
                 INSERT INTO entry_image(entry_id, ordinal, image_file, local_path, sha256) VALUES ('p1',0,'p1.png','/img/p1.png','sha-p1');",
            )?;
            let model = MODEL_NAME;
            assert_eq!(count_total(c)?, 2, "图包展示图不再纳入应算范围");
            assert_eq!(pending(c, model)?.iter().map(|j| j.entry_id.as_str()).collect::<Vec<_>>(), ["a1", "a2"]);
            upsert_now(c, "p1", 0, model, &[100.0, 100.0], Some("sha-p1"))?;
            assert_eq!(count_indexed(c, model)?, 0);
            assert!(all_vectors(c, model)?.is_empty());
            assert_eq!(ensure_mean(c, model)?, None);
            upsert_now(c, "a1", 0, model, &[1.0, 0.0], Some("sha-a1"))?;
            upsert_now(c, "a2", 0, model, &[0.0, 1.0], None)?;
            app_state_set(c, &format!("style_mean:{model}"), &f32_base64(&[33.6667, 33.6667]))?;
            assert_eq!(cached_mean(c, model)?, None, "不能复用升级前的全库均值");
            assert_eq!(ensure_mean(c, model)?, Some(vec![0.5, 0.5]));
            assert_eq!(count_indexed(c, model)?, 2);
            assert_eq!(get_vector(c, "p1", 0, model)?, Some(vec![100.0, 100.0]), "旧图包向量保留，只是不参与检索");
            Ok(())
        }).unwrap();
    }

    /// §12.1：写 / 读 / 覆盖向量；当前模型缺省名；待算清单只含 ordinal=0、已下载、未下架且缺向量或哈希不一致的；
    /// 计数；均值缓存写入 / 读回 / 重算 / 删模型后清空。
    #[test]
    fn store_round_trip_pending_and_mean() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            seed(c);
            assert_eq!(current_model(c)?, MODEL_NAME);
            app_state_set(c, STYLE_MODEL_KEY, "  ")?;
            assert_eq!(current_model(c)?, MODEL_NAME, "空白按缺省");
            app_state_set(c, STYLE_MODEL_KEY, "other-model")?;
            assert!(current_model(c).is_err(), "未知模型不能作为目录或编码器选择");
            app_state_set(c, STYLE_MODEL_KEY, "csd-vit-large-patch14")?;
            assert_eq!(current_model(c)?, "csd-vit-large-patch14");
            app_state_set(c, STYLE_MODEL_KEY, MODEL_NAME)?;
            let m = MODEL_NAME;
            assert_eq!(count_total(c)?, 2, "a1 / a2 的主图（a3 已下架、a1#1 不是主图）");
            assert_eq!(count_indexed(c, m)?, 0);
            let p = pending(c, m)?;
            assert_eq!(p.iter().map(|x| x.entry_id.as_str()).collect::<Vec<_>>(), ["a1", "a2"]);
            assert_eq!(p[0], PendingImage { entry_id: "a1".into(), ordinal: 0, local_path: "/img/a1.png".into(), sha256: Some("sha-a1".into()) });
            assert_eq!(p[1].sha256, None);

            upsert_now(c, "a1", 0, m, &[1.0, 0.0, 0.0], Some("sha-a1"))?;
            upsert_now(c, "a2", 0, m, &[0.0, 1.0, 0.0], None)?;
            upsert_now(c, "a3", 0, m, &[0.0, 0.0, 1.0], Some("sha-a3"))?;
            upsert_now(c, "a1", 0, "other-model", &[9.0, 9.0], Some("sha-a1"))?;
            assert_eq!(get_vector(c, "a1", 0, m)?, Some(vec![1.0, 0.0, 0.0]));
            assert_eq!(get_vector(c, "a1", 0, "other-model")?, Some(vec![9.0, 9.0]));
            assert_eq!(get_vector(c, "zz", 0, m)?, None);
            assert!(pending(c, m)?.is_empty(), "全算过且哈希一致（a2 两边都 NULL 也算一致）");
            assert_eq!(count_indexed(c, m)?, 2, "a3 已下架不计");
            let all = all_vectors(c, m)?;
            assert_eq!(all.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["a1", "a2"], "按 book_order，a3 不在");
            // 图片换了：entry_image.sha256 变 → 该条重新待算；覆盖后再次为空
            c.execute("UPDATE entry_image SET sha256 = 'sha-a1-new' WHERE entry_id = 'a1' AND ordinal = 0", [])?;
            assert_eq!(pending(c, m)?.len(), 1);
            upsert_now(c, "a1", 0, m, &[0.6, 0.8, 0.0], Some("sha-a1-new"))?;
            assert!(pending(c, m)?.is_empty());
            assert_eq!(get_vector(c, "a1", 0, m)?, Some(vec![0.6, 0.8, 0.0]), "同主键覆盖");
            // 后补哈希的情形：向量存的是 NULL，图片有了哈希 → 待算
            c.execute("UPDATE entry_image SET sha256 = 'sha-a2' WHERE entry_id = 'a2' AND ordinal = 0", [])?;
            assert_eq!(pending(c, m)?[0].entry_id, "a2");
            upsert_now(c, "a2", 0, m, &[0.0, 1.0, 0.0], Some("sha-a2"))?;

            // 均值
            assert_eq!(cached_mean(c, m)?, None);
            let mean = ensure_mean(c, m)?.unwrap();
            assert_eq!(mean, vec![0.3, 0.9, 0.0], "a1 与 a2 的均值（a3 已下架不计）");
            assert_eq!(cached_mean(c, m)?, Some(vec![0.3, 0.9, 0.0]));
            assert_eq!(app_state_get(c, &mean_key(m))?.as_deref(), Some(f32_base64(&[0.3, 0.9, 0.0]).as_str()));
            assert_eq!(recompute_mean(c, m)?, Some(vec![0.3, 0.9, 0.0]));
            assert_eq!(cached_mean(c, "other-model")?, None, "别的模型没有缓存");
            assert_eq!(delete_model(c, m)?, 3, "三行（含已下架的 a3）");
            assert_eq!(cached_mean(c, m)?, None, "删模型清缓存");
            assert_eq!(recompute_mean(c, m)?, None);
            assert_eq!(get_vector(c, "a1", 0, "other-model")?, Some(vec![9.0, 9.0]), "别的模型不受影响");
            assert_eq!(pending(c, m)?.len(), 2);
            Ok(())
        })
        .unwrap();
    }
}
