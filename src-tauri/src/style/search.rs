//! `style_search`：查询图 → 同一编码器算向量 → 与当前模型下画风词典的展示图向量比余弦
//! （`center=true` 时两边都先减画风词典均值）→ 降序取前 K → 拼条目信息。
//! 黑名单命中的永远不返回；每条带 decision 由界面标灰。
//! `style_prompt` 保留原始完整配方，用于复制和重画；`core` 仅保留兼容元数据，图包不参与画风检索。
//! 模型未下载 / 索引为空 → `bad_request`。查询条目本身若在索引里会以 ≈1.0 排第一（契约未说要排除，留着当健全性参照）。
//! v0.3.1（§12.7）：`ensure_ready` 单独拆出来给批次开始前 / 单条重画前查（精准反推不能白耗点数）；`search_with` 不依赖 `AppState`，
//! 重画流程（`reverse::render::resolve_style`）直接调它。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};

use crate::artist;
use crate::db::{json_str_vec, Db};
use crate::error::{AppError, AppResult};
use crate::export::hits_blacklist;
use crate::library::blacklist_set;
use crate::models::{ArtistWeighted, Decision, StyleHit, StyleSearchRequest, StyleSearchResult, StyleSearchSource};
use crate::state::AppState;
use crate::sync::upstream::library_of;

use super::{model, store, StyleState};

pub const DEFAULT_TOP_K: i64 = 10;
pub const MAX_TOP_K: i64 = 50;

/// `top_k`：≤ 0 → 缺省 10，> 50 → 50。
pub fn normalize_top_k(k: i64) -> usize {
    if k <= 0 {
        DEFAULT_TOP_K as usize
    } else {
        k.min(MAX_TOP_K) as usize
    }
}

/// 余弦相似度；维度不同或任一范数为 0 → 0。
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        dot += *x as f64 * *y as f64;
        na += *x as f64 * *x as f64;
        nb += *y as f64 * *y as f64;
    }
    if na <= 0.0 || nb <= 0.0 {
        return 0.0;
    }
    (dot / (na.sqrt() * nb.sqrt())).clamp(-1.0, 1.0)
}

/// 减均值（`center=false` 或均值维度不符时原样）。
pub fn centered(v: &[f32], mean: Option<&[f32]>) -> Vec<f32> {
    match mean {
        Some(m) if m.len() == v.len() => v.iter().zip(m).map(|(x, y)| x - y).collect(),
        _ => v.to_vec(),
    }
}

/// 打分并降序排序（同分按原顺序稳定）；维度对不上的候选得 0 分留在末尾。返回 (候选下标, 分数)。
pub fn rank(query: &[f32], mean: Option<&[f32]>, candidates: &[(String, Vec<f32>)]) -> Vec<(usize, f64)> {
    let q = centered(query, mean);
    let mut scored: Vec<(usize, f64)> = candidates.iter().enumerate().map(|(i, (_, v))| (i, cosine(&q, &centered(v, mean)))).collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// `source` → 查询图片的本地路径：题库条目要有已下载的展示图，本地路径要是文件。
pub fn resolve_source(conn: &Connection, source: &StyleSearchSource) -> AppResult<PathBuf> {
    match source {
        StyleSearchSource::Path { path } => {
            let p = PathBuf::from(path.trim());
            if path.trim().is_empty() || !p.is_file() {
                return Err(AppError::bad_request(format!("图片不存在: {path}")));
            }
            Ok(p)
        }
        StyleSearchSource::Entry { entry_id, ordinal } => {
            let row: Option<Option<String>> = conn
                .query_row("SELECT local_path FROM entry_image WHERE entry_id = ?1 AND ordinal = ?2", params![entry_id, ordinal], |r| r.get(0))
                .optional()?;
            match row {
                None => Err(AppError::not_found(format!("题目不存在: {entry_id}#{ordinal}"))),
                Some(None) => Err(AppError::bad_request(format!("题目 {entry_id}#{ordinal} 的图片尚未下载"))),
                Some(Some(p)) => {
                    let pb = PathBuf::from(&p);
                    if !pb.is_file() {
                        return Err(AppError::bad_request(format!("题目 {entry_id}#{ordinal} 的图片文件不在了: {p}")));
                    }
                    Ok(pb)
                }
            }
        }
    }
}

/// 当前模型下画风词典的候选向量（ordinal=0、未下架），按 `codex_ids` / `decisions` 筛（空 = 不再额外筛）。
pub fn candidates(conn: &Connection, model: &str, codex_ids: &[String], decisions: &[Decision]) -> AppResult<Vec<(String, Vec<f32>)>> {
    let mut sql = String::from(
        "SELECT v.entry_id, v.vec FROM image_embedding v JOIN codex_entry e ON e.entry_id = v.entry_id
         LEFT JOIN library_membership m ON m.entry_id = e.entry_id
         WHERE v.model = ? AND v.ordinal = 0 AND e.removed_at IS NULL AND e.codex_id IN ('artist_nai5_personal','nax_v5_artist')",
    );
    let mut args: Vec<Box<dyn ToSql>> = vec![Box::new(model.to_string())];
    if !codex_ids.is_empty() {
        sql.push_str(&format!(" AND e.codex_id IN ({})", vec!["?"; codex_ids.len()].join(",")));
        for c in codex_ids {
            args.push(Box::new(c.clone()));
        }
    }
    if !decisions.is_empty() {
        sql.push_str(&format!(" AND COALESCE(m.decision, 'undecided') IN ({})", vec!["?"; decisions.len()].join(",")));
        for d in decisions {
            args.push(Box::new(d.as_str().to_string()));
        }
    }
    sql.push_str(" ORDER BY e.codex_id, e.book_order");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(args.iter().map(|b| b.as_ref())), |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().filter_map(|(id, b)| store::blob_to_vec(&b).map(|v| (id, v))).collect())
}

struct EntryInfo {
    codex_id: String,
    title: String,
    path: Vec<String>,
    tags: String,
    decision: Decision,
    artists_json: Option<String>,
    thumb_path: Option<String>,
}

fn entry_info(conn: &Connection, entry_id: &str) -> AppResult<Option<EntryInfo>> {
    conn.query_row(
        "SELECT e.codex_id, e.title, e.path_json, e.tags, e.character_prompts_json, COALESCE(m.decision, 'undecided'), p.artists_json, i.thumb_path
         FROM codex_entry e
         LEFT JOIN library_membership m ON m.entry_id = e.entry_id
         LEFT JOIN artist_parse p ON p.entry_id = e.entry_id
         LEFT JOIN entry_image i ON i.entry_id = e.entry_id AND i.ordinal = 0
         WHERE e.entry_id = ?1 AND e.removed_at IS NULL",
        params![entry_id],
        |r| {
            let decision: String = r.get(5)?;
            Ok(EntryInfo {
                codex_id: r.get(0)?,
                title: r.get(1)?,
                path: json_str_vec(r.get(2)?),
                tags: r.get(3)?,
                decision: Decision::parse(&decision).unwrap_or(Decision::Undecided),
                artists_json: r.get(6)?,
                thumb_path: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// 一条候选 → `StyleHit`；图包 / 黑名单命中 / 条目不在了 / 原配方为空 → None。
fn hit_for(conn: &Connection, entry_id: &str, score: f64, blacklist: &HashSet<String>) -> AppResult<Option<StyleHit>> {
    let Some(info) = entry_info(conn, entry_id)? else { return Ok(None) };
    if library_of(&info.codex_id) != "artist" {
        return Ok(None);
    }
    let style_prompt = artist::style_prompt(&info.tags);
    if style_prompt.is_empty() {
        return Ok(None);
    }
    let prose = artist::is_prose_prompt(&info.tags);
    let path0 = info.path.first().map(|s| s.as_str());
    // 解析仅用于黑名单与兼容元数据，不能用解析结果覆盖原始配方。
    let artists: Vec<ArtistWeighted> = info
        .artists_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<ArtistWeighted>>(s).ok())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| artist::parse_entry_tags(&info.tags, path0).artists);
    if hits_blacklist(&artists, blacklist) {
        return Ok(None);
    }
    let core = artist::core_of(&artists);
    Ok(Some(StyleHit {
        entry_id: entry_id.to_string(),
        codex_id: info.codex_id,
        title: info.title,
        path: info.path,
        decision: info.decision,
        blacklisted: false,
        score,
        style_prompt,
        core: if core.is_empty() { None } else { Some(core) },
        pack_tags: None,
        thumb_path: info.thumb_path,
        prose,
    }))
}

/// 持锁部分：候选 → 排序 → 逐条拼信息（黑名单跳过）直到凑够 `top_k`。`query` 已是 L2 归一的向量。
pub fn search_with_query(conn: &Connection, query: &[f32], model: &str, req: &StyleSearchRequest) -> AppResult<Vec<StyleHit>> {
    let top_k = normalize_top_k(req.top_k);
    let cands = candidates(conn, model, &req.codex_ids, &req.decisions)?;
    if cands.is_empty() {
        return Ok(vec![]);
    }
    let mean = if req.center { store::ensure_mean(conn, model)? } else { None };
    let ranked = rank(query, mean.as_deref(), &cands);
    let blacklist = blacklist_set(conn)?;
    let mut hits = Vec::with_capacity(top_k);
    for (i, score) in ranked {
        if hits.len() >= top_k {
            break;
        }
        if let Some(h) = hit_for(conn, &cands[i].0, score, &blacklist)? {
            hits.push(h);
        }
    }
    Ok(hits)
}

/// §12.2 / §12.7：当前模型已下载且索引非空，否则 `bad_request`（文字说明去设置页下载 / 重建）。`style_search` 本身、
/// 批次开始前（`reverse::batch::prepare`）与单条重画前（`reverse::render::rerender`）都先经这里——精准反推不能画到一半才发现没索引。
/// 返回 (模型名, 已索引条数)。
pub fn ensure_ready(conn: &Connection, data_dir: &Path) -> AppResult<(String, i64)> {
    ensure_ready_for_model(conn, data_dir, None)
}

pub fn ensure_ready_for_model(conn: &Connection, data_dir: &Path, requested: Option<&str>) -> AppResult<(String, i64)> {
    let model = match requested { Some(id) => super::catalog::get(id)?.id.to_string(), None => store::current_model(conn)? };
    if !super::catalog::get(&model)?.present(&model::model_dir(data_dir, &model)) {
        return Err(AppError::bad_request(format!("画风模型 {model} 未下载，请到设置页「画风模型」下载后再试")));
    }
    let indexed = store::count_indexed(conn, &model)?;
    if indexed == 0 {
        return Err(AppError::bad_request("画风索引为空，请先同步（图片下载完成后自动建索引）或到设置页「画风模型」重建索引"));
    }
    Ok((model, indexed))
}

/// §12.2 `style_search` 的实现体（不依赖 `AppState`；§12.7 的重画流程也从这里调）。
pub async fn search_with(db: Arc<Db>, data_dir: &Path, style: &StyleState, req: StyleSearchRequest) -> AppResult<StyleSearchResult> {
    let _inference = style.inference_lock.lock().await;
    super::classifier::unload();
    let started = Instant::now();
    let join = |e: tokio::task::JoinError| AppError::new("internal", e.to_string());
    let db1 = db.clone();
    let source = req.source.clone();
    let requested_model = req.model.clone();
    let dd = data_dir.to_path_buf();
    let (model, indexed, path) = tokio::task::spawn_blocking(move || {
        db1.with(|c| {
            let path = resolve_source(c, &source)?;
            let (model, indexed) = ensure_ready_for_model(c, &dd, requested_model.as_deref())?;
            Ok((model, indexed, path))
        })
    })
    .await
    .map_err(join)??;
    let dir = model::model_dir(data_dir, &model);
    let preference = crate::state::load_settings(&db)?.style_device;
    let encoder = style.encoder(&dir, preference).await?;
    let query = tokio::task::spawn_blocking(move || -> AppResult<Vec<f32>> {
        let img = image::open(&path).map_err(|e| AppError::bad_request(format!("读取图片失败 {}: {e}", path.display())))?;
        encoder.embed_batch(&[img])?.into_iter().next().ok_or_else(|| AppError::new("model", "编码器没有输出"))
    })
    .await
    .map_err(join)??;
    let model2 = model.clone();
    let hits = tokio::task::spawn_blocking(move || db.with(|c| search_with_query(c, &query, &model2, &req))).await.map_err(join)??;
    Ok(StyleSearchResult { model, indexed, query_ms: started.elapsed().as_millis() as i64, hits })
}

/// §12.2 `style_search`。
pub async fn search(state: &AppState, req: StyleSearchRequest) -> AppResult<StyleSearchResult> {
    search_with(state.db.clone(), &state.data_dir, &state.style, req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::library;
    use crate::models::DecisionVia;

    /// 余弦 / 减均值 / 排序：去内容后本来「同向」的两条被均值分开；维度不符得 0；top_k 归一。
    #[test]
    fn cosine_ranking_with_centering_on_synthetic_vectors() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!((cosine(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-9);
        assert!((cosine(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-9);
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), 0.0, "维度不符");
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0, "零向量");
        assert_eq!(centered(&[1.0, 2.0], Some(&[0.5, 0.5])), vec![0.5, 1.5]);
        assert_eq!(centered(&[1.0, 2.0], Some(&[0.5])), vec![1.0, 2.0], "均值维度不符原样");
        assert_eq!(centered(&[1.0, 2.0], None), vec![1.0, 2.0]);
        // 库里三条：共享一个「内容」分量 c=(1,0,0)，各自的「风格」分量在后两维
        let c = [10.0f32, 0.0, 0.0];
        let mk = |s: [f32; 2]| vec![c[0], s[0], s[1]];
        let cands = vec![("a".to_string(), mk([1.0, 0.0])), ("b".to_string(), mk([0.0, 1.0])), ("d".to_string(), mk([-1.0, 0.0]))];
        let query = mk([0.9, 0.1]);
        // 不去内容：内容分量占主导，三条都 ≈ 1，几乎分不开
        let raw = rank(&query, None, &cands);
        assert_eq!(raw[0].0, 0);
        assert!(raw.iter().all(|(_, s)| *s > 0.98), "{raw:?}");
        // 去内容（均值 = (10, 0, 1/3)）：a 最像（≈0.998）、d 最不像（q'=(0,0.9,-0.23) 与 d'=(0,-1,-0.33) 的余弦 ≈ -0.84），且分数拉开
        let mean = store::mean_of(cands.iter().map(|(_, v)| v)).unwrap();
        assert_eq!(mean, vec![10.0, 0.0, 1.0 / 3.0]);
        let cen = rank(&query, Some(&mean), &cands);
        assert_eq!(cen.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(cen[0].1 > 0.99 && cen[2].1 < -0.8, "{cen:?}");
        assert!(cen[0].1 - cen[1].1 > 0.3);
        // 稳定排序：同分按原顺序
        let same = rank(&[1.0, 0.0], None, &[("x".into(), vec![1.0, 0.0]), ("y".into(), vec![2.0, 0.0])]);
        assert_eq!(same.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 1]);
        assert_eq!((normalize_top_k(0), normalize_top_k(-3), normalize_top_k(7), normalize_top_k(999)), (10, 10, 7, 50));
    }

    fn seeded() -> (Db, tempfile::TempDir) {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO codex_release(release_id, published_at, content_hash, fetched_at, raw_dir) VALUES ('r-1','p','h','f','d');
                 INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('artist_nai5_personal','artist','画师','1','[]');
                 INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('nai5_community_pack','pack','图包','1','[]');",
            )?;
            for (id, book, order, path, tags, cp) in [
                ("a_foo", "artist_nai5_personal", 0, r#"["单画师词典","A"]"#, "1.2::artist:foo::, year 2024", None),
                ("a_black", "artist_nai5_personal", 1, r#"["单画师词典","B"]"#, "artist:Lumen Brook Fieldnote", None),
                ("a_bar", "artist_nai5_personal", 2, r#"["单画师词典","C"]"#, "artist:bar, artist:baz", None),
                ("p_pack", "nai5_community_pack", 0, r#"["示例甲 · N5社区图包","常规"]"#, "artist:tessel, masterpiece, sunlight, cowboy shot", Some(r#"[{"label":"char1","prompt":"takamatsu tomori, smile"}]"#)),
                ("p_black", "nai5_community_pack", 1, r#"["示例甲 · N5社区图包","常规"]"#, "<artist>artist:lumen_brook_fieldnote</artist>\nsmile, standing", None),
                ("p_prose", "nai5_community_pack", 2, r#"["示例乙·N5图包"]"#, "<artist>\nartist:foo\n</artist>\n\n<scene>\nA girl reading under a tree.\n</scene>", None),
                ("p_removed", "nai5_community_pack", 3, "[]", "artist:x, smile", None),
            ] {
                c.execute(
                    "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, character_prompts_json, rating, first_seen_release, last_seen_release, removed_at)
                     VALUES (?1,?2,?3,?1,?4,?5,?6,'safe','r-1','r-1', CASE WHEN ?1 = 'p_removed' THEN '2026-09-01T00:00:00Z' END)",
                    params![id, book, order, path, tags, cp],
                )?;
                c.execute(
                    "INSERT INTO entry_image(entry_id, ordinal, image_file, local_path, thumb_path) VALUES (?1, 0, ?2, ?3, ?4)",
                    params![id, format!("{id}.png"), format!("/img/{id}.png"), format!("/thumbs/{id}.png.jpg")],
                )?;
            }
            // a_foo 有 artist_parse（同步时写的）；a_bar 没有 → 现场解析
            c.execute(
                "INSERT INTO artist_parse(entry_id, artists_json, auxiliary_json, negative_tail_json, signature) VALUES ('a_foo', '[{\"name\":\"foo\",\"weight\":1.2}]', '[]', '[]', 'foo@1.25')",
                [],
            )?;
            library::blacklist_add(c, "Lumen Brook Fieldnote", "试验")?;
            Ok(())
        })
        .unwrap();
        library::set_decision(&db, &["a_foo".to_string()], Decision::Include, DecisionVia::Batch).unwrap();
        library::set_decision(&db, &["a_bar".to_string()], Decision::Exclude, DecisionVia::Click).unwrap();
        (db, dir)
    }

    /// 检索只接受画风词典，黑名单/下架/高分图包均不返回；完整配方原样给出；额外 codex/decision 筛选仍有效。
    #[test]
    fn search_excludes_blacklist_and_fills_hits() {
        let (db, _dir) = seeded();
        let m = model::MODEL_NAME;
        db.with(|c| {
            // 同一「内容」分量 + 各自风格分量：查询最像 a_black（应被黑名单挡掉）、其次 p_pack、a_foo、p_prose、a_bar、p_black
            let mk = |s: [f32; 3]| vec![5.0, s[0], s[1], s[2]];
            for (id, s) in [
                ("a_foo", [0.8, 0.2, 0.0]),
                ("a_black", [1.0, 0.0, 0.0]),
                ("a_bar", [0.0, 0.0, 1.0]),
                ("p_pack", [0.9, 0.1, 0.0]),
                ("p_black", [-1.0, 0.0, 0.0]),
                ("p_prose", [0.5, 0.5, 0.0]),
                ("p_removed", [1.0, 0.0, 0.0]),
            ] {
                store::upsert_now(c, id, 0, m, &mk(s), None)?;
            }
            let req = |top_k: i64, codex_ids: &[&str], decisions: &[Decision], center: bool| StyleSearchRequest {
                source: StyleSearchSource::Path { path: "/q.png".into() },
                model: None,
                top_k,
                codex_ids: codex_ids.iter().map(|s| s.to_string()).collect(),
                decisions: decisions.to_vec(),
                center,
            };
            let query = mk([1.0, 0.0, 0.0]);
            let hits = search_with_query(c, &query, m, &req(10, &[], &[], true))?;
            let ids: Vec<&str> = hits.iter().map(|h| h.entry_id.as_str()).collect();
            assert_eq!(ids, ["a_foo", "a_bar"], "高分图包、黑名单与已下架不出现: {hits:?}");
            assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
            assert!(hits.iter().all(|h| !h.blacklisted));
            assert!(hits.iter().all(|h| h.score >= -1.0 && h.score <= 1.0));
            let foo = hits.iter().find(|h| h.entry_id == "a_foo").unwrap();
            assert_eq!((foo.codex_id.as_str(), foo.decision, foo.core.as_deref(), foo.pack_tags.as_deref(), foo.prose), ("artist_nai5_personal", Decision::Include, Some("1.2::artist:foo::"), None, false));
            assert_eq!(foo.path, ["单画师词典", "A"]);
            assert_eq!(foo.thumb_path.as_deref(), Some("/thumbs/a_foo.png.jpg"));
            assert_eq!(foo.title, "a_foo");
            assert_eq!(foo.style_prompt, "1.2::artist:foo::, year 2024", "原始配方不按 core 截断");
            let bar = hits.iter().find(|h| h.entry_id == "a_bar").unwrap();
            assert_eq!((bar.decision, bar.core.as_deref()), (Decision::Exclude, Some("artist:bar, artist:baz")), "没有 artist_parse 的现场解析");
            // top_k / 筛选
            assert_eq!(search_with_query(c, &query, m, &req(2, &[], &[], true))?.len(), 2);
            let only_pack = search_with_query(c, &query, m, &req(10, &["nai5_community_pack"], &[], true))?;
            assert!(only_pack.is_empty(), "显式选择图包也不能绕过画风词典范围");
            let only_include = search_with_query(c, &query, m, &req(10, &[], &[Decision::Include], true))?;
            assert_eq!(only_include.iter().map(|h| h.entry_id.as_str()).collect::<Vec<_>>(), ["a_foo"]);
            let ex_und = search_with_query(c, &query, m, &req(10, &["artist_nai5_personal"], &[Decision::Exclude, Decision::Undecided], true))?;
            assert_eq!(ex_und.iter().map(|h| h.entry_id.as_str()).collect::<Vec<_>>(), ["a_bar"]);
            assert!(search_with_query(c, &query, m, &req(10, &["no_such_book"], &[], true))?.is_empty());
            // center=false：内容分量占主导，全部 ≈ 1；center=true 时 a_bar 明显更低
            let raw = search_with_query(c, &query, m, &req(10, &[], &[], false))?;
            assert!(raw.iter().all(|h| h.score > 0.95), "{raw:?}");
            assert!(hits.iter().find(|h| h.entry_id == "a_bar").unwrap().score < 0.5, "{hits:?}");
            assert!(store::cached_mean(c, m)?.is_some(), "检索顺手把均值缓存建好");
            // 黑名单解除后 a_black 回到第一
            library::blacklist_remove(c, "Lumen Brook Fieldnote")?;
            let hits = search_with_query(c, &query, m, &req(3, &[], &[], true))?;
            assert_eq!(hits.iter().map(|h| h.entry_id.as_str()).collect::<Vec<_>>(), ["a_black", "a_foo", "a_bar"]);
            assert_eq!(hits[0].core.as_deref(), Some("artist:Lumen Brook Fieldnote"));
            // 别的模型没有向量 → 空
            assert!(search_with_query(c, &query, "other", &req(10, &[], &[], true))?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn nax_is_searchable_and_can_be_selected_as_a_separate_source() {
        let (db, _dir) = seeded();
        db.with(|c| {
            c.execute("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES('nax_v5_artist','string','SYNTHETIC NAX','fixture','[]')", [])?;
            for (id, tags, removed) in [("n_foo", "artist:synthetic_nax", None), ("n_black", "artist:lumen_brook_fieldnote", None), ("n_removed", "artist:synthetic_removed", Some("2026-01-01"))] {
                c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,'nax_v5_artist',0,?1,'[\"NAX V5\"]',?2,'safe','r-1','r-1',?3)", params![id, tags, removed])?;
                c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file) VALUES(?1,0,'fixture.png')", [id])?;
                store::upsert_now(c, id, 0, model::MODEL_NAME, &[1.0, 0.0], None)?;
            }
            store::upsert_now(c, "a_foo", 0, model::MODEL_NAME, &[0.0, 1.0], None)?;
            store::upsert_now(c, "p_pack", 0, model::MODEL_NAME, &[1.0, 0.0], None)?;
            let mut req = StyleSearchRequest { source: StyleSearchSource::Path { path: "/synthetic/query.png".into() }, model: None, top_k: 10, codex_ids: vec![], decisions: vec![], center: false };
            let hits = search_with_query(c, &[1.0, 0.0], model::MODEL_NAME, &req)?;
            assert_eq!(hits.iter().map(|h| h.entry_id.as_str()).collect::<Vec<_>>(), ["n_foo", "a_foo"]);
            assert_eq!(hits[0].codex_id, "nax_v5_artist");
            assert_eq!(hits[0].style_prompt, "artist:synthetic_nax");
            assert_eq!(hits[0].core.as_deref(), Some("artist:synthetic_nax"));
            assert_eq!(hits[0].pack_tags, None);
            req.codex_ids = vec!["nax_v5_artist".into()];
            assert_eq!(search_with_query(c, &[1.0, 0.0], model::MODEL_NAME, &req)?.iter().map(|h| h.entry_id.as_str()).collect::<Vec<_>>(), ["n_foo"]);
            req.decisions = vec![Decision::Include];
            assert!(search_with_query(c, &[1.0, 0.0], model::MODEL_NAME, &req)?.is_empty());
            Ok(())
        }).unwrap();
    }

    #[test]
    fn search_returns_original_complete_style_recipe() {
        let (db, _dir) = seeded();
        db.with(|c| {
            c.execute("UPDATE codex_entry SET tags = ?1 WHERE entry_id = 'a_foo'", [format!("  {}\n", artist::FULL_STYLE_RECIPE)])?;
            store::upsert_now(c, "a_foo", 0, model::MODEL_NAME, &[1.0, 0.0], None)?;
            let req = StyleSearchRequest {
                source: StyleSearchSource::Path { path: "/q.png".into() },
                model: None,
                top_k: 10, codex_ids: vec![], decisions: vec![], center: false,
            };
            let hits = search_with_query(c, &[1.0, 0.0], model::MODEL_NAME, &req)?;
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].style_prompt, artist::FULL_STYLE_RECIPE);
            assert_ne!(hits[0].core.as_deref(), Some(artist::FULL_STYLE_RECIPE), "解析元数据与完整原文是独立字段");
            assert_eq!(hits[0].pack_tags, None);
            Ok(())
        }).unwrap();
    }

    /// §12.7 `ensure_ready`：模型目录缺 → bad_request「未下载」；两个文件齐了但没向量 → bad_request「索引为空」；有一条向量 → Ok((模型, 1))；
    /// 已下架条目的向量不算。
    #[test]
    fn ensure_ready_requires_model_and_index() {
        let (db, dir) = seeded();
        db.with(|c| {
            let err = ensure_ready(c, dir.path()).unwrap_err();
            assert_eq!(err.code, "bad_request");
            assert!(err.message.contains("未下载"), "{}", err.message);
            let mdir = model::model_dir(dir.path(), model::MODEL_NAME);
            std::fs::create_dir_all(&mdir).unwrap();
            std::fs::write(mdir.join("config.json"), b"{}").unwrap();
            std::fs::write(mdir.join("model.safetensors"), b"weights").unwrap();
            let err = ensure_ready(c, dir.path()).unwrap_err();
            assert_eq!(err.code, "bad_request");
            assert!(err.message.contains("索引为空"), "{}", err.message);
            store::upsert_now(c, "p_removed", 0, model::MODEL_NAME, &[1.0, 0.0], None)?;
            assert!(ensure_ready(c, dir.path()).unwrap_err().message.contains("索引为空"), "已下架条目的向量不算");
            store::upsert_now(c, "p_pack", 0, model::MODEL_NAME, &[1.0, 0.0], None)?;
            assert!(ensure_ready(c, dir.path()).unwrap_err().message.contains("索引为空"), "仅旧图包向量不能认为画风索引就绪");
            store::upsert_now(c, "a_foo", 0, model::MODEL_NAME, &[1.0, 0.0], None)?;
            assert_eq!(ensure_ready(c, dir.path())?, (model::MODEL_NAME.to_string(), 1));
            assert!(ensure_ready_for_model(c, dir.path(), Some("csd-vit-large-patch14")).unwrap_err().message.contains("csd-vit-large-patch14"));
            crate::db::app_state_set(c, store::STYLE_MODEL_KEY, "csd-vit-large-patch14")?;
            assert_eq!(ensure_ready_for_model(c, dir.path(), Some(model::MODEL_NAME))?, (model::MODEL_NAME.to_string(), 1), "已固定的模型不跟随默认项改变");
            assert!(ensure_ready(c, dir.path()).is_err(), "新默认模型没有索引不能借用CLIP");
            Ok(())
        })
        .unwrap();
    }

    /// `resolve_source`：本地路径要是文件；题库条目要有已下载且仍在的展示图。
    #[test]
    fn resolve_source_validates() {
        let (db, dir) = seeded();
        let img = dir.path().join("q.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0])).save(&img).unwrap();
        db.with(|c| {
            assert_eq!(resolve_source(c, &StyleSearchSource::Path { path: img.to_string_lossy().to_string() })?, img);
            assert_eq!(resolve_source(c, &StyleSearchSource::Path { path: "/no/such.png".into() }).unwrap_err().code, "bad_request");
            assert_eq!(resolve_source(c, &StyleSearchSource::Path { path: "  ".into() }).unwrap_err().code, "bad_request");
            assert_eq!(resolve_source(c, &StyleSearchSource::Entry { entry_id: "zz".into(), ordinal: 0 }).unwrap_err().code, "not_found");
            assert_eq!(resolve_source(c, &StyleSearchSource::Entry { entry_id: "a_foo".into(), ordinal: 0 }).unwrap_err().code, "bad_request", "文件不在了");
            c.execute("UPDATE entry_image SET local_path = ?1 WHERE entry_id = 'a_foo'", params![img.to_string_lossy().to_string()])?;
            assert_eq!(resolve_source(c, &StyleSearchSource::Entry { entry_id: "a_foo".into(), ordinal: 0 })?, img);
            c.execute("UPDATE entry_image SET local_path = NULL WHERE entry_id = 'a_foo'", [])?;
            assert!(resolve_source(c, &StyleSearchSource::Entry { entry_id: "a_foo".into(), ordinal: 0 }).unwrap_err().message.contains("尚未下载"));
            Ok(())
        })
        .unwrap();
    }
}
