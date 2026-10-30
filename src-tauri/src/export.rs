//! 导出与推送（CONTRACT §3 / §8）：`nai5_artist_pool.json`（artist_library → 同 signature 去重 → 黑名单 → compose），
//! `pack_pool.json`（pack_library，tags 按 §3.2 清洗，只导出 safe），
//! `library_export.json`（两库全量，images[].file 为相对数据目录路径）；v0.1.4 起另有 `full_pool.json`
//! （§8.1：在线词典来源全部未删除条目，带决策 / 黑名单标记，只标不剔）。v0.2.4（§11.2）：散文条目（`artist::is_prose_prompt`）
//! 的 `pack.tags` 是剥掉 `<artist>` 块后的原文（不切分、不清洗、不剔角色，角色提示词空一行接在后面），`t` 标「散文」、
//! `pack.prose=true`、`FullEntry.prose=true`，`pack_pool.json` 里 `prose` 只在 true 时写；主标签词汇表只从非散文条目建。
//! v0.3.0（§12.4）：当前画风模型有向量时 `full_pool.json` 的 `embedding = { model, dim, centered: false, mean }`，每条 `FullEntry.vec`
//! = base64(float16 小端) 的原始向量（未减均值），没有向量的条目 `vec: null`；没有索引时 `embedding: null`、`vec` 一律 null。
//! 推送：`/api/auth/login` 换 JWT（AstrBot 前端登录前就把密码 md5 了，所以这里**永远**发 md5(钥匙串明文)，
//! 不再按「32 位 hex 即已哈希」猜测，v0.1.1 #10），再 POST `/api/plug/astrbot_plugin_vivian/page/library/import`
//! （完整池走 `/import_full`，状态查询 GET `/status`），401 重登一次，非 2xx → plugin_rejected。
//! 推送只在内存里构造 JSON，不写 `exports/`（v0.1.1 #9）。设置 / 钥匙串 / 登录 / 重登集中在
//! `plugin::plugin_conn` + `plugin::plugin_request`（v0.2.0 从本文件搬到 `plugin.rs`，实验室共用），行为不变。

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::artist;
use crate::db::{json_str_vec, now_iso, Db};
use crate::error::{AppError, AppResult};
use crate::library;
use crate::models::*;
use crate::plugin::{join_err, plugin_conn, plugin_data, plugin_request, truncate};
use crate::reverse::builtin::character_aliases;
use crate::reverse::metrics::{is_character_name, is_generic_word, is_paren_form, norm};
use crate::state::{AppState, APP_VERSION};
use crate::sync::images::{display_dest, sha256_file};
use crate::sync::upstream::library_of;

/// 按版本号语义比较（`"2026.9.6" < "2026.10.1"`，v0.1.1 #17）：去掉前导 `v`，按 `.` 切段，
/// 两边都是数字的段按数值比较，否则按字符串比较；前缀相同时段数少者在前。
pub fn cmp_version(a: &str, b: &str) -> Ordering {
    let seg = |s: &str| s.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.trim().to_string()).collect::<Vec<_>>();
    let (sa, sb) = (seg(a), seg(b));
    for (x, y) in sa.iter().zip(sb.iter()) {
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(nx), Ok(ny)) => nx.cmp(&ny),
            _ => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    sa.len().cmp(&sb.len())
}

/// `update_batches` 里语义上最新的一个批次。
pub fn latest_batch(batches: &[String]) -> Option<&String> {
    batches.iter().max_by(|a, b| cmp_version(a, b))
}

pub struct Bundle {
    pub nai5: Vec<String>,
    pub pack: Value,
    pub library: Value,
    pub pack_count: i64,
    pub library_count: i64,
    pub skipped: i64,
    pub release_id: Option<String>,
    pub exported_at: String,
}

/// 画师任一 name canonical 命中黑名单（§3.1 第 4 步剔除、§8.1 `blacklisted` 标记与 §12.2 检索排除共用同一比较）。
pub(crate) fn hits_blacklist(artists: &[ArtistWeighted], blacklist: &HashSet<String>) -> bool {
    artists.iter().any(|a| blacklist.contains(&artist::canonical_artist_tag_name(&a.name)))
}

/// rating 门槛（§3.2 / §8.1 共用）：只导出 `safe`。库里本来只有 safe（非 safe 条目在同步解析时已丢弃），这里再把一道关。
fn allowed_ratings() -> Vec<&'static str> {
    vec!["safe"]
}

/// §3.1：画师池。返回 (串列表, 跳过数)。
pub fn build_artist_pool(db: &Db) -> AppResult<(Vec<String>, i64)> {
    db.with(|c| {
        let blacklist = library::blacklist_set(c)?;
        let mut stmt = c.prepare("SELECT tags, path_json FROM artist_library ORDER BY decided_at DESC, book_order")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, json_str_vec(r.get(1)?))))?.collect::<Result<Vec<_>, _>>()?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut skipped = 0;
        for (tags, path) in rows {
            let prompt = artist::style_prompt(&tags);
            let parsed = artist::parse_entry_tags(&tags, path.first().map(String::as_str));
            if prompt.is_empty() || hits_blacklist(&parsed.artists, &blacklist) || !seen.insert(prompt.clone()) {
                skipped += 1;
                continue;
            }
            out.push(prompt);
        }
        Ok((out, skipped))
    })
}

/// `codex_entry.character_prompts_json` → 角色提示词；空 / 坏 JSON 视为没有。
fn parse_character_prompts(json: Option<&str>) -> Vec<CharacterPrompt> {
    json.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

/// §9.6 图包标签来源：`tags` 与全部 `character_prompts[].prompt` 依次拼接（主提示词在前，用 `, ` 连接，空段跳过）。
pub fn pack_source_text(tags: &str, character_prompts: &[CharacterPrompt]) -> String {
    std::iter::once(tags)
        .chain(character_prompts.iter().map(|c| c.prompt.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// §9.6 角色 token 之 (a)：每个 `character_prompts[].label` 的首个非通用 token（归一后）。
fn label_character_names(character_prompts: &[CharacterPrompt]) -> HashSet<String> {
    character_prompts
        .iter()
        .filter_map(|cp| {
            artist::parse_artist_recipe(&artist::strip_wrappers(&cp.label))
                .into_iter()
                .map(|t| norm(&t.value))
                .find(|v| !v.is_empty() && !is_generic_word(v))
        })
        .collect()
}

/// §9.6 主标签词表：`nai5_community_pack` 全部未删除条目（不看 decision / rating）的主 `tags` 列，
/// 用同一分词器切开、`norm` 去权重后的 token 集合，括号型不入表。角色提示词的首个非通用 token 不在此表里就按角色名剔
/// （上游 label 多为 `char1` 占位，`takamatsu tomori` 这类无前缀名靠它兜住；`cowboy shot` 这类真姿势词因主标签里出现过而保留）。
/// 每次导出只算一次，`build_pack` / `build_full_pool` 对每条复用。
pub fn pack_tag_vocabulary(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT tags FROM codex_entry WHERE codex_id = 'nai5_community_pack' AND removed_at IS NULL")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut vocab = HashSet::new();
    for tags in rows {
        let tags = tags?;
        // §11.2：散文条目的"词"不是标签，不进词表
        if artist::is_prose_prompt(&tags) {
            continue;
        }
        for t in artist::parse_artist_recipe(&artist::strip_wrappers(&tags)) {
            let v = norm(&t.value);
            if !v.is_empty() && !is_paren_form(&v) {
                vocab.insert(v);
            }
        }
    }
    Ok(vocab)
}

/// §9.6 角色 token 之 (d)：每个 `character_prompts[].prompt` 经 §3.2 清洗后的首个非通用 token 是「候选角色名」
/// （先清洗，免得 `masterpiece` / `artist:x` 顶掉真正的无标注角色名）；候选是括号型 / aliases 命中 / 不在主标签词表 → 剔。
/// 在词表里的（`cowboy shot`）当作真姿势 / 服装词保留；候选之后的 token 不看词表。
fn prompt_character_names(character_prompts: &[CharacterPrompt], vocab: &HashSet<String>, aliases: &HashSet<String>) -> HashSet<String> {
    character_prompts
        .iter()
        .filter_map(|cp| artist::clean_pack_tokens(&cp.prompt).iter().map(|t| norm(&t.value)).find(|v| !v.is_empty() && !is_generic_word(v)))
        .filter(|v| is_character_name(v, aliases) || !vocab.contains(v))
        .collect()
}

/// §9.6 清洗：先做 §3.2 清洗（剔 artist/quality/negative/year、`rating:*`），再剔角色 token——
/// (a) label 首个非通用 token、(b) `\(.+\)$` 括号型、(c) aliases 表命中、(d) prompt 首个非通用 token 不在主标签词表 `vocab`
/// （`pack_tag_vocabulary`）；其余按原顺序 `formatToken` 用 `, ` 连接。`source` 应来自 `pack_source_text`。
pub fn clean_pack_source(source: &str, character_prompts: &[CharacterPrompt], vocab: &HashSet<String>) -> String {
    let aliases = character_aliases();
    let mut names = label_character_names(character_prompts);
    names.extend(prompt_character_names(character_prompts, vocab, aliases));
    artist::clean_pack_tokens(source)
        .iter()
        .filter(|t| {
            let v = norm(&t.value);
            !names.contains(&v) && !is_character_name(&v, aliases)
        })
        .map(|t| artist::format_token(&t.value, t.weight))
        .collect::<Vec<_>>()
        .join(", ")
}

/// §11.2 散文条目的 `pack.tags`：主 tags 剥掉 `<artist>` 块、去首尾空白（`<style>` 块与 XML 块原样保留，不切分、不清洗、不剔角色），
/// 非空的角色提示词原文空一行接在后面。
pub fn prose_pack_tags(tags: &str, character_prompts: &[CharacterPrompt]) -> String {
    let mut parts = vec![artist::strip_artist_block(tags).trim().to_string()];
    parts.extend(character_prompts.iter().map(|c| c.prompt.as_str()).filter(|p| !p.trim().is_empty()).map(str::to_string));
    parts.join("\n\n")
}

/// 图包条目导出用的 `pack.tags`（§3.2 / §9.6 / §11.2）：散文条目 → `prose_pack_tags` 原文；否则合并来源清洗后的标签，
/// 清洗后为空 → None。`build_pack` / `build_full_pool` / `style_search` 的 `pack_tags` 都从这里出。
pub fn pack_tags_of(tags: &str, character_prompts: &[CharacterPrompt], vocab: &HashSet<String>) -> Option<String> {
    if artist::is_prose_prompt(tags) {
        Some(prose_pack_tags(tags, character_prompts))
    } else {
        let cleaned = clean_pack_source(&pack_source_text(tags, character_prompts), character_prompts, vocab);
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    }
}

/// §3.2 单条图包条目 `{ s, c, t, tags, r, prose }`；`tags` 与 `t` 都从 §9.6 的合并来源出；
/// 清洗后 tags 为空 → None（§6：调用方决定跳过还是置 null）。散文条目（§11.2）走 `prose_pack_tags`、`t` 标「散文」、永不为 None。
/// `prose` 键总是带着（`build_pack` 对非散文条目去掉它，`full_pool.json` 的 `pack` 两种都留）。
fn pack_entry(title: &str, path: &[String], tags: &str, character_prompts: &[CharacterPrompt], rating: &str, vocab: &HashSet<String>) -> Option<Value> {
    let prose = artist::is_prose_prompt(tags);
    let out_tags = pack_tags_of(tags, character_prompts, vocab)?;
    let t = if prose {
        format!("{title} · 散文")
    } else {
        let preview = artist::first_content_values(&pack_source_text(tags, character_prompts), 3);
        if preview.is_empty() {
            title.to_string()
        } else {
            format!("{} · {}", title, preview.join(", "))
        }
    };
    let s = path.first().map(|p| p.split('·').next().unwrap_or("").trim().to_string()).unwrap_or_default();
    let mut c = String::from("图包");
    for seg in path.iter().skip(1).take(2) {
        if !seg.trim().is_empty() {
            c.push('/');
            c.push_str(seg.trim());
        }
    }
    Some(json!({ "s": s, "c": c, "t": t, "tags": out_tags, "r": rating, "prose": prose }))
}

/// §3.2：图包。返回 (JSON, 条数, 跳过数)。
pub fn build_pack(conn: &Connection) -> AppResult<(Value, i64, i64)> {
    let version: String = conn
        .query_row("SELECT version FROM codex_book WHERE codex_id = 'nai5_community_pack'", [], |r| r.get(0))
        .unwrap_or_default();
    let vocab = pack_tag_vocabulary(conn)?;
    let ratings = allowed_ratings();
    let sql = format!(
        "SELECT entry_id, title, path_json, tags, character_prompts_json, rating FROM pack_library WHERE rating IN ({}) ORDER BY codex_id, book_order",
        ratings.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(ratings.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                json_str_vec(r.get(2)?),
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut entries = Vec::new();
    let mut skipped = 0i64;
    for (_entry_id, title, path, tags, cp_json, rating) in rows {
        let character_prompts = parse_character_prompts(cp_json.as_deref());
        match pack_entry(&title, &path, &tags, &character_prompts, &rating, &vocab) {
            Some(mut e) => {
                // §11.2：pack_pool.json 里 prose 只在 true 时写
                if e["prose"] != Value::Bool(true) {
                    if let Some(o) = e.as_object_mut() {
                        o.remove("prose");
                    }
                }
                entries.push(e)
            }
            None => skipped += 1,
        }
    }
    let count = entries.len() as i64;
    let out = json!({
        "source": format!("法典图鉴 N5社区图包 v{version}"),
        "note": "由 nai5-codex-lab 导出；未过插件清洗",
        "entries": entries,
    });
    Ok((out, count, skipped))
}

/// §3.3：全量库导出。
pub fn build_library_export(conn: &Connection, release_id: Option<&str>, exported_at: &str) -> AppResult<(Value, i64)> {
    let mut entries = Vec::new();
    for (view, library) in [("artist_library", "artist"), ("pack_library", "pack")] {
        let sql = format!(
            "SELECT entry_id, codex_id, title, path_json, tags, negative, character_prompts_json, rating, member_note FROM {view} ORDER BY codex_id, book_order"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    json_str_vec(r.get(3)?),
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, Option<String>>(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut img_stmt = conn.prepare("SELECT ordinal, raw_tag, image_file FROM entry_image WHERE entry_id = ?1 ORDER BY ordinal")?;
        for (entry_id, codex_id, title, path, tags, negative, cp, rating, member_note) in rows {
            let images = img_stmt
                .query_map(params![entry_id], |r| {
                    let file: String = r.get(2)?;
                    let relative = display_dest(Path::new(""), &codex_id, &file).to_string_lossy().into_owned();
                    Ok(json!({ "ordinal": r.get::<_, i64>(0)?, "raw_tag": r.get::<_, Option<String>>(1)?, "file": relative }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            entries.push(json!({
                "entry_id": entry_id, "codex_id": codex_id, "library": library, "title": title, "path": path, "tags": tags, "negative": negative,
                "character_prompts": cp.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                "rating": rating, "member_note": member_note, "images": images,
            }));
        }
    }
    let count = entries.len() as i64;
    Ok((json!({ "schema": "nai5-codex-lab/1", "exported_at": exported_at, "release_id": release_id, "entries": entries }), count))
}

pub fn build_bundle(db: &Db) -> AppResult<Bundle> {
    let exported_at = now_iso();
    let (nai5, skipped_artist) = build_artist_pool(db)?;
    let (pack, pack_count, skipped_pack, library, library_count, release_id) = db.with(|c| {
        let release_id = library::app_info_release(c)?;
        let (pack, pack_count, skipped_pack) = build_pack(c)?;
        let (library, library_count) = build_library_export(c, release_id.as_deref(), &exported_at)?;
        Ok((pack, pack_count, skipped_pack, library, library_count, release_id))
    })?;
    Ok(Bundle { nai5, pack, library, pack_count, library_count, skipped: skipped_artist + skipped_pack, release_id, exported_at })
}

fn write_json(path: &Path, v: &Value, pretty: bool) -> AppResult<()> {
    let text = if pretty { serde_json::to_string_pretty(v)? } else { serde_json::to_string(v)? };
    let tmp = PathBuf::from(format!("{}.tmp", path.to_string_lossy()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn export_dir(data_dir: &Path, options: &ExportOptions) -> PathBuf {
    match options.out_dir.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(d) => PathBuf::from(d),
        None => data_dir.join("exports").join(chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string()),
    }
}

pub fn export_libraries(db: &Db, data_dir: &Path, options: &ExportOptions) -> AppResult<(ExportResult, Bundle)> {
    let bundle = build_bundle(db)?;
    let dir = export_dir(data_dir, options);
    std::fs::create_dir_all(&dir)?;
    let p1 = dir.join("nai5_artist_pool.json");
    let p2 = dir.join("pack_pool.json");
    let p3 = dir.join("library_export.json");
    write_json(&p1, &json!(bundle.nai5), true)?;
    write_json(&p2, &bundle.pack, true)?;
    write_json(&p3, &bundle.library, false)?;
    let result = ExportResult {
        dir: dir.to_string_lossy().to_string(),
        files: vec![
            ExportFile { name: "nai5_artist_pool.json".into(), count: bundle.nai5.len() as i64, path: p1.to_string_lossy().to_string() },
            ExportFile { name: "pack_pool.json".into(), count: bundle.pack_count, path: p2.to_string_lossy().to_string() },
            ExportFile { name: "library_export.json".into(), count: bundle.library_count, path: p3.to_string_lossy().to_string() },
        ],
        skipped: bundle.skipped,
    };
    db.log_event(
        "info",
        "export",
        "导出完成",
        json!({ "dir": result.dir, "nai5": bundle.nai5.len(), "pack": bundle.pack_count, "library": bundle.library_count, "skipped": bundle.skipped }),
    );
    Ok((result, bundle))
}

// ---------- 完整池（§8.1） ----------

pub const FULL_POOL_SCHEMA: &str = "nai5-codex-lab/full/1";
/// 完整池的在线来源与顺序。`local_artist` / `local_pack` 不在其中。
pub const FULL_POOL_CODEXES: [&str; 3] = ["artist_nai5_personal", "nax_v5_artist", "nai5_community_pack"];

struct FullRow {
    entry_id: String,
    title: String,
    path: Vec<String>,
    book_order: i64,
    tags: String,
    negative: Option<String>,
    character_prompts: Option<String>,
    rating: String,
    decision: Option<String>,
    decided_at: Option<String>,
    member_note: Option<String>,
    artists_json: Option<String>,
}

/// 已下载但 `entry_image.sha256` 仍为空的行（002 迁移之前下载的）：补算并写回，返回补算条数。
/// 哈希在锁外算，只在读清单与写回时短时持锁；文件不在的跳过（记 warn），下次导出再试。
pub fn backfill_image_sha256(db: &Db) -> AppResult<usize> {
    let pending: Vec<(String, i64, String)> = db.with(|c| {
        let mut stmt = c.prepare("SELECT entry_id, ordinal, local_path FROM entry_image WHERE local_path IS NOT NULL AND sha256 IS NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    if pending.is_empty() {
        return Ok(0);
    }
    let mut hashed = Vec::with_capacity(pending.len());
    for (entry_id, ordinal, path) in pending {
        match sha256_file(Path::new(&path)) {
            Ok(h) => hashed.push((entry_id, ordinal, h)),
            Err(e) => tracing::warn!(entry_id = %entry_id, ordinal, path = %path, "sha256 补算失败，跳过: {e}"),
        }
    }
    if !hashed.is_empty() {
        db.with(|c| {
            let tx = c.transaction()?;
            for (entry_id, ordinal, h) in &hashed {
                tx.execute("UPDATE entry_image SET sha256 = ?3 WHERE entry_id = ?1 AND ordinal = ?2 AND sha256 IS NULL", params![entry_id, ordinal, h])?;
            }
            tx.commit()?;
            Ok(())
        })?;
    }
    Ok(hashed.len())
}

/// §8.1 `full_pool.json`：在线来源（固定顺序）全部未删除条目，rating 门槛同 §3.2，每条带 decision / blacklisted 标记，
/// 画师条目带 artists / core（无质量后缀），图包条目带 pack（§9.6 合并来源清洗，为空 → null），images 全部 ordinal。
/// 返回 (JSON, 按书计数, 条目总数)。
pub fn build_full_pool(db: &Db) -> AppResult<(Value, BTreeMap<String, FullCounts>, i64)> {
    let exported_at = now_iso();
    let backfilled = backfill_image_sha256(db)?;
    if backfilled > 0 {
        tracing::info!(backfilled, "entry_image.sha256 已补算");
    }
    let ratings = allowed_ratings();
    let entry_sql = format!(
        "SELECT e.entry_id, e.title, e.path_json, e.book_order, e.tags, e.negative, e.character_prompts_json, e.rating,
                m.decision, m.decided_at, m.note, p.artists_json
         FROM codex_entry e
         LEFT JOIN library_membership m ON m.entry_id = e.entry_id
         LEFT JOIN artist_parse p ON p.entry_id = e.entry_id
         WHERE e.codex_id = ?1 AND e.removed_at IS NULL AND e.rating IN ({})
         ORDER BY e.book_order",
        ratings.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );
    db.with(|c| {
        let release_id = library::app_info_release(c)?;
        let blacklist = library::blacklist_set(c)?;
        let vocab = pack_tag_vocabulary(c)?;
        // §12.4：当前画风模型有向量 → embedding（均值另给，未减均值）+ 每条 vec（f16 base64）；没有索引 → embedding null、vec 一律 null
        let style_model = crate::style::store::current_model(c)?;
        let vectors: HashMap<String, Vec<f32>> = crate::style::store::all_vectors(c, &style_model)?.into_iter().collect();
        let embedding = if vectors.is_empty() {
            Value::Null
        } else {
            let dim = vectors.values().next().map(|v| v.len()).unwrap_or(0);
            let mean = crate::style::store::ensure_mean(c, &style_model)?.unwrap_or_else(|| vec![0.0; dim]);
            json!({ "model": style_model, "dim": dim, "centered": false, "mean": crate::style::store::f16_base64(&mean) })
        };
        let mut entry_stmt = c.prepare(&entry_sql)?;
        let mut img_stmt = c.prepare("SELECT ordinal, raw_tag, image_file, local_path, sha256 FROM entry_image WHERE entry_id = ?1 ORDER BY ordinal")?;
        let mut codexes = Vec::new();
        let mut counts: BTreeMap<String, FullCounts> = BTreeMap::new();
        let mut total = 0i64;
        for codex_id in FULL_POOL_CODEXES {
            let title: Option<String> =
                c.query_row("SELECT title FROM codex_book WHERE codex_id = ?1", params![codex_id], |r| r.get(0)).optional()?;
            let Some(title) = title else { continue };
            let lib = library_of(codex_id);
            let rows = entry_stmt
                .query_map(params_from_iter(std::iter::once(codex_id).chain(ratings.iter().copied())), |r| {
                    Ok(FullRow {
                        entry_id: r.get(0)?,
                        title: r.get(1)?,
                        path: json_str_vec(r.get(2)?),
                        book_order: r.get(3)?,
                        tags: r.get(4)?,
                        negative: r.get(5)?,
                        character_prompts: r.get(6)?,
                        rating: r.get(7)?,
                        decision: r.get(8)?,
                        decided_at: r.get(9)?,
                        member_note: r.get(10)?,
                        artists_json: r.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut cnt = FullCounts::default();
            let mut entries = Vec::with_capacity(rows.len());
            for row in rows {
                let decision = row.decision.as_deref().and_then(Decision::parse).unwrap_or(Decision::Undecided);
                match decision {
                    Decision::Include => cnt.include += 1,
                    Decision::Exclude => cnt.exclude += 1,
                    Decision::Undecided => cnt.undecided += 1,
                }
                let character_prompts = parse_character_prompts(row.character_prompts.as_deref());
                // §11.2：FullEntry.prose（所有来源同一判定；画师条目实际上总是 false）
                let prose = artist::is_prose_prompt(&row.tags);
                let (artists, core, pack, blacklisted) = if lib == "artist" {
                    // 同 build_artist_pool：artist_parse 有结果就用，否则现场解析；解析不出 → null（条目仍导出）
                    let parsed = row
                        .artists_json
                        .as_deref()
                        .and_then(|s| serde_json::from_str::<Vec<ArtistWeighted>>(s).ok())
                        .filter(|a| !a.is_empty())
                        .unwrap_or_else(|| artist::parse_entry_tags(&row.tags, row.path.first().map(|s| s.as_str())).artists);
                    if parsed.is_empty() {
                        (Value::Null, Value::Null, Value::Null, false)
                    } else {
                        let bl = hits_blacklist(&parsed, &blacklist);
                        (json!(parsed), json!(artist::core_of(&parsed)), Value::Null, bl)
                    }
                } else {
                    let pack = pack_entry(&row.title, &row.path, &row.tags, &character_prompts, &row.rating, &vocab).unwrap_or(Value::Null);
                    (Value::Null, Value::Null, pack, false)
                };
                if blacklisted {
                    cnt.blacklisted += 1;
                }
                let images = img_stmt
                    .query_map(params![row.entry_id], |r| {
                        let file: String = r.get(2)?;
                        let downloaded = r.get::<_, Option<String>>(3)?.is_some();
                        let sha256: Option<String> = r.get(4)?;
                        Ok(json!({
                            "ordinal": r.get::<_, i64>(0)?,
                            "raw_tag": r.get::<_, Option<String>>(1)?,
                            "sha256": if downloaded { sha256 } else { None },
                            "file": if downloaded { Some(display_dest(Path::new(""), codex_id, &file).to_string_lossy().into_owned()) } else { None },
                        }))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                entries.push(json!({
                    "entry_id": row.entry_id, "title": row.title, "path": row.path, "book_order": row.book_order, "tags": row.tags,
                    "negative": row.negative, "character_prompts": character_prompts, "rating": row.rating,
                    "decision": decision.as_str(), "decided_at": row.decided_at, "member_note": row.member_note, "blacklisted": blacklisted,
                    "artists": artists, "core": core, "pack": pack, "images": images, "prose": prose,
                    "style_prompt": if lib == "artist" { Some(artist::style_prompt(&row.tags)) } else { None },
                    "vec": vectors.get(&row.entry_id).map(|v| crate::style::store::f16_base64(v)),
                }));
            }
            cnt.total = entries.len() as i64;
            total += cnt.total;
            counts.insert(codex_id.to_string(), cnt);
            codexes.push(json!({ "codex_id": codex_id, "title": title, "library": lib, "entries": entries }));
        }
        let out = json!({
            "schema": FULL_POOL_SCHEMA,
            "exported_at": exported_at,
            "release_id": release_id,
            "app_version": APP_VERSION,
            "counts": &counts,
            "codexes": codexes,
            "embedding": embedding,
        });
        Ok((out, counts, total))
    })
}

/// §8.2 `export_full_pool`：写 `exports/<时间戳>/full_pool.json`（或 `out_dir`），一项 `files`，`skipped=0`。
pub fn export_full_pool(db: &Db, data_dir: &Path, options: &ExportOptions) -> AppResult<ExportResult> {
    let (pool, counts, total) = build_full_pool(db)?;
    let dir = export_dir(data_dir, options);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("full_pool.json");
    write_json(&path, &pool, false)?;
    let result = ExportResult {
        dir: dir.to_string_lossy().to_string(),
        files: vec![ExportFile { name: "full_pool.json".into(), count: total, path: path.to_string_lossy().to_string() }],
        skipped: 0,
    };
    db.log_event("info", "export", "完整池导出完成", json!({ "dir": result.dir, "total": total, "counts": counts }));
    Ok(result)
}

// ---------- 推送 ----------

const PLUGIN_LIBRARY_PATH: &str = "/api/plug/astrbot_plugin_vivian/page/library";

pub async fn push_to_plugin(state: &AppState) -> AppResult<PushResult> {
    let conn = plugin_conn(state).await?;

    // 只在内存里构造三个 JSON，不再每次推送都写一个 exports/<时间戳>/ 目录（#9）
    let db = state.db.clone();
    let bundle = tokio::task::spawn_blocking(move || build_bundle(&db)).await.map_err(join_err)??;
    tracing::info!(nai5 = bundle.nai5.len(), pack = bundle.pack_count, library = bundle.library_count, skipped = bundle.skipped, "push bundle built in memory");

    let body = json!({
        "nai5_artist_pool": bundle.nai5,
        "pack_pool": bundle.pack,
        "library_export": bundle.library,
        "exported_at": bundle.exported_at,
        "release_id": bundle.release_id,
    });
    let (status, text) =
        plugin_request(&state.http, &conn, reqwest::Method::POST, &format!("{PLUGIN_LIBRARY_PATH}/import"), &[], Some(&body), Duration::from_secs(120)).await?;
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        state.db.log_event("error", "push", &format!("推送被拒 HTTP {status}"), json!({ "body": truncate(&text, 500) }));
        return Err(AppError::plugin_rejected(format!("HTTP {status}: {}", truncate(&text, 500))));
    }
    let data = plugin_data(&parsed);
    let result = PushResult {
        ok: data.get("ok").and_then(Value::as_bool).unwrap_or(parsed.get("status").and_then(Value::as_str) == Some("ok")),
        nai5: data.get("nai5").and_then(Value::as_i64).unwrap_or(bundle.nai5.len() as i64),
        pack: data.get("pack").and_then(Value::as_i64).unwrap_or(bundle.pack_count),
        dropped_blacklist: data.get("dropped_blacklist").and_then(Value::as_i64).unwrap_or(0),
        bak: data.get("bak").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
        message: parsed.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| "推送成功".into()),
        plugin_response: parsed.clone(),
    };
    state.db.log_event(
        "info",
        "push",
        "推送完成",
        json!({ "nai5": result.nai5, "pack": result.pack, "dropped_blacklist": result.dropped_blacklist, "bak": result.bak, "response": parsed }),
    );
    Ok(result)
}

/// §8.2 `push_full_to_plugin`：内存构建完整池 → POST `/page/library/import_full`（不落盘）。
/// 插件返回 `{ ok, total, counts, bak, release_id, exported_at }`，缺的字段用本地构建值兜底。
pub async fn push_full_to_plugin(state: &AppState) -> AppResult<PushFullResult> {
    let conn = plugin_conn(state).await?;
    let db = state.db.clone();
    let (pool, counts, total) = tokio::task::spawn_blocking(move || build_full_pool(&db)).await.map_err(join_err)??;
    tracing::info!(total, "full pool built in memory");

    let (status, text) = plugin_request(
        &state.http,
        &conn,
        reqwest::Method::POST,
        &format!("{PLUGIN_LIBRARY_PATH}/import_full"),
        &[],
        Some(&pool),
        Duration::from_secs(180),
    )
    .await?;
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        state.db.log_event("error", "push", &format!("完整池推送被拒 HTTP {status}"), json!({ "body": truncate(&text, 500) }));
        return Err(AppError::plugin_rejected(format!("HTTP {status}: {}", truncate(&text, 500))));
    }
    let data = plugin_data(&parsed);
    let result = PushFullResult {
        ok: data.get("ok").and_then(Value::as_bool).unwrap_or(parsed.get("status").and_then(Value::as_str) == Some("ok")),
        message: parsed
            .get("message")
            .or_else(|| data.get("message"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "完整池推送成功".into()),
        total: data.get("total").and_then(Value::as_i64).unwrap_or(total),
        counts: data.get("counts").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(counts),
        bak: data.get("bak").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
        plugin_response: parsed.clone(),
    };
    state.db.log_event(
        "info",
        "push",
        "完整池推送完成",
        json!({ "total": result.total, "counts": result.counts, "bak": result.bak, "response": parsed }),
    );
    Ok(result)
}

/// §8.2 `plugin_library_status`：GET `/page/library/status`。地址 / 密码 / token 未配置、连不上、超时、
/// 插件没有这条路由（非 401/403 的非 2xx，多半是插件未升级）→ `reachable=false` + 原因，不抛错；401/403 → `plugin_rejected`。
pub async fn plugin_library_status(state: &AppState) -> AppResult<PluginLibraryStatus> {
    fn offline(message: String) -> PluginLibraryStatus {
        PluginLibraryStatus { reachable: false, message, plugin_version: None, filtered: None, full: None }
    }
    let conn = match plugin_conn(state).await {
        Ok(c) => c,
        Err(e) if e.code == "bad_request" || e.code == "secret" => return Ok(offline(e.message)),
        Err(e) => return Err(e),
    };
    if conn.library_token.is_none() {
        return Ok(offline("钥匙串里没有 library_token".into()));
    }
    let (status, text) =
        match plugin_request(&state.http, &conn, reqwest::Method::GET, &format!("{PLUGIN_LIBRARY_PATH}/status"), &[], None, Duration::from_secs(30)).await {
            Ok(r) => r,
            Err(e) if e.code == "network" => return Ok(offline(e.message)),
            Err(e) => return Err(e),
        };
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::plugin_rejected(format!("HTTP {status}: {}", truncate(&text, 500))));
    }
    if !status.is_success() {
        return Ok(offline(format!("HTTP {status}: {}", truncate(&text, 500))));
    }
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Ok(offline(format!("响应不是 JSON: {e}"))),
    };
    let data = plugin_data(&parsed);
    let pool = |key: &str| data.get(key).filter(|v| v.is_object()).and_then(|v| serde_json::from_value::<PoolStatus>(v.clone()).ok());
    Ok(PluginLibraryStatus {
        reachable: true,
        message: parsed
            .get("message")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "已连接".into()),
        plugin_version: data.get("plugin_version").and_then(Value::as_str).map(str::to_string),
        filtered: pool("filtered"),
        full: pool("full"),
    })
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
            crate::db::app_state_set(c, "latest_release", "r-1")?;
            // 一本假的图包
            c.execute(
                "INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('nai5_community_pack','pack','图包','2026.9.1','[]')",
                [],
            )?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, negative, rating, first_seen_release, last_seen_release)
                 VALUES ('pack_1','nai5_community_pack',0,'社区精选 001','[\"示例甲 · N5社区图包\",\"社区整理\",\"常规\"]',
                   'artist:tessel,year 2025,2::chromatic_aberration::,sunlight,rating:general,masterpiece','lowres','safe','r-1','r-1')",
                [],
            )?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                 VALUES ('pack_2','nai5_community_pack',1,'样例 002','[\"示例乙·N5图包\",\"筛选整理1\"]','artist:x, best quality','unknown','r-1','r-1')",
                [],
            )?;
            c.execute("INSERT INTO entry_image(entry_id, ordinal, image_file, raw_tag) VALUES ('pack_1', 0, 'p1.jpg', 'solo, smile')", [])?;
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn artist_pool_preserves_full_recipes_and_applies_blacklist() {
        let db = seeded();
        let ids: Vec<String> = ["artist_nai5_personal_demo_0001", "artist_nai5_personal_demo_0002", "artist_nai5_personal_combo_v1_0001"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        library::set_decision(&db, &ids, Decision::Include, DecisionVia::Batch).unwrap();
        // 再造一条同 signature 的本地条目（inkwell 权重 1.1 → 量化后相同），decided_at 更新
        db.with(|c| {
            c.execute("INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('local_artist','local','本地','1','[]')", [])?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                 VALUES ('local_artist_0001','local_artist',0,'inkwell dup','[]','1.1::artist:inkwell::','safe','local','local')",
                [],
            )?;
            c.execute(
                "INSERT INTO library_membership(entry_id, library, decision, decided_at) VALUES ('local_artist_0001','artist','include','2099-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let (pool, skipped) = build_artist_pool(&db).unwrap();
        assert_eq!(pool.len(), 4, "相同画师不同权重/配方不能合并：{pool:?}");
        assert_eq!(skipped, 0);
        assert!(pool.iter().any(|s| s == "1.1::artist:inkwell::"), "无额外质量词后缀：{pool:?}");
        let original: String = db.with(|c| Ok(c.query_row("SELECT tags FROM codex_entry WHERE entry_id='artist_nai5_personal_demo_0001'", [], |r| r.get(0))?)).unwrap();
        assert!(pool.contains(&original), "词典原始画风串必须完整导出");
        db.with(|c| library::blacklist_add(c, "Lumen Brook Fieldnote", "")).unwrap();
        let (pool, skipped) = build_artist_pool(&db).unwrap();
        assert_eq!(pool.len(), 3);
        assert_eq!(skipped, 1);
        // 同一画师配方含线条、配色、负权重时，导出必须逐字保持；完全相同串只留一份。
        db.with(|c| {
            c.execute("UPDATE codex_entry SET tags=?1 WHERE entry_id IN ('artist_nai5_personal_demo_0001','local_artist_0001')", params![crate::artist::FULL_STYLE_RECIPE])?;
            Ok(())
        }).unwrap();
        let (pool, _) = build_artist_pool(&db).unwrap();
        assert_eq!(pool.iter().filter(|s| *s == crate::artist::FULL_STYLE_RECIPE).count(), 1);
        assert!(pool.iter().any(|s| s.contains("-1::muted colors::,")));
    }

    #[test]
    fn pack_export_respects_rating_and_cleans_tags() {
        let db = seeded();
        library::set_decision(&db, &["pack_1".to_string(), "pack_2".to_string()], Decision::Include, DecisionVia::Batch).unwrap();
        db.with(|c| {
            let (v, count, skipped) = build_pack(c)?;
            assert_eq!(count, 1, "pack_2 不是 safe，不进图包池");
            assert_eq!(skipped, 0);
            assert_eq!(v["source"], "法典图鉴 N5社区图包 v2026.9.1");
            let e = &v["entries"][0];
            assert_eq!(e["s"], "示例甲");
            assert_eq!(e["c"], "图包/社区整理/常规");
            assert_eq!(e["t"], "社区精选 001 · year 2025, chromatic_aberration, sunlight");
            assert_eq!(e["tags"], "2::chromatic_aberration::, sunlight");
            assert_eq!(e["r"], "safe");
            assert!(e.get("prose").is_none(), "非散文条目不写 prose 键（§11.2）");
            c.execute("UPDATE codex_entry SET rating='safe' WHERE entry_id='pack_2'", [])?;
            let (v, count, skipped) = build_pack(c)?;
            // pack_2 改成 safe 后进入候选，但清洗后 tags 为空 → 跳过
            assert_eq!(count, 1);
            assert_eq!(skipped, 1);
            assert_eq!(v["entries"].as_array().unwrap().len(), 1);
            let (lib, n) = build_library_export(c, Some("r-1"), "2026-09-06T00:00:00Z")?;
            assert_eq!(n, 2);
            assert_eq!(lib["schema"], "nai5-codex-lab/1");
            let pack1 = lib["entries"].as_array().unwrap().iter().find(|e| e["entry_id"] == "pack_1").unwrap();
            assert_eq!(pack1["library"], "pack");
            assert_eq!(pack1["images"][0]["file"], "images/nai5_community_pack/p1.jpg");
            assert_eq!(pack1["images"][0]["raw_tag"], "solo, smile");
            Ok(())
        })
        .unwrap();
    }

    /// §9.6：图包标签来源 = tags + 全部 character_prompts[].prompt 依次拼接，再 §3.2 清洗 + 剔角色 token
    /// （label 首个非通用 token、`\(.+\)$` 括号型、aliases 表命中、prompt 首个非通用 token 不在主标签词表）；`t` 也从合并来源出。
    #[test]
    fn pack_tags_merge_character_prompts_and_drop_character_tokens() {
        let vocab_of = |words: &[&str]| words.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
        let vocab = vocab_of(&["cowboy shot", "sunlight"]);
        // 空 tags、姿势服装全在角色提示词里：label 的角色名不在 prompt 中；prompt 首 token "cowboy shot" 在主标签词表里 → 姿势词，三个 token 原样导出
        let cps = vec![CharacterPrompt { label: "Vera (Sample)".into(), prompt: "cowboy shot, school uniform, smile".into(), negative: None }];
        assert_eq!(pack_source_text("", &cps), "cowboy shot, school uniform, smile");
        assert_eq!(clean_pack_source(&pack_source_text("", &cps), &cps, &vocab), "cowboy shot, school uniform, smile");
        // 同一 prompt，词表里没有 "cowboy shot" → 首个非通用 token 视为无标注角色名剔掉；后面的 token 不看词表
        assert_eq!(clean_pack_source(&pack_source_text("", &cps), &cps, &HashSet::new()), "school uniform, smile");
        let e = pack_entry("薇拉 010", &["示例甲 · N5社区图包".to_string(), "角色".to_string()], "", &cps, "safe", &vocab).unwrap();
        assert_eq!(e["tags"], "cowboy shot, school uniform, smile");
        assert_eq!(e["t"], "薇拉 010 · cowboy shot, school uniform, smile");
        assert_eq!(e["c"], "图包/角色");
        // (d) 无前缀名：label 是占位 char1，prompt 首 token "takamatsu tomori" 不在任何主标签里 → 剔
        let cps_tomori = vec![CharacterPrompt { label: "char1".into(), prompt: "takamatsu tomori, smile".into(), negative: None }];
        assert_eq!(clean_pack_source(&pack_source_text("", &cps_tomori), &cps_tomori, &vocab), "smile");
        // (d) 候选先过 §3.2：masterpiece / artist:foo 不会顶掉无前缀名
        let cps_q = vec![CharacterPrompt { label: "char1".into(), prompt: "masterpiece, artist:foo, takamatsu tomori, smile".into(), negative: None }];
        assert_eq!(clean_pack_source(&pack_source_text("", &cps_q), &cps_q, &vocab), "smile");
        // (b) 只剔 lumi：1girl 通用词跳过，候选 = lumi (sample series) 括号型；holding umbrella 不是候选，不在词表也保留
        let cps_lumi = vec![CharacterPrompt { label: "char1".into(), prompt: "1girl, lumi (sample series), holding umbrella".into(), negative: None }];
        assert_eq!(clean_pack_source(&pack_source_text("", &cps_lumi), &cps_lumi, &vocab), "1girl, holding umbrella");
        // (a) 候选在词表里但 label 点名 → 仍剔
        let cps_label = vec![CharacterPrompt { label: "Tomori".into(), prompt: "tomori, smile".into(), negative: None }];
        assert_eq!(clean_pack_source(&pack_source_text("", &cps_label), &cps_label, &vocab_of(&["tomori"])), "smile");
        // tags 与角色提示词都有：主提示词在前、各角色按顺序在后；空 prompt 跳过
        let cps2 = vec![
            CharacterPrompt { label: "1girl, Aria".into(), prompt: "1girl, aria, blue hair, 1.2::smile::".into(), negative: Some("blurry".into()) },
            CharacterPrompt { label: "char2".into(), prompt: "lumi (sample series), red dress".into(), negative: None },
            CharacterPrompt { label: "角色3".into(), prompt: "   ".into(), negative: None },
        ];
        let src = pack_source_text("masterpiece, artist:foo, 2girls, sunlight", &cps2);
        assert_eq!(src, "masterpiece, artist:foo, 2girls, sunlight, 1girl, aria, blue hair, 1.2::smile::, lumi (sample series), red dress");
        // (a) label "1girl, Aria" 首个非通用 token = aria → 剔（(d) 也会剔：不在词表）；"char2" / "角色3" 是占位，不产生角色名
        // (b) lumi (sample series) 括号型 → 剔；(c) 同时也是 aliases 表命中
        // 通用词 1girl / 2girls 不是角色 token，保留；权重照 formatToken
        assert_eq!(clean_pack_source(&src, &cps2, &vocab), "2girls, sunlight, 1girl, blue hair, 1.2::smile::, red dress");
        assert!(crate::reverse::builtin::character_aliases().contains("lumi (sample series)"), "aliases 表里有露米");
        let e2 = pack_entry("阿莉亚 020", &["示例乙·N5图包".to_string()], "masterpiece, artist:foo, 2girls, sunlight", &cps2, "safe", &vocab).unwrap();
        assert_eq!(e2["t"], "阿莉亚 020 · 2girls, sunlight, 1girl", "t 的预览也从合并来源取前 3 个非画师非质量 token");
        assert_eq!(e2["s"], "示例乙");
        // 只剩角色 token → 清洗后为空 → None
        let cps3 = vec![CharacterPrompt { label: "Aria".into(), prompt: "aria (sample series)".into(), negative: None }];
        assert!(pack_entry("只有角色 030", &[], "artist:foo", &cps3, "safe", &vocab).is_none());
        // 没有角色提示词时与旧行为一致，词表无关
        assert_eq!(pack_source_text("a, b", &[]), "a, b");
        assert_eq!(clean_pack_source("artist:x, a, year 2025, b", &[], &HashSet::new()), "a, b");

        // 走库：词表来自 nai5_community_pack 全部未删除条目的主 tags（pack_8 未决策也算；pack_11 已下架不算）；
        // pack_pool.json 与 full_pool.json 的 pack 都按此生成，且两边一致
        let db = seeded();
        db.with(|c| {
            for (id, order, title, tags, cp) in [
                ("pack_5", 4, "薇拉 010", "", r#"[{"label":"Vera (Sample)","prompt":"cowboy shot, school uniform, smile"}]"#),
                (
                    "pack_6",
                    5,
                    "阿莉亚 020",
                    "masterpiece, artist:foo, 2girls, sunlight",
                    r#"[{"label":"1girl, Aria","prompt":"1girl, aria, blue hair, 1.2::smile::","negative":"blurry"},{"label":"char2","prompt":"lumi (sample series), red dress"}]"#,
                ),
                ("pack_7", 6, "只有角色 030", "artist:foo", r#"[{"label":"Aria","prompt":"aria (sample series)"}]"#),
                ("pack_8", 7, "姿势 040", "1.1::cowboy shot::, standing", "[]"),
                ("pack_9", 8, "灯 050", "", r#"[{"label":"char1","prompt":"takamatsu tomori, smile"}]"#),
                ("pack_10", 9, "露米 060", "", r#"[{"label":"char1","prompt":"1girl, lumi (sample series), holding umbrella"}]"#),
            ] {
                c.execute(
                    "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, character_prompts_json, rating, first_seen_release, last_seen_release)
                     VALUES (?1,'nai5_community_pack',?2,?3,'[\"示例甲 · N5社区图包\",\"角色\"]',?4,?5,'safe','r-1','r-1')",
                    params![id, order, title, tags, cp],
                )?;
            }
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release, removed_at)
                 VALUES ('pack_11','nai5_community_pack',10,'已下架 070','[]','takamatsu tomori, smile','safe','r-1','r-1','2026-09-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let db_vocab = db.with(|c| pack_tag_vocabulary(c)).unwrap();
        for w in ["cowboy shot", "standing", "sunlight", "artist:tessel", "chromatic aberration"] {
            assert!(db_vocab.contains(w), "{w} 应在词表里: {db_vocab:?}");
        }
        assert!(!db_vocab.contains("takamatsu tomori"), "已下架条目的主 tags 不入词表");
        assert!(!db_vocab.contains("school uniform"), "角色提示词不入词表");
        let include_ids: Vec<String> = ["pack_1", "pack_5", "pack_6", "pack_7", "pack_9", "pack_10"].iter().map(|s| s.to_string()).collect();
        library::set_decision(&db, &include_ids, Decision::Include, DecisionVia::Batch).unwrap();
        let (pack, count, skipped) = db.with(|c| build_pack(c)).unwrap();
        assert_eq!(count, 5, "{pack}");
        assert_eq!(skipped, 1, "pack_7 清洗后只剩角色 token → 跳过并计数");
        let entries = pack["entries"].as_array().unwrap();
        assert_eq!(entries[0]["tags"], "2::chromatic_aberration::, sunlight", "无角色提示词的条目不受影响");
        assert_eq!(entries[1]["t"], "薇拉 010 · cowboy shot, school uniform, smile");
        assert_eq!(entries[1]["tags"], "cowboy shot, school uniform, smile", "cowboy shot 在 pack_8 主 tags 里 → 保留");
        assert_eq!(entries[2]["t"], "阿莉亚 020 · 2girls, sunlight, 1girl");
        assert_eq!(entries[2]["tags"], "2girls, sunlight, 1girl, blue hair, 1.2::smile::, red dress");
        assert_eq!(entries[3]["tags"], "smile", "takamatsu tomori 不在任何主 tags 里 → 剔");
        assert_eq!(entries[3]["t"], "灯 050 · takamatsu tomori, smile", "t 的预览不剔角色名（契约只说非画师非质量）");
        assert_eq!(entries[4]["tags"], "1girl, holding umbrella", "只剔括号型 lumi");
        let (pool, _, _) = build_full_pool(&db).unwrap();
        assert_eq!(ids(&pool, "nai5_community_pack"), ["pack_1", "pack_5", "pack_6", "pack_7", "pack_8", "pack_9", "pack_10"]);
        for (i, id) in [(0, "pack_1"), (1, "pack_5"), (2, "pack_6"), (3, "pack_9"), (4, "pack_10")] {
            let pack = &find(&pool, "nai5_community_pack", id)["pack"];
            assert_eq!(pack["prose"], false, "{id}：full_pool 的 pack 总带 prose 键");
            assert_eq!(without_prose(pack), entries[i], "{id}：full_pool 的 pack 与 pack_pool 条目除 prose 键外逐键相同");
        }
        let p5 = find(&pool, "nai5_community_pack", "pack_5");
        assert_eq!(p5["tags"], "", "FullEntry.tags 仍是原始 tags");
        assert_eq!(p5["character_prompts"], json!([{ "label": "Vera (Sample)", "prompt": "cowboy shot, school uniform, smile" }]));
        assert!(find(&pool, "nai5_community_pack", "pack_7")["pack"].is_null(), "清洗后为空 → pack: null，条目仍导出");
        let p8 = find(&pool, "nai5_community_pack", "pack_8");
        assert_eq!(p8["decision"], "undecided", "未决策条目不在图包池里，但主 tags 照样入词表");
        assert_eq!(p8["pack"]["tags"], "1.1::cowboy shot::, standing");
    }

    #[test]
    fn export_writes_three_files() {
        let db = seeded();
        let dir = tempfile::tempdir().unwrap();
        let (res, bundle) = export_libraries(&db, dir.path(), &ExportOptions { out_dir: None }).unwrap();
        assert_eq!(res.files.len(), 3);
        for f in &res.files {
            assert!(Path::new(&f.path).exists(), "{}", f.path);
        }
        assert!(bundle.nai5.is_empty());
        assert_eq!(bundle.release_id.as_deref(), Some("r-1"));
    }

    /// #17：批次号按版本号语义比较，而不是字符串。
    #[test]
    fn batches_compare_by_version_semantics() {
        assert_eq!(cmp_version("2026.9.6", "2026.10.1"), Ordering::Less);
        assert!("2026.9.6" > "2026.10.1", "字符串比较正好相反，这是修前的坑");
        assert_eq!(cmp_version("2026.10.1", "2026.9.6"), Ordering::Greater);
        assert_eq!(cmp_version("2026.9.6", "2026.9.6"), Ordering::Equal);
        assert_eq!(cmp_version("2026.9", "2026.9.0"), Ordering::Less);
        assert_eq!(cmp_version("v2026.9.6", "2026.9.6"), Ordering::Equal);
        assert_eq!(cmp_version("2026.9.6a", "2026.9.6b"), Ordering::Less);
        let batches: Vec<String> = ["2026.9.6", "2026.10.1", "2026.9.10", "2025.12.31"].iter().map(|s| s.to_string()).collect();
        assert_eq!(latest_batch(&batches).map(|s| s.as_str()), Some("2026.10.1"));
        assert_eq!(latest_batch(&[]), None);
    }

    // ---------- 完整池（§8） ----------

    const FAKE_IMAGE: &[u8] = b"fake image bytes";

    /// §8 测试用：在 `seeded()` 之上加非 safe / 已删除 / 本地书条目、三态决策、备注、黑名单，以及一张「已下载」的图
    /// （pack_1 ordinal 0，local_path 指向临时文件，sha256 留空让导出补算）。
    fn seeded_full() -> (Db, tempfile::TempDir) {
        let db = seeded();
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("p1.jpg");
        std::fs::write(&local, FAKE_IMAGE).unwrap();
        library::set_decision(&db, &["artist_nai5_personal_demo_0001".to_string(), "pack_1".to_string()], Decision::Include, DecisionVia::Batch).unwrap();
        library::set_decision(&db, &["artist_nai5_personal_demo_0002".to_string()], Decision::Exclude, DecisionVia::Click).unwrap();
        db.with(|c| {
            library::set_member_note(c, "artist_nai5_personal_demo_0002", "画风太糊")?;
            library::blacklist_add(c, "Lumen Brook Fieldnote", "试验")?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                 VALUES ('pack_3','nai5_community_pack',2,'未分级 003','[\"示例甲 · N5社区图包\",\"其他\"]','beach, sunset','unknown','r-1','r-1')",
                [],
            )?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release, removed_at)
                 VALUES ('pack_4','nai5_community_pack',3,'已下架 004','[\"示例甲 · N5社区图包\"]','sunset','safe','r-1','r-1','2026-09-01T00:00:00Z')",
                [],
            )?;
            c.execute("INSERT INTO codex_book(codex_id, type, title, version, tree_json) VALUES ('local_artist','local','本地','1','[]')", [])?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                 VALUES ('local_artist_0001','local_artist',0,'本地画师','[]','artist:someone','safe','local','local')",
                [],
            )?;
            c.execute(
                "UPDATE entry_image SET local_path = ?1, downloaded_at = '2026-09-06T00:00:00Z' WHERE entry_id = 'pack_1' AND ordinal = 0",
                params![local.to_string_lossy().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
        (db, dir)
    }

    fn entries_of<'a>(pool: &'a Value, codex_id: &str) -> &'a Vec<Value> {
        pool["codexes"].as_array().unwrap().iter().find(|c| c["codex_id"] == codex_id).unwrap()["entries"].as_array().unwrap()
    }

    fn find<'a>(pool: &'a Value, codex_id: &str, entry_id: &str) -> &'a Value {
        entries_of(pool, codex_id).iter().find(|e| e["entry_id"] == entry_id).unwrap_or_else(|| panic!("{entry_id} 不在完整池里"))
    }

    fn ids(pool: &Value, codex_id: &str) -> Vec<String> {
        entries_of(pool, codex_id).iter().map(|e| e["entry_id"].as_str().unwrap().to_string()).collect()
    }

    /// 去掉 `prose` 键后的拷贝（pack_pool.json 对非散文条目不写它，full_pool 的 pack 总带着）。
    fn without_prose(v: &Value) -> Value {
        let mut v = v.clone();
        if let Some(o) = v.as_object_mut() {
            o.remove("prose");
        }
        v
    }

    #[test]
    fn nax_exports_as_artist_source_in_filtered_and_full_pools() {
        let (db, dir) = seeded_full();
        let image_path = display_dest(dir.path(), "nax_v5_artist", "synthetic.webp");
        std::fs::create_dir_all(image_path.parent().unwrap()).unwrap();
        std::fs::write(&image_path, FAKE_IMAGE).unwrap();
        db.with(|c| {
            c.execute("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES('nax_v5_artist','string','SYNTHETIC NAX','fixture','[]')", [])?;
            for (order, id, rating, removed) in [
                (0, "nax_v5_artist_synthetic_include", "safe", None),
                (1, "nax_v5_artist_synthetic_exclude", "safe", None),
                (2, "nax_v5_artist_synthetic_undecided", "safe", None),
                (3, "nax_v5_artist_synthetic_removed", "safe", Some("2026-01-01")),
                (4, "nax_v5_artist_synthetic_unrated", "unknown", None),
            ] {
                c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,'nax_v5_artist',?2,?1,'[\"NAX V5\"]',?3,?4,'fixture-nax','fixture-nax',?5)", params![id, order, format!("artist:synthetic_nax_{order}"), rating, removed])?;
                c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file) VALUES(?1,0,'synthetic.webp')", [id])?;
            }
            c.execute("UPDATE entry_image SET local_path=?1 WHERE entry_id='nax_v5_artist_synthetic_include' AND ordinal=0", [image_path.to_string_lossy().to_string()])?;
            c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file) VALUES('nax_v5_artist_synthetic_include',1,'synthetic-missing.webp')", [])?;
            crate::style::store::upsert_now(c, "nax_v5_artist_synthetic_include", 0, crate::style::model::MODEL_NAME, &[0.0, 1.0], None)?;
            Ok(())
        }).unwrap();
        library::set_decision(&db, &["nax_v5_artist_synthetic_include".into()], Decision::Include, DecisionVia::Click).unwrap();
        library::set_decision(&db, &["nax_v5_artist_synthetic_exclude".into()], Decision::Exclude, DecisionVia::Click).unwrap();
        db.with(|c| library::set_member_note(c, "nax_v5_artist_synthetic_include", "合成备注🧪")).unwrap();
        let (artist_pool, _) = build_artist_pool(&db).unwrap();
        assert!(artist_pool.iter().any(|s| s == "artist:synthetic_nax_0"));
        assert!(!artist_pool.iter().any(|s| s.contains("synthetic_nax_1")));
        let bundle = build_bundle(&db).unwrap();
        let filtered = bundle.library["entries"].as_array().unwrap().iter().find(|e| e["entry_id"] == "nax_v5_artist_synthetic_include").unwrap();
        assert_eq!(filtered["codex_id"], "nax_v5_artist");
        assert_eq!(filtered["library"], "artist");
        assert_eq!(filtered["member_note"], "合成备注🧪");
        assert_eq!(filtered["images"][0]["file"], "images/nax_v5_artist/f92831932700b911f45aaa5b3a69b97af06cd999dfe2274688237a68a63b05e2.webp");
        assert_eq!(filtered["images"][1]["file"], "images/nax_v5_artist/6d0af727352a74bf8843fe0c4391b6e4f834d11c99a42dce494de456b1d46263.webp", "未下载图片也预测真实落盘路径");
        let (full, counts, total) = build_full_pool(&db).unwrap();
        assert_eq!(full["codexes"].as_array().unwrap().iter().map(|c| c["codex_id"].as_str().unwrap()).collect::<Vec<_>>(), FULL_POOL_CODEXES);
        assert_eq!(counts["nax_v5_artist"], FullCounts { total: 3, include: 1, exclude: 1, undecided: 1, blacklisted: 0 }, "已下架与非 safe 都不导出");
        assert_eq!(total, 7);
        let nax = find(&full, "nax_v5_artist", "nax_v5_artist_synthetic_include");
        assert_eq!(nax["artists"], json!([{ "name": "synthetic_nax_0", "weight": 1.0 }]));
        assert_eq!(nax["core"], "artist:synthetic_nax_0");
        assert_eq!(nax["style_prompt"], "artist:synthetic_nax_0");
        assert_eq!(nax["vec"], crate::style::store::f16_base64(&[0.0, 1.0]));
        assert_eq!(full["embedding"]["mean"], crate::style::store::f16_base64(&[0.0, 1.0]));
        assert!(nax["pack"].is_null());
        assert_eq!(nax["member_note"], "合成备注🧪");
        assert_eq!(nax["images"][0]["file"], filtered["images"][0]["file"]);
        assert_eq!(nax["images"][0]["sha256"], sha256_file(&image_path).unwrap());
        assert!(nax["images"][1]["file"].is_null(), "完整池缺图仍标 null");
        assert_eq!(find(&full, "nax_v5_artist", "nax_v5_artist_synthetic_exclude")["decision"], "exclude");
        assert_eq!(find(&full, "nax_v5_artist", "nax_v5_artist_synthetic_undecided")["decision"], "undecided");
    }

    /// §11.2：散文条目（XML / 长句）导出时 tags = 剥 `<artist>` 后原文 + 空一行接角色提示词原文，`t` 标「散文」，`prose: true`，
    /// 不算 skipped；主标签词表不收散文条目的词；full_pool 的 FullEntry.prose / pack.prose 一致，非散文条目 prose=false。
    #[test]
    fn prose_entries_export_raw_text_and_stay_out_of_vocabulary() {
        let db = seeded();
        let xml = "<artist>\nartist:foo, 0.8::artist:bar::\n</artist>\n\n<style>\nyear 2025, zzz unique token\n</style>\n\n<scene>\nA girl reading under a tree.\n</scene>";
        let long = "An anime-style illustration of a young woman with short reddish-brown hair standing amidst a swirling vortex of cherry blossoms, she wears a dark grey long-sleeved dress";
        db.with(|c| {
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, character_prompts_json, rating, first_seen_release, last_seen_release)
                 VALUES ('pack_12','nai5_community_pack',11,'散文 012','[\"示例甲 · N5社区图包\",\"XML\"]',?1,?2,'safe','r-1','r-1')",
                params![xml, r#"[{"label":"char1","prompt":"1girl, aria, smile"},{"label":"char2","prompt":"   "}]"#],
            )?;
            c.execute(
                "INSERT INTO codex_entry(entry_id, codex_id, book_order, title, path_json, tags, rating, first_seen_release, last_seen_release)
                 VALUES ('pack_13','nai5_community_pack',12,'长句 013','[\"示例乙·N5图包\"]',?1,'safe','r-1','r-1')",
                params![long],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(artist::is_prose_prompt(xml) && artist::is_prose_prompt(long));
        let vocab = db.with(|c| pack_tag_vocabulary(c)).unwrap();
        assert!(vocab.contains("sunlight"), "普通条目的词照旧入表");
        assert!(!vocab.contains("zzz unique token") && !vocab.contains("a girl reading under a tree."), "散文条目的词不入表");
        assert_eq!(prose_pack_tags(xml, &[]), "<style>\nyear 2025, zzz unique token\n</style>\n\n<scene>\nA girl reading under a tree.\n</scene>");
        library::set_decision(&db, &["pack_1".to_string(), "pack_12".to_string(), "pack_13".to_string()], Decision::Include, DecisionVia::Batch).unwrap();
        let (pack, count, skipped) = db.with(|c| build_pack(c)).unwrap();
        assert_eq!((count, skipped), (3, 0), "散文条目不算 skipped");
        let entries = pack["entries"].as_array().unwrap();
        assert!(entries[0].get("prose").is_none(), "pack_1 非散文：不写 prose 键");
        let e12 = &entries[1];
        assert_eq!(e12["prose"], true);
        assert_eq!(e12["t"], "散文 012 · 散文");
        assert_eq!(e12["tags"], "<style>\nyear 2025, zzz unique token\n</style>\n\n<scene>\nA girl reading under a tree.\n</scene>\n\n1girl, aria, smile", "剥 <artist> 块、去首尾空白、<style>/XML 原样、角色提示词原文空一行接后面、空白提示词跳过、不剔 aria");
        assert_eq!((e12["s"].as_str(), e12["c"].as_str(), e12["r"].as_str()), (Some("示例甲"), Some("图包/XML"), Some("safe")));
        let e13 = &entries[2];
        assert_eq!(e13["prose"], true);
        assert_eq!(e13["t"], "长句 013 · 散文");
        assert_eq!(e13["tags"], long);
        assert_eq!(e13["s"], "示例乙");
        let (pool, _, _) = build_full_pool(&db).unwrap();
        let p12 = find(&pool, "nai5_community_pack", "pack_12");
        assert_eq!(p12["prose"], true, "FullEntry.prose");
        assert_eq!(p12["pack"]["prose"], true);
        assert_eq!(p12["pack"]["tags"], e12["tags"]);
        assert_eq!(p12["tags"], xml, "FullEntry.tags 仍是原始 tags（含 <artist> 块）");
        assert_eq!(find(&pool, "nai5_community_pack", "pack_13")["prose"], true);
        let p1 = find(&pool, "nai5_community_pack", "pack_1");
        assert_eq!((p1["prose"].as_bool(), p1["pack"]["prose"].as_bool()), (Some(false), Some(false)));
        for e in entries_of(&pool, "artist_nai5_personal") {
            assert_eq!(e["prose"], false, "画师条目 prose=false: {}", e["entry_id"]);
        }
    }

    /// (a) rating 门槛、decision 三态、blacklisted 只标不剔、removed_at 不导出、codex 顺序固定、本地书不在其中。
    #[test]
    fn full_pool_rating_decision_blacklist_and_fixed_order() {
        let (db, _dir) = seeded_full();
        let (pool, counts, total) = build_full_pool(&db).unwrap();
        assert_eq!(pool["schema"], FULL_POOL_SCHEMA);
        assert_eq!(pool["app_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(pool["release_id"], "r-1");
        assert!(pool["embedding"].is_null(), "没有画风索引 → embedding null");
        assert!(pool["codexes"][0]["entries"][0].get("vec").is_some_and(Value::is_null), "没有索引时 vec 键在、值为 null");
        assert!(pool["exported_at"].as_str().unwrap().ends_with('Z'));
        let codex_ids: Vec<&str> = pool["codexes"].as_array().unwrap().iter().map(|c| c["codex_id"].as_str().unwrap()).collect();
        assert_eq!(codex_ids, ["artist_nai5_personal", "nai5_community_pack"], "顺序固定，尚未同步的 NAX 与本地书都不导出空壳");
        assert!(!counts.contains_key("nax_v5_artist"));
        assert_eq!(pool["codexes"][0]["library"], "artist");
        assert_eq!(pool["codexes"][0]["title"], "示例画师词典");
        assert_eq!(pool["codexes"][1]["library"], "pack");
        // safe 才导出：pack_2 / pack_3（非 safe）、pack_4（已删除）都不在
        assert_eq!(ids(&pool, "artist_nai5_personal"), ["artist_nai5_personal_demo_0001", "artist_nai5_personal_demo_0002", "artist_nai5_personal_combo_v1_0001"]);
        assert_eq!(ids(&pool, "nai5_community_pack"), ["pack_1"]);
        assert_eq!(total, 4);
        assert_eq!(counts["artist_nai5_personal"], FullCounts { total: 3, include: 1, exclude: 1, undecided: 1, blacklisted: 1 });
        assert_eq!(counts["nai5_community_pack"], FullCounts { total: 1, include: 1, exclude: 0, undecided: 0, blacklisted: 0 });
        assert_eq!(pool["counts"]["artist_nai5_personal"]["blacklisted"], 1, "JSON 里的 counts 与返回值一致");
        // decision / decided_at / member_note / blacklisted
        let a1 = find(&pool, "artist_nai5_personal", "artist_nai5_personal_demo_0001");
        assert_eq!(a1["decision"], "include");
        assert!(a1["decided_at"].as_str().is_some());
        assert!(a1["member_note"].is_null());
        assert_eq!(a1["blacklisted"], false);
        let a2 = find(&pool, "artist_nai5_personal", "artist_nai5_personal_demo_0002");
        assert_eq!(a2["decision"], "exclude");
        assert_eq!(a2["member_note"], "画风太糊");
        assert_eq!(a2["blacklisted"], true, "黑名单命中只标记，条目仍导出");
        let combo = find(&pool, "artist_nai5_personal", "artist_nai5_personal_combo_v1_0001");
        assert_eq!(combo["decision"], "undecided");
        assert!(combo["decided_at"].is_null());
        assert_eq!(combo["rating"], "safe");
        // book_order 升序
        let orders: Vec<i64> = entries_of(&pool, "artist_nai5_personal").iter().map(|e| e["book_order"].as_i64().unwrap()).collect();
        assert_eq!(orders, [0, 1, 2]);
    }

    /// (b) 画师条目 artists/core（无质量后缀）、图包条目 pack（清洗后为空 → null）、images 全部 ordinal 且未下载为 null，
    /// 已下载的给相对路径与 sha256（导出时补算并写回 entry_image.sha256）。
    #[test]
    fn full_pool_entry_shapes_artists_pack_and_images() {
        let (db, _dir) = seeded_full();
        db.with(|c| Ok(c.execute("UPDATE codex_entry SET rating='safe' WHERE entry_id='pack_2'", [])?)).unwrap();
        let (pool, _, _) = build_full_pool(&db).unwrap();
        // 画师条目：artists + core，无 QUALITY_SUFFIX；pack 为 null
        let a1 = find(&pool, "artist_nai5_personal", "artist_nai5_personal_demo_0001");
        assert_eq!(a1["artists"], json!([{ "name": "inkwell", "weight": 1.0 }]));
        assert_eq!(a1["core"], "artist:inkwell");
        assert!(a1["pack"].is_null());
        assert_eq!(a1["path"], json!(["单画师词典", "作者甲(无原图)", "单画师画风参考"]));
        assert_eq!(a1["character_prompts"], json!([]));
        assert!(a1["negative"].is_null());
        let combo = find(&pool, "artist_nai5_personal", "artist_nai5_personal_combo_v1_0001");
        assert_eq!(combo["core"], "0.7::artist:mo_lantern153::, artist:hollow_(mint046)");
        assert!(!combo["core"].as_str().unwrap().contains("amazing quality"), "core 不带质量后缀");
        assert_eq!(combo["artists"].as_array().unwrap().len(), 2);
        assert_eq!(combo["negative"], "lowres, bad quality");
        assert_eq!(combo["character_prompts"], json!([{ "label": "char1", "prompt": "1.2::artist:quillmark::" }, { "label": "char2", "prompt": "artist:pebble", "negative": "blurry" }]));
        // images：全部 ordinal；未下载 → file / sha256 都是 null，raw_tag 照旧
        assert_eq!(combo["images"].as_array().unwrap().len(), 2);
        assert_eq!(combo["images"][0]["ordinal"], 0);
        assert_eq!(combo["images"][0]["raw_tag"], "solo, smile");
        assert!(combo["images"][0]["file"].is_null());
        assert!(combo["images"][0]["sha256"].is_null());
        assert_eq!(combo["images"][1]["ordinal"], 1);
        assert_eq!(combo["images"][1]["raw_tag"], "1girl, sitting");
        // 图包条目：pack 同 §3.2 清洗；artists / core 为 null；blacklisted 恒 false
        let p1 = find(&pool, "nai5_community_pack", "pack_1");
        assert!(p1["artists"].is_null());
        assert!(p1["core"].is_null());
        assert_eq!(p1["blacklisted"], false);
        assert_eq!(p1["pack"]["s"], "示例甲");
        assert_eq!(p1["pack"]["c"], "图包/社区整理/常规");
        assert_eq!(p1["pack"]["t"], "社区精选 001 · year 2025, chromatic_aberration, sunlight");
        assert_eq!(p1["pack"]["tags"], "2::chromatic_aberration::, sunlight");
        assert_eq!(p1["pack"]["r"], "safe");
        assert_eq!(p1["tags"], "artist:tessel,year 2025,2::chromatic_aberration::,sunlight,rating:general,masterpiece", "原 tags 原样保留");
        // 已下载：file 相对数据目录，sha256 补算自本地文件
        let expected_sha = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(FAKE_IMAGE))
        };
        assert_eq!(p1["images"][0]["file"], "images/nai5_community_pack/p1.jpg");
        assert_eq!(p1["images"][0]["sha256"], expected_sha);
        let stored: Option<String> = db
            .with(|c| Ok(c.query_row("SELECT sha256 FROM entry_image WHERE entry_id = 'pack_1' AND ordinal = 0", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(stored.as_deref(), Some(expected_sha.as_str()), "补算结果写回 entry_image.sha256");
        assert_eq!(backfill_image_sha256(&db).unwrap(), 0, "第二次没有要补的");
        // 清洗后 tags 为空的图包条目：pack 为 null，但条目仍导出
        let p2 = find(&pool, "nai5_community_pack", "pack_2");
        assert!(p2["pack"].is_null());
        assert_eq!(p2["decision"], "undecided");
    }

    /// §12.4：当前模型有向量 → `embedding = { model, dim, centered: false, mean }`（mean = 库内全部向量的均值，f16 base64）、
    /// 有向量的条目 `vec` 是原始向量的 f16 base64（未减均值），没有的 null；换成没有向量的模型 → embedding 回 null、vec 全 null；
    /// 已下架条目的向量不参与均值。
    #[test]
    fn full_pool_carries_embedding_and_vec_when_index_exists() {
        use crate::style::store::{f16_from_base64, upsert_now};
        let (db, _dir) = seeded_full();
        db.with(|c| {
            upsert_now(c, "pack_1", 0, "clip-vit-base-patch32", &[1.0, 0.0, 0.5, -0.25], Some("sha"))?;
            upsert_now(c, "artist_nai5_personal_demo_0001", 0, "clip-vit-base-patch32", &[0.0, 1.0, 0.5, 0.25], None)?;
            upsert_now(c, "pack_4", 0, "clip-vit-base-patch32", &[9.0, 9.0, 9.0, 9.0], None)?; // 已下架：不参与
            Ok(())
        })
        .unwrap();
        let (pool, _, _) = build_full_pool(&db).unwrap();
        let emb = &pool["embedding"];
        assert_eq!(emb["model"], "clip-vit-base-patch32");
        assert_eq!(emb["dim"], 4);
        assert_eq!(emb["centered"], false);
        let mean = f16_from_base64(emb["mean"].as_str().unwrap()).unwrap();
        assert_eq!(mean, vec![0.0, 1.0, 0.5, 0.25], "仅画风词典参与均值");
        assert_eq!(emb.as_object().unwrap().len(), 4);
        let p1 = find(&pool, "nai5_community_pack", "pack_1");
        assert!(p1["vec"].is_null(), "旧图包向量保留在数据库但不用于画风导出");
        assert!(p1["style_prompt"].is_null());
        let a1 = find(&pool, "artist_nai5_personal", "artist_nai5_personal_demo_0001");
        assert_eq!(f16_from_base64(a1["vec"].as_str().unwrap()).unwrap(), vec![0.0, 1.0, 0.5, 0.25]);
        assert_eq!(a1["style_prompt"], a1["tags"]);
        assert!(find(&pool, "artist_nai5_personal", "artist_nai5_personal_demo_0002")["vec"].is_null(), "没有向量的条目 vec null");
        assert!(find(&pool, "artist_nai5_personal", "artist_nai5_personal_combo_v1_0001")["vec"].is_null());
        // 换模型：没有该模型的向量 → 回到 null
        db.app_state_set("style_model", "csd-vit-large-patch14").unwrap();
        let (pool, _, _) = build_full_pool(&db).unwrap();
        assert!(pool["embedding"].is_null());
        assert!(find(&pool, "nai5_community_pack", "pack_1")["vec"].is_null());
    }

    /// §8.2 `export_full_pool`：一个文件、count = 条目总数、skipped = 0，写出的 JSON 能读回。
    #[test]
    fn export_full_pool_writes_one_file() {
        let (db, _dir) = seeded_full();
        let out = tempfile::tempdir().unwrap();
        let res = export_full_pool(&db, out.path(), &ExportOptions { out_dir: None }).unwrap();
        assert_eq!(res.files.len(), 1);
        assert_eq!(res.files[0].name, "full_pool.json");
        assert_eq!(res.files[0].count, 4);
        assert_eq!(res.skipped, 0);
        assert!(Path::new(&res.dir).starts_with(out.path().join("exports")));
        let text = std::fs::read_to_string(&res.files[0].path).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["schema"], FULL_POOL_SCHEMA);
        assert_eq!(back["counts"]["nai5_community_pack"]["total"], 1);
        assert!(!Path::new(&format!("{}.tmp", res.files[0].path)).exists());
        // out_dir 指定时直接写进去
        let custom = out.path().join("custom");
        let res = export_full_pool(&db, out.path(), &ExportOptions { out_dir: Some(custom.to_string_lossy().to_string()) }).unwrap();
        assert_eq!(res.files[0].count, 4);
        assert!(custom.join("full_pool.json").exists());
    }

    /// (c) 给插件端对拍用：`FULL_POOL_SAMPLE_OUT=/path/full_pool.sample.json cargo test sample_full_pool` 会把 fixture 的
    /// 完整池写成 pretty JSON；未设环境变量时静默通过。
    #[test]
    fn sample_full_pool_writes_when_env_set() {
        let Ok(out) = std::env::var("FULL_POOL_SAMPLE_OUT") else { return };
        if out.trim().is_empty() {
            return;
        }
        let (db, _dir) = seeded_full();
        let (pool, _, total) = build_full_pool(&db).unwrap();
        let path = PathBuf::from(out);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, serde_json::to_string_pretty(&pool).unwrap()).unwrap();
        eprintln!("full_pool sample ({total} entries) → {}", path.display());
    }
}
