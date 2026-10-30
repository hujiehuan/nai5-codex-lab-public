//! 上游 JSON 的形状（设计 §1.1/§1.2）与条目规范化：`data-source.json` → `current.json` →
//! `manifest.json` → `codexes.json` / `media.json` / 两本书。缺 key 一律视为 null，
//! 每条必有 `id/tags/image`，书目 `type ∈ {string, pack}`，`schemaVersion === 1`，否则中止不猜（设计 §8.1）。
//! v0.5.16：所长 N5 法典（`CODEX_BOOKS`）例外——书文件不带 `type`（codexes.json 里是 `codex`），
//! 少数条目没有配图（`image: null`），条目也不写 rating（缺省按 safe）。
//! 只收 `safe` 分级的条目：任何一本书里分级不是 `safe` 的条目都在解析时丢弃，不入库、不下图。
//! 本文件不做网络与数据库，便于离线用 fixture 测试。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::CharacterPrompt;

/// 法典图鉴每次同步要拉的书。前两本是必需的（缺了整次同步失败）；`CODEX_BOOKS` 是可选的，
/// 单本失败只记一条并跳过，不连累前两本。
pub const BOOK_WHITELIST: &[&str] = &["artist_nai5_personal", "nai5_community_pack", "suozhang_nai5"];
pub const ARTIST_BOOKS: &[&str] = &["artist_nai5_personal", "local_artist", "nax_v5_artist"];
pub const PACK_BOOKS: &[&str] = &["nai5_community_pack", "local_pack"];
/// 所长 N5 法典（常规）。只在「所长法典」页浏览和标记：不进完整池推送、Studio 参考与画风索引
/// （那几处都是写死的书单，见 export::FULL_POOL_CODEXES）。
pub const CODEX_BOOKS: &[&str] = &["suozhang_nai5"];

pub fn is_artist_book(codex_id: &str) -> bool {
    ARTIST_BOOKS.contains(&codex_id)
}

pub fn is_codex_book(codex_id: &str) -> bool {
    CODEX_BOOKS.contains(&codex_id)
}

/// 决策表里的 library：法典条目和图包一样是「配方」，记在 pack 下。
pub fn library_of(codex_id: &str) -> &'static str {
    if PACK_BOOKS.contains(&codex_id) || is_codex_book(codex_id) {
        "pack"
    } else {
        "artist"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataSource {
    #[serde(rename = "schemaVersion")]
    pub schema_version: i64,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub pointer: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Current {
    pub release: String,
    pub manifest: String,
    #[serde(rename = "previousRelease", default)]
    pub previous_release: Option<String>,
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    #[serde(rename = "publishedAt")]
    pub published_at: String,
    #[serde(rename = "schemaVersion")]
    pub schema_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileMeta {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub files: BTreeMap<String, FileMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "imagePrefix")]
    pub image_prefix: String,
    #[serde(rename = "originalPrefix")]
    pub original_prefix: String,
}

impl Media {
    pub fn image_url(&self, codex_id: &str, path: &str) -> String {
        if codex_id == super::nax::BOOK_ID {
            return super::nax::image_url(path);
        }
        format!("{}/{}/{}/{}", self.base_url.trim_end_matches('/'), self.image_prefix, codex_id, path)
    }
    pub fn original_url(&self, codex_id: &str, original: &str) -> String {
        format!("{}/{}/{}/{}", self.base_url.trim_end_matches('/'), self.original_prefix, codex_id, original)
    }
}

/// 规范化后的图片项：`images[i]`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormImage {
    pub path: String,
    pub original: Option<String>,
    pub raw_tag: Option<String>,
}

/// 规范化后的条目（缺 key = None）。`Comparable` 子集用于变更判定（设计 §1.3 第 4 步）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormEntry {
    pub id: String,
    pub title: String,
    pub path: Vec<String>,
    pub tags: String,
    pub negative: Option<String>,
    pub character_prompts: Option<Vec<CharacterPrompt>>,
    pub note: Option<String>,
    pub rating: String,
    pub is_new: bool,
    pub image: String,
    pub image_w: Option<i64>,
    pub image_h: Option<i64>,
    pub original: Option<String>,
    pub images: Vec<NormImage>,
    pub asset_rev: Option<String>,
    pub update_batches: Option<Vec<String>>,
}

/// 变更比较集：`assetRev, tags, negative, characterPrompts, title, path, rating, note, images[]`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparable {
    pub asset_rev: Option<String>,
    pub tags: String,
    pub negative: Option<String>,
    pub character_prompts: Option<String>,
    pub title: String,
    pub path: Vec<String>,
    pub rating: String,
    pub note: Option<String>,
    pub images: Vec<NormImage>,
}

impl NormEntry {
    pub fn comparable(&self) -> Comparable {
        Comparable {
            asset_rev: self.asset_rev.clone(),
            tags: self.tags.clone(),
            negative: self.negative.clone(),
            character_prompts: character_prompts_json(&self.character_prompts),
            title: self.title.clone(),
            path: self.path.clone(),
            rating: self.rating.clone(),
            note: self.note.clone(),
            images: self.images.clone(),
        }
    }
}

/// characterPrompts 的规范 JSON（None / 空数组 → None），两边比较都用它。
pub fn character_prompts_json(cp: &Option<Vec<CharacterPrompt>>) -> Option<String> {
    match cp {
        Some(v) if !v.is_empty() => serde_json::to_string(v).ok(),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct UpstreamBook {
    pub id: String,
    pub r#type: String,
    pub title: String,
    pub version: String,
    pub author: Option<String>,
    pub entry_count: Option<i64>,
    pub tree: Value,
    pub update_filters: Option<Value>,
    pub entries: Vec<NormEntry>,
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
}

fn str_vec(v: Option<&Value>) -> Option<Vec<String>> {
    v.and_then(|x| x.as_array()).map(|a| {
        a.iter()
            .filter_map(|e| match e {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect()
    })
}

fn parse_character_prompts(v: Option<&Value>) -> Option<Vec<CharacterPrompt>> {
    let arr = v?.as_array()?;
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let prompt = opt_str(item, "prompt").unwrap_or_default();
        let label = opt_str(item, "label").unwrap_or_else(|| format!("char{}", i + 1));
        let negative = opt_str(item, "negative").filter(|s| !s.is_empty());
        out.push(CharacterPrompt { label, prompt, negative });
    }
    Some(out)
}

/// 条目规范化的书级规则：普通书每条必须有图；所长法典允许无图。缺 rating 一律算 safe。
#[derive(Debug, Clone, Copy)]
pub struct EntryRules {
    pub image_optional: bool,
}

impl EntryRules {
    pub const STRICT: EntryRules = EntryRules { image_optional: false };

    pub fn for_book(codex_id: &str) -> EntryRules {
        EntryRules { image_optional: is_codex_book(codex_id) }
    }
}

pub fn normalize_entry(v: &Value) -> AppResult<NormEntry> {
    normalize_entry_with(v, EntryRules::STRICT)
}

pub fn normalize_entry_with(v: &Value, rules: EntryRules) -> AppResult<NormEntry> {
    let id = opt_str(v, "id").filter(|s| !s.is_empty()).ok_or_else(|| AppError::upstream("条目缺 id"))?;
    let tags = match v.get("tags") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(AppError::upstream(format!("条目 {id} 缺 tags"))),
    };
    // 法典里没有配图的条目 image 是 null：记成空串、images 为空，库里就没有 entry_image 行（页面显示「无配图」）。
    let image = match opt_str(v, "image").filter(|s| !s.is_empty()) {
        Some(image) => image,
        None if rules.image_optional => String::new(),
        None => return Err(AppError::upstream(format!("条目 {id} 缺 image"))),
    };
    let original = opt_str(v, "original").filter(|s| !s.is_empty());
    let mut images: Vec<NormImage> = v
        .get("images")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|img| {
                    let path = opt_str(img, "path").filter(|s| !s.is_empty())?;
                    Some(NormImage {
                        path,
                        original: opt_str(img, "original").filter(|s| !s.is_empty()),
                        raw_tag: opt_str(img, "rawTag"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if images.is_empty() && !image.is_empty() {
        images.push(NormImage { path: image.clone(), original: original.clone(), raw_tag: None });
    }
    let rating = opt_str(v, "rating").unwrap_or_else(|| "safe".into());
    Ok(NormEntry {
        title: opt_str(v, "title").unwrap_or_else(|| id.clone()),
        path: str_vec(v.get("path")).unwrap_or_default(),
        tags,
        negative: opt_str(v, "negative"),
        character_prompts: parse_character_prompts(v.get("characterPrompts")),
        note: opt_str(v, "note"),
        rating,
        is_new: v.get("isNew").and_then(|x| x.as_bool()).unwrap_or(false),
        image,
        image_w: opt_i64(v, "imageWidth"),
        image_h: opt_i64(v, "imageHeight"),
        original,
        images,
        asset_rev: opt_str(v, "assetRev"),
        update_batches: str_vec(v.get("updateBatches")),
        id,
    })
}

/// 解析并校验一本书（设计 §1.3 第 3 步）。
pub fn parse_book(raw: &str) -> AppResult<UpstreamBook> {
    let v: Value = serde_json::from_str(raw).map_err(|e| AppError::upstream(format!("书 JSON 解析失败: {e}")))?;
    let id = opt_str(&v, "id").ok_or_else(|| AppError::upstream("书缺 id"))?;
    let mut r#type = opt_str(&v, "type").unwrap_or_default();
    // 所长法典的书文件不写 type（codexes.json 里是 codex），只对这几本放行；别的书仍然缺 type 就中止。
    if is_codex_book(&id) && (r#type.is_empty() || r#type == "codex") {
        r#type = "codex".into();
    } else if !["string", "pack"].contains(&r#type.as_str()) {
        return Err(AppError::upstream(format!("书 {id} type 非法: {type}")));
    }
    let entries_raw = v.get("entries").and_then(|x| x.as_array()).ok_or_else(|| AppError::upstream(format!("书 {id} 缺 entries")))?;
    let rules = EntryRules::for_book(&id);
    let mut entries = Vec::with_capacity(entries_raw.len());
    for e in entries_raw {
        // 分级过滤：非 safe 条目先于校验丢弃，不入库（`entryCount` 仍是上游原值，页面按实际条数显示）。
        if opt_str(e, "rating").is_some_and(|r| r != "safe") {
            continue;
        }
        entries.push(normalize_entry_with(e, rules)?);
    }
    Ok(UpstreamBook {
        title: opt_str(&v, "title").unwrap_or_else(|| id.clone()),
        version: opt_str(&v, "version").unwrap_or_default(),
        author: opt_str(&v, "author"),
        entry_count: opt_i64(&v, "entryCount"),
        tree: v.get("tree").cloned().unwrap_or(Value::Array(vec![])),
        update_filters: v.get("updateFilters").cloned(),
        entries,
        id,
        r#type,
    })
}

pub fn parse_data_source(raw: &str) -> AppResult<DataSource> {
    let ds: DataSource = serde_json::from_str(raw).map_err(|e| AppError::upstream(format!("data-source.json: {e}")))?;
    if ds.schema_version != 1 {
        return Err(AppError::upstream(format!("data-source schemaVersion={} 不支持", ds.schema_version)));
    }
    Ok(ds)
}

pub fn parse_current(raw: &str) -> AppResult<Current> {
    let c: Current = serde_json::from_str(raw).map_err(|e| AppError::upstream(format!("current.json: {e}")))?;
    if c.schema_version != 1 {
        return Err(AppError::upstream(format!("current schemaVersion={} 不支持", c.schema_version)));
    }
    Ok(c)
}

pub fn parse_manifest(raw: &str) -> AppResult<Manifest> {
    serde_json::from_str(raw).map_err(|e| AppError::upstream(format!("manifest.json: {e}")))
}

pub fn parse_media(raw: &str) -> AppResult<Media> {
    serde_json::from_str(raw).map_err(|e| AppError::upstream(format!("media.json: {e}")))
}

/// 校验下载文件的 sha256 与 size（设计 §1.3 第 2 步）。
pub fn verify_file(name: &str, bytes: &[u8], meta: &FileMeta) -> AppResult<()> {
    use sha2::{Digest, Sha256};
    if bytes.len() as u64 != meta.size {
        return Err(AppError::upstream(format!("{name} size 不符: 期望 {} 实际 {}", meta.size, bytes.len())));
    }
    let digest = hex::encode(Sha256::digest(bytes));
    if !digest.eq_ignore_ascii_case(&meta.sha256) {
        return Err(AppError::upstream(format!("{name} sha256 不符")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const FIXTURE_BOOK: &str = r#"{
      "id": "artist_nai5_personal", "type": "string", "title": "示例画师词典", "version": "2026.9.6",
      "author": "作者甲 / 作者乙", "entryCount": 2,
      "tree": [{"name": "单画师词典", "count": 2, "children": [{"name": "作者甲(无原图)", "count": 2, "children": []}]}],
      "updateFilters": [{"id": "2026.8.25", "label": "8.25更新"}, {"id": "2026.9.6", "label": "9.6更新", "latest": true}],
      "entries": [
        {"title": "inkwell", "path": ["单画师词典", "作者甲(无原图)", "单画师画风参考"], "tags": "artist:inkwell",
         "note": "元数据：png", "rating": "safe", "isNew": false, "id": "artist_nai5_personal_demo_0001",
         "image": "artist_nai5_personal_demo_0001.png", "imageWidth": 491, "imageHeight": 491,
         "original": "artist_nai5_personal_demo_0001.png",
         "images": [{"path": "artist_nai5_personal_demo_0001.png", "original": "artist_nai5_personal_demo_0001.png"}],
         "assetRev": "28620c7a6fe3f640", "updateBatches": ["2026.8.25"]},
        {"title": "作者丙 V5画风 001", "path": ["画风组词典", "作者丙 V5画风组合 Ver_1.0"],
         "tags": "<artist>\n0.7::artist:mo_lantern153 ::, 1::artist:hollow_(mint046)::\n</artist>,\n<style>\n2::chibi::\n</style>",
         "negative": "lowres", "characterPrompts": [{"label": "char1", "prompt": "1.2::artist:quillmark::"}, {"label": "char2", "prompt": "artist:pebble", "negative": "blurry"}],
         "rating": "safe", "isNew": true, "id": "artist_nai5_personal_combo_v1_0001",
         "image": "artist_nai5_personal_combo_v1_0001.jpg", "imageWidth": 753, "imageHeight": 1100,
         "original": "artist_nai5_personal_combo_v1_0001.png",
         "images": [{"path": "artist_nai5_personal_combo_v1_0001.jpg", "original": "artist_nai5_personal_combo_v1_0001.png", "rawTag": "solo, smile"},
                    {"path": "artist_nai5_personal_combo_v1_0001_2.jpg", "original": "artist_nai5_personal_combo_v1_0001_2.png"}],
         "assetRev": "f1a2b3c4d5e6f708", "updateBatches": ["2026.9.5", "2026.9.6"]}
      ]
    }"#;

    #[test]
    fn parses_fixture_book_and_normalizes_missing_keys() {
        let book = parse_book(FIXTURE_BOOK).unwrap();
        assert_eq!(book.id, "artist_nai5_personal");
        assert_eq!(book.entries.len(), 2);
        let e0 = &book.entries[0];
        assert_eq!(e0.negative, None);
        assert_eq!(e0.character_prompts, None);
        assert_eq!(e0.images.len(), 1);
        assert_eq!(e0.images[0].raw_tag, None);
        let e1 = &book.entries[1];
        assert_eq!(e1.images[0].raw_tag.as_deref(), Some("solo, smile"));
        assert_eq!(e1.character_prompts.as_ref().unwrap()[1].negative.as_deref(), Some("blurry"));
        assert_eq!(e1.update_batches.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn comparable_ignores_is_new_and_batches_but_sees_character_prompts() {
        let book = parse_book(FIXTURE_BOOK).unwrap();
        let mut a = book.entries[1].clone();
        let b = a.clone();
        a.is_new = false;
        a.update_batches = None;
        assert_eq!(a.comparable(), b.comparable());
        a.character_prompts = None;
        assert_ne!(a.comparable(), b.comparable());
        let mut c = book.entries[0].clone();
        c.images[0].raw_tag = Some("x".into());
        assert_ne!(c.comparable(), book.entries[0].comparable());
    }

    // 所长法典的形状（合成数据）：书不带 type，有的条目 image 为 null，条目不写 rating。
    const CODEX_BOOK: &str = r#"{
      "id": "suozhang_nai5", "title": "示例常规法典", "author": "示例编者", "version": "2026.10.27",
      "entryCount": 3, "imagedCount": 2,
      "tree": [{"name": "日常场景", "count": 3, "children": []}],
      "entries": [
        {"id": "suozhang_nai5-0001", "title": "无图条目", "path": ["日常场景"], "tags": "1girl", "isNew": false, "image": null,
         "updateBatches": ["2026.9.13"]},
        {"id": "suozhang_nai5-0002", "title": "有图条目", "path": ["日常场景"], "tags": "",
         "characterPrompts": [{"label": "char1", "prompt": "girl, smile"}], "isNew": true,
         "image": "suozhang_nai5-0002.jpg", "imageWidth": 753, "imageHeight": 1100, "original": "suozhang_nai5-0002.png",
         "assetRev": "d5aa4303188522fc", "updateBatches": ["2026.10.27"]},
        {"id": "suozhang_nai5-0003", "title": "自带分级", "path": ["日常场景"], "tags": "1boy", "rating": "restricted",
         "image": "suozhang_nai5-0003.jpg"}
      ]
    }"#;

    #[test]
    fn parses_codex_book_without_type_images_or_ratings() {
        let book = parse_book(CODEX_BOOK).unwrap();
        assert_eq!(book.r#type, "codex");
        let [no_image, imaged] = &book.entries[..] else { panic!("非 safe 条目应被丢弃，只剩 2 条") };
        assert_eq!(no_image.image, "");
        assert!(no_image.images.is_empty(), "无图条目不产生 entry_image 行");
        assert_eq!(no_image.rating, "safe", "缺 rating 按 safe");
        assert_eq!(imaged.images.len(), 1);
        assert_eq!(imaged.images[0].original.as_deref(), Some("suozhang_nai5-0002.png"));
        assert_eq!(imaged.character_prompts.as_ref().unwrap()[0].prompt, "girl, smile");
        assert_eq!(book.entry_count, Some(3), "entryCount 保留上游原值");
        assert_eq!(library_of("suozhang_nai5"), "pack");
        assert!(!is_artist_book("suozhang_nai5"));
    }

    #[test]
    fn non_safe_entries_are_dropped_in_every_book() {
        let mut v: Value = serde_json::from_str(FIXTURE_BOOK).unwrap();
        v["entries"][1]["rating"] = Value::String("restricted".into());
        let book = parse_book(&v.to_string()).unwrap();
        assert_eq!(book.entries.len(), 1);
        assert_eq!(book.entries[0].id, "artist_nai5_personal_demo_0001");
        v["entries"][0]["rating"] = Value::Null;
        assert_eq!(parse_book(&v.to_string()).unwrap().entries[0].rating, "safe", "rating: null 按 safe");
    }

    #[test]
    fn codex_leniency_does_not_leak_to_other_books() {
        // 换成别的书 id：缺 type、缺图都照旧中止
        let other = CODEX_BOOK.replace("\"suozhang_nai5\", \"title\"", "\"nai5_community_pack\", \"title\"");
        assert!(parse_book(&other).unwrap_err().message.contains("type 非法"));
        let typed = other.replacen("\"title\"", "\"type\": \"pack\", \"title\"", 1);
        assert!(parse_book(&typed).unwrap_err().message.contains("缺 image"));
        // 所长法典写了别的 type 也不放行
        let wrong = CODEX_BOOK.replacen("\"title\"", "\"type\": \"gallery\", \"title\"", 1);
        assert!(parse_book(&wrong).is_err());
    }

    #[test]
    fn rejects_bad_schema() {
        assert!(parse_book(r#"{"id":"x","type":"gallery","entries":[]}"#).is_err());
        assert!(parse_book(r#"{"id":"x","type":"pack","entries":[{"id":"a","tags":"t"}]}"#).is_err());
        assert!(parse_current(r#"{"release":"r","manifest":"m","contentHash":"h","publishedAt":"p","schemaVersion":2}"#).is_err());
        let meta = FileMeta { sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into(), size: 4 };
        assert!(verify_file("t", b"test", &meta).is_ok());
        assert!(verify_file("t", b"tes", &meta).is_err());
    }
}
