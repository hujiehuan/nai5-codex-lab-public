//! NAX 的公开 V5 画师实验图库适配器。只读网站实际公开的编号页与 WebP 展示图；
//! 不猜测原图地址，不执行网站 JavaScript，不使用会随投票变化的排序作身份。
//! 抓取由同步编排复用已有的限速/取消 GET，全部页校验成功后才交给 upsert。
//! 编号页不是原子快照：编排需重查首页，并对本来源禁用缺失条目的删除。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

use super::upstream::{Media, NormEntry, NormImage, UpstreamBook};

pub const BOOK_ID: &str = "nax_v5_artist";
pub const GALLERY_ID: &str = "danbooru-artist-tags-2-v5";
pub const GALLERY_URL: &str =
    "https://nax.moe/?gallery=danbooru-artist-tags-2-v5&sort=alphabetical&page=1";
pub const MEDIA_BASE_URL: &str = "https://cdn.zele.st/data/NAX";
const IMAGE_PREFIX: &str = "https://cdn.zele.st/data/NAX/Images/danbooru-artist-tags-2-v5/";
const GALLERY_TITLE: &str = "Artist Tags - Loose Prompt";
// 防止网站异常导航导致无限抓取；不是正常图库条目的裁剪阈值。
const MAX_PAGES: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GenerationSettings {
    pub model: String,
    pub steps: u32,
    pub guidance: String,
    pub quality_tags: String,
    pub uc_preset: String,
    pub seed: u64,
    pub prompt: String,
    pub character_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GalleryEntry {
    /// 保留网站标题的大小写、空格和标点；身份不受分页/排序影响。
    pub artist: String,
    /// CDN 实际使用的二次百分号编码文件名；只有单个安全路径段。
    pub image: String,
    pub is_new: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryPage {
    pub entries: Vec<GalleryEntry>,
    /// 只有完整首页包含导航；部分页为空。
    pub page_numbers: Vec<usize>,
    /// 只有完整首页包含生成设置；部分页为空。
    pub generation: Option<GenerationSettings>,
}

#[derive(Debug, Clone)]
pub struct NaxSnapshot {
    pub book: UpstreamBook,
    /// 只包含画师、图片路径和生成设置；投票不会制造条目历史或重复下载。
    pub content_hash: String,
    /// 用于本机来源快照；不是网站 HTML，也不包含投票/用户资料。
    pub raw: String,
}

pub fn media() -> Media {
    Media {
        base_url: MEDIA_BASE_URL.into(),
        image_prefix: "Images".into(),
        original_prefix: String::new(),
    }
}

/// Media 的 NAX 分支使用图库 slug，而本机 codex_id 使用稳定的 BOOK_ID。
/// path 来自已验证的 GalleryEntry，不做 decode，避免破坏 CDN 的双重编码。
pub fn image_url(path: &str) -> String {
    format!("{IMAGE_PREFIX}{path}")
}

pub fn entry_id(artist: &str) -> String {
    format!(
        "{BOOK_ID}_{}",
        hex::encode(Sha256::digest(artist.as_bytes()))
    )
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("constant NAX regex"))
}

static ATTR: OnceLock<Regex> = OnceLock::new();
static TAG: OnceLock<Regex> = OnceLock::new();
static FIGURE: OnceLock<Regex> = OnceLock::new();
static FIGURE_START: OnceLock<Regex> = OnceLock::new();
static SECTION: OnceLock<Regex> = OnceLock::new();
static SECTION_END: OnceLock<Regex> = OnceLock::new();
static DIV: OnceLock<Regex> = OnceLock::new();
static IMG: OnceLock<Regex> = OnceLock::new();
static CAPTION: OnceLock<Regex> = OnceLock::new();
static ANCHOR: OnceLock<Regex> = OnceLock::new();
static PARAGRAPH: OnceLock<Regex> = OnceLock::new();
static SETTINGS: OnceLock<Regex> = OnceLock::new();
static SPAN: OnceLock<Regex> = OnceLock::new();

fn decode_html(raw: &str) -> AppResult<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        // 普通标签名中的未转义的 & 不是 HTML entity；已编码的 entity 必须认识。
        let Some(end) = rest
            .find(';')
            .filter(|i| *i <= 16 && !rest[..*i].contains([' ', '<', '&']))
        else {
            out.push('&');
            continue;
        };
        let entity = &rest[..end];
        let c = match entity {
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            "lt" => '<',
            "gt" => '>',
            "nbsp" => '\u{a0}',
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                u32::from_str_radix(&entity[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| AppError::upstream("NAX HTML 十六进制 entity 非法"))?
            }
            _ if entity.starts_with('#') => entity[1..]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| AppError::upstream("NAX HTML 数字 entity 非法"))?,
            _ => {
                return Err(AppError::upstream(format!(
                    "NAX HTML entity 不支持: &{entity};"
                )))
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn attrs(raw: &str) -> AppResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for c in regex(
        &ATTR,
        r#"(?is)([a-zA-Z][a-zA-Z0-9_-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#,
    )
    .captures_iter(raw)
    {
        let key = c[1].to_ascii_lowercase();
        let value = decode_html(c.get(2).or_else(|| c.get(3)).unwrap().as_str())?;
        if values.insert(key.clone(), value).is_some() {
            return Err(AppError::upstream(format!("NAX 重复 HTML 属性: {key}")));
        }
    }
    Ok(values)
}

fn has_class(values: &BTreeMap<String, String>, class: &str) -> bool {
    values
        .get("class")
        .is_some_and(|s| s.split_ascii_whitespace().any(|part| part == class))
}

fn text_of(raw: &str) -> AppResult<String> {
    let text = regex(&TAG, r"(?s)<[^>]*>").replace_all(raw, "");
    decode_html(&text)
}

fn encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn validate_filename(filename: &str) -> AppResult<()> {
    if !filename.ends_with(".webp") || filename == ".webp" || filename.contains(['/', '\\']) {
        return Err(AppError::upstream("NAX 展示图文件名非法"));
    }
    // data-image 是 rawurlencode 后的单段文件名；不接受 query、控制字符和坏编码。
    let bytes = filename.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return Err(AppError::upstream("NAX 展示图百分号编码非法"));
            }
            let escaped = u8::from_str_radix(&filename[i + 1..i + 3], 16).unwrap();
            if matches!(escaped, 0..=31 | 127 | b'/' | b'\\') {
                return Err(AppError::upstream("NAX 展示图文件名含路径或控制字符"));
            }
            i += 3;
        } else if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            i += 1;
        } else {
            return Err(AppError::upstream("NAX 展示图文件名不是编码后的安全路径段"));
        }
    }
    Ok(())
}

fn generation_settings(raw: &str) -> AppResult<Option<GenerationSettings>> {
    let mut codes = Vec::new();
    let mut summary = None;
    for c in regex(&PARAGRAPH, r"(?is)<p\b([^>]*)>(.*?)</p\s*>").captures_iter(raw) {
        let a = attrs(&c[1])?;
        let text = text_of(&c[2])?;
        if has_class(&a, "code") {
            codes.push(text.trim().to_string());
        }
        if text.trim_start().starts_with("Model used is ") {
            if summary.is_some() {
                return Err(AppError::upstream("NAX 首页含重复生成设置"));
            }
            summary = Some(text.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    let Some(summary) = summary else {
        return Ok(None);
    };
    let c = regex(&SETTINGS, r"^Model used is (.+), Steps ([0-9]+), Guidance ([0-9]+(?:\.[0-9]+)?), Quality Tags (Enabled|Disabled), UC Preset ([^,]+), Seed ([0-9]+)\.$")
        .captures(&summary).ok_or_else(|| AppError::upstream("NAX 生成设置格式已变化"))?;
    if &c[1] != "NAI Diffusion V5 Full"
        || codes.len() != 2
        || !codes[0].contains("artist:______")
        || codes[1].is_empty()
    {
        return Err(AppError::upstream(
            "NAX 首页不是预期 V5 Full 画师实验或缺完整提示词",
        ));
    }
    Ok(Some(GenerationSettings {
        model: c[1].to_string(),
        steps: c[2]
            .parse()
            .map_err(|_| AppError::upstream("NAX steps 非法"))?,
        guidance: c[3].to_string(),
        quality_tags: c[4].to_string(),
        uc_preset: c[5].to_string(),
        seed: c[6]
            .parse()
            .map_err(|_| AppError::upstream("NAX seed 非法"))?,
        prompt: codes.remove(0),
        character_prompt: codes.remove(0),
    }))
}

/// 严格解析完整首页或网站自身使用的 gallery-display.php 部分页。
/// 不完整 figure、缺字段、错误图库、错误 CDN 路径均报错，不静默跳过条目。
pub fn parse_gallery_page(raw: &str) -> AppResult<GalleryPage> {
    let generation = generation_settings(raw)?;
    let mut page_numbers = BTreeSet::new();
    for c in regex(&ANCHOR, r"(?is)<a\b([^>]*)>").captures_iter(raw) {
        let a = attrs(&c[1])?;
        if let Some(page) = a.get("data-page") {
            let n = page
                .parse::<i64>()
                .map_err(|_| AppError::upstream("NAX 页码非法"))?;
            if n > 0 {
                let href = a
                    .get("href")
                    .ok_or_else(|| AppError::upstream("NAX 页码缺链接"))?;
                let parsed = reqwest::Url::parse(&format!("https://nax.moe/{href}"))
                    .map_err(|_| AppError::upstream("NAX 页码链接非法"))?;
                if !parsed
                    .query_pairs()
                    .any(|(k, v)| k == "gallery" && v == GALLERY_ID)
                    || !parsed
                        .query_pairs()
                        .any(|(k, v)| k == "page" && v == n.to_string())
                {
                    return Err(AppError::upstream("NAX 页码链接不是目标图库"));
                }
                page_numbers.insert(n as usize);
            }
        }
    }
    let page_numbers: Vec<usize> = page_numbers.into_iter().collect();
    if generation.is_some() {
        if page_numbers.is_empty()
            || page_numbers.len() > MAX_PAGES
            || page_numbers.iter().enumerate().any(|(i, n)| *n != i + 1)
        {
            return Err(AppError::upstream("NAX 首页缺连续完整编号页导航"));
        }
        if !raw.contains(GALLERY_TITLE) {
            return Err(AppError::upstream("NAX 首页图库标题不符"));
        }
    } else if !page_numbers.is_empty() || raw.to_ascii_lowercase().contains("<html") {
        return Err(AppError::upstream("NAX 完整首页缺生成设置"));
    }

    let mut scope = raw;
    let mut found_section = false;
    for c in regex(&SECTION, r"(?is)<section\b([^>]*)>").captures_iter(raw) {
        if attrs(&c[1])?.get("id").is_some_and(|id| id == "gallery") {
            if found_section {
                return Err(AppError::upstream("NAX 重复图库 section"));
            }
            found_section = true;
            let start = c.get(0).unwrap().end();
            let end = regex(&SECTION_END, r"(?is)</section\s*>")
                .find(&raw[start..])
                .ok_or_else(|| AppError::upstream("NAX 图库 HTML 被截断"))?;
            scope = &raw[start..start + end.start()];
        }
    }
    if generation.is_some() && (!found_section || !raw.to_ascii_lowercase().contains("</html>")) {
        return Err(AppError::upstream("NAX 首页 HTML 不完整"));
    }
    let starts = regex(&FIGURE_START, r"(?is)<figure\b")
        .find_iter(scope)
        .count();
    let mut entries = Vec::new();
    for c in regex(&FIGURE, r"(?is)<figure\b([^>]*)>(.*?)</figure\s*>").captures_iter(scope) {
        if !has_class(&attrs(&c[1])?, "imagePanel") {
            return Err(AppError::upstream("NAX 图库包含未知 figure 类型"));
        }
        let body = &c[2];
        let mut source = None;
        for div in regex(&DIV, r"(?is)<div\b([^>]*)>").captures_iter(body) {
            let a = attrs(&div[1])?;
            if has_class(&a, "votes") {
                if source.is_some() {
                    return Err(AppError::upstream("NAX 条目含重复来源字段"));
                }
                source = Some(a);
            }
        }
        let a = source.ok_or_else(|| AppError::upstream("NAX 条目缺来源字段"))?;
        if a.get("data-gallery").map(String::as_str) != Some(GALLERY_ID) {
            return Err(AppError::upstream("NAX 条目不是目标图库"));
        }
        let artist = a
            .get("data-display-name")
            .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
            .ok_or_else(|| AppError::upstream("NAX 条目缺有效画师名"))?
            .clone();
        let filename = a
            .get("data-image")
            .ok_or_else(|| AppError::upstream("NAX 条目缺图片文件名"))?;
        validate_filename(filename)?;
        let image = encode_segment(filename);
        let expected = image_url(&image);
        let mut images = Vec::new();
        for img in regex(&IMG, r"(?is)<img\b([^>]*)>").captures_iter(body) {
            let a = attrs(&img[1])?;
            if let Some(src) = a.get("src").filter(|s| s.starts_with(IMAGE_PREFIX)) {
                images.push(src.clone());
            }
        }
        if images.len() != 1 || images[0] != expected {
            return Err(AppError::upstream(format!(
                "NAX 画师 {artist} 展示图 URL 不符"
            )));
        }
        let captions: Vec<String> = regex(
            &CAPTION,
            r"(?is)<figurecaption\b[^>]*>(.*?)</figurecaption\s*>",
        )
        .captures_iter(body)
        .map(|c| text_of(&c[1]))
        .collect::<AppResult<_>>()?;
        if captions.len() != 1 || captions[0] != artist {
            return Err(AppError::upstream(format!(
                "NAX 画师 {artist} 标题与来源字段不符"
            )));
        }
        let is_new = regex(&SPAN, r"(?is)<span\b([^>]*)>")
            .captures_iter(body)
            .map(|c| attrs(&c[1]))
            .collect::<AppResult<Vec<_>>>()?
            .iter()
            .any(|a| has_class(a, "new"));
        entries.push(GalleryEntry {
            artist,
            image,
            is_new,
        });
    }
    if starts == 0 || entries.len() != starts {
        return Err(AppError::upstream("NAX 图库为空或存在被截断的条目"));
    }
    let mut artists = BTreeSet::new();
    let mut files = BTreeSet::new();
    for e in &entries {
        if !artists.insert(&e.artist) || !files.insert(&e.image) {
            return Err(AppError::upstream("NAX 页内有重复画师或图片"));
        }
    }
    Ok(GalleryPage {
        entries,
        page_numbers,
        generation,
    })
}

/// 首页本身已包含第 1 页。后续只取网站 JS 所使用的公开编号页，不取热门/搜索切片。
pub fn page_urls(first: &GalleryPage) -> Vec<String> {
    first.page_numbers.iter().copied().filter(|n| *n > 1)
        .map(|n| format!("https://nax.moe/partials/components/gallery-display.php?gallery={GALLERY_ID}&sort=alphabetical&page={n}"))
        .collect()
}

/// 抓取结束后重读首页；页边界/设置变化时整批丢弃，由下次同步重试。
/// 投票数、NEW 标记和非图库头像不参与比较。
pub fn same_inventory(first: &GalleryPage, last: &GalleryPage) -> bool {
    first.page_numbers == last.page_numbers
        && first.generation == last.generation
        && first
            .entries
            .iter()
            .map(|e| (&e.artist, &e.image))
            .eq(last.entries.iter().map(|e| (&e.artist, &e.image)))
}

/// 将完整编号页拼成独立画师书。ID 按精确标签生成，位置/排名/新增不会改变旧 ID。
pub fn assemble_book(pages: &[GalleryPage]) -> AppResult<NaxSnapshot> {
    let first = pages
        .first()
        .ok_or_else(|| AppError::upstream("NAX 缺首页"))?;
    let settings = first
        .generation
        .as_ref()
        .ok_or_else(|| AppError::upstream("NAX 缺 V5 生成设置"))?;
    if pages.len() != first.page_numbers.len() {
        return Err(AppError::upstream("NAX 编号页未抓取完整"));
    }
    let mut source_entries: BTreeMap<String, GalleryEntry> = BTreeMap::new();
    let mut files = BTreeSet::new();
    for page in pages {
        if page.entries.is_empty() {
            return Err(AppError::upstream("NAX 有空编号页"));
        }
        for e in &page.entries {
            // 当前网站确有同一条目在相邻页边界重复的情况，同身份同图片可合并。
            // 编排对 NAX 禁用缺失删除，分页移动漏项也不会删本机旧条目。
            if let Some(existing) = source_entries.get_mut(&e.artist) {
                if existing.image != e.image {
                    return Err(AppError::upstream("NAX 同一画师有冲突的图片来源"));
                }
                existing.is_new |= e.is_new;
                continue;
            }
            if !files.insert(e.image.clone()) {
                return Err(AppError::upstream("NAX 不同画师共用同一图片来源"));
            }
            source_entries.insert(e.artist.clone(), e.clone());
        }
    }
    // BTreeMap 固定精确标签次序；官方页面分页移动不会造成顺序/版本噪音。
    let fingerprint_entries: Vec<_> = source_entries
        .values()
        .map(|e| (&e.artist, &e.image))
        .collect();
    let fingerprint = serde_json::to_string(
        &json!({"gallery": GALLERY_ID, "generation": settings, "entries": fingerprint_entries}),
    )?;
    let content_hash = hex::encode(Sha256::digest(fingerprint.as_bytes()));
    let mut entries = Vec::with_capacity(source_entries.len());
    for e in source_entries.into_values() {
        let tags = format!("artist:{}", e.artist);
        let note = format!(
            "来源：NAX · {GALLERY_TITLE}\nhttps://nax.moe/?gallery={GALLERY_ID}\n作者：u/Carlyone (Zelest) · CC BY 4.0\n模型：{}；Steps {}；Guidance {}；Quality Tags {}；UC Preset {}；Seed {}\n示例主提示词：{}\n角色 1：{}\n图片为网站公开的 WebP 展示图。",
            settings.model, settings.steps, settings.guidance, settings.quality_tags, settings.uc_preset, settings.seed,
            settings.prompt.replace("______", &e.artist), settings.character_prompt,
        );
        // CDN 可能沿用文件名重新生成示例图；设置改变也必须刷新图片及其画风向量。
        // 固定字段顺序的设置序列化与本条 URL 决定版本，新增其他画师不会使旧图失效。
        let asset_revision = serde_json::to_vec(&(image_url(&e.image), settings))?;
        let asset_rev = hex::encode(Sha256::digest(&asset_revision));
        entries.push(NormEntry {
            id: entry_id(&e.artist),
            title: e.artist,
            path: vec!["NAX".into(), GALLERY_TITLE.into()],
            tags,
            negative: None,
            character_prompts: None,
            note: Some(note),
            rating: "safe".into(),
            is_new: e.is_new,
            image: e.image.clone(),
            image_w: None,
            image_h: None,
            original: None,
            images: vec![NormImage {
                path: e.image,
                original: None,
                raw_tag: None,
            }],
            asset_rev: Some(asset_rev),
            update_batches: None,
        });
    }
    let count = entries.len() as i64;
    let raw = serde_json::to_string(&json!({
        "source": format!("https://nax.moe/?gallery={GALLERY_ID}"), "gallery": GALLERY_ID,
        "contentHash": content_hash, "generation": settings, "entries": entries,
    }))?;
    Ok(NaxSnapshot {
        book: UpstreamBook {
            id: BOOK_ID.into(),
            r#type: "string".into(),
            title: "NAX · V5 画师实验（宽松提示词）".into(),
            version: "V5 Full".into(),
            author: Some("u/Carlyone (Zelest) · CC BY 4.0".into()),
            entry_count: Some(count),
            tree: json!([{ "name": "NAX", "count": count, "children": [{ "name": GALLERY_TITLE, "count": count, "children": [] }] }]),
            update_filters: None,
            entries,
        },
        content_hash,
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figure(name: &str, filename: &str, votes: u32) -> String {
        format!(
            r#"<figure class="imagePanel"><div class="imageHolder"><div class="votes" data-image="{filename}" data-gallery="{GALLERY_ID}" data-display-name="{name}"><span class="vote-count">{votes}</span></div><img src="{IMAGE_PREFIX}{}" loading="lazy"></div><figurecaption class="imageText">{name}</figurecaption></figure>"#,
            encode_segment(filename)
        )
    }

    fn home(figures: &str, pages: usize) -> String {
        let links: String = (1..=pages).map(|n| format!(r#"<a class="page-link" data-page="{n}" href="?gallery={GALLERY_ID}&amp;page={n}">page</a>"#)).collect();
        format!(
            r#"<!doctype html><html><head><title>{GALLERY_TITLE}</title></head><body><p class="code">artist:______, cowboy shot, -2::blurry, lowres::</p><p class="code">girl, facing viewer, fully clothed</p><p>Model used is <span>NAI Diffusion V5 Full</span>, Steps <span>28</span>, Guidance <span>4</span>, Quality Tags <span>Enabled</span>, UC Preset <span>Heavy</span>, Seed <span>2138582679</span>.</p><nav>{links}</nav><section id="gallery">{figures}</section></body></html>"#
        )
    }

    #[test]
    fn parses_exact_artist_entities_settings_and_cdn_encoding() {
        let first = parse_gallery_page(&home(
            &figure(
                "MiXeD &amp; name (a)",
                "MiXeD%20%26%20name%20%28a%29.webp",
                5,
            ),
            1,
        ))
        .unwrap();
        let snapshot = assemble_book(&[first]).unwrap();
        let entry = &snapshot.book.entries[0];
        assert_eq!(entry.tags, "artist:MiXeD & name (a)");
        assert_eq!(entry.title, "MiXeD & name (a)");
        assert_eq!(entry.image, "MiXeD%2520%2526%2520name%2520%2528a%2529.webp");
        assert_eq!(
            image_url(&entry.image),
            format!("{IMAGE_PREFIX}MiXeD%2520%2526%2520name%2520%2528a%2529.webp")
        );
        assert!(entry.original.is_none());
        assert!(entry.character_prompts.is_none());
        let note = entry.note.as_ref().unwrap();
        assert!(note.contains("Steps 28；Guidance 4"));
        assert!(note.contains("artist:MiXeD & name (a), cowboy shot"));
    }

    #[test]
    fn ids_and_fingerprints_ignore_order_votes_and_new_markers() {
        let a = figure("MiXeD Name", "MiXeD%20Name.webp", 1);
        let b = figure("other", "other.webp", 8);
        let p1 = parse_gallery_page(&home(&(a.clone() + &b), 1)).unwrap();
        let changed = a.replace(">1<", ">99<").replace(
            "<div class=\"imageHolder\">",
            "<span class=\"new\">NEW</span><div class=\"imageHolder\">",
        );
        let p2 = parse_gallery_page(&home(&(b + &changed), 1)).unwrap();
        let s1 = assemble_book(&[p1.clone()]).unwrap();
        let s2 = assemble_book(&[p2]).unwrap();
        assert_eq!(s1.content_hash, s2.content_hash);
        assert_eq!(s1.book.entries[0].id, s2.book.entries[0].id);
        assert_ne!(entry_id("MiXeD Name"), entry_id("mixed name"));
        let more = parse_gallery_page(&home(&(a + &figure("added", "added.webp", 0)), 1)).unwrap();
        let s3 = assemble_book(&[more]).unwrap();
        let id = s1
            .book
            .entries
            .iter()
            .find(|e| e.title == "MiXeD Name")
            .unwrap()
            .id
            .clone();
        assert_eq!(
            s3.book
                .entries
                .iter()
                .find(|e| e.title == "MiXeD Name")
                .unwrap()
                .id,
            id
        );
        assert_ne!(s1.content_hash, s3.content_hash);
        let voted =
            parse_gallery_page(&home(&figure("MiXeD Name", "MiXeD%20Name.webp", 99), 1)).unwrap();
        let plain =
            parse_gallery_page(&home(&figure("MiXeD Name", "MiXeD%20Name.webp", 0), 1)).unwrap();
        assert!(same_inventory(&plain, &voted));
    }

    #[test]
    fn generation_settings_refresh_assets_without_invalidating_other_artist_additions() {
        fn snapshot(raw: &str) -> NaxSnapshot {
            assemble_book(&[parse_gallery_page(raw).unwrap()]).unwrap()
        }
        let original_figure = figure("Example Artist", "Example%20Artist.webp", 1);
        let original_html = home(&original_figure, 1);
        let original = snapshot(&original_html);
        let entry = &original.book.entries[0];
        // 即使图片路径没变，seed / steps / prompt 改变也让标准图片队列发现旧缓存。
        for changed in [
            original_html.replace("2138582679", "2138582680"),
            original_html.replace("<span>28</span>", "<span>27</span>"),
            original_html.replace("cowboy shot", "portrait"),
            original_html.replace("girl, facing viewer", "girl, looking away"),
        ] {
            let changed = snapshot(&changed);
            assert_eq!(changed.book.entries[0].id, entry.id);
            assert_eq!(changed.book.entries[0].image, entry.image);
            assert_ne!(changed.book.entries[0].asset_rev, entry.asset_rev);
            assert_ne!(changed.content_hash, original.content_hash);
        }
        let voted = snapshot(&home(
            &figure("Example Artist", "Example%20Artist.webp", 99),
            1,
        ));
        assert_eq!(voted.book.entries[0].asset_rev, entry.asset_rev);
        assert_eq!(voted.content_hash, original.content_hash);
        let added = snapshot(&home(
            &(original_figure + &figure("Added", "Added.webp", 0)),
            1,
        ));
        let retained = added
            .book
            .entries
            .iter()
            .find(|e| e.id == entry.id)
            .unwrap();
        assert_eq!(retained.asset_rev, entry.asset_rev);
        assert_ne!(added.content_hash, original.content_hash);
    }

    #[test]
    fn validates_complete_navigation_and_page_set_before_upsert() {
        let first = parse_gallery_page(&home(&figure("first", "first.webp", 0), 2)).unwrap();
        assert_eq!(page_urls(&first), vec![format!("https://nax.moe/partials/components/gallery-display.php?gallery={GALLERY_ID}&sort=alphabetical&page=2")]);
        assert!(assemble_book(&[first.clone()]).is_err());
        let second = parse_gallery_page(&figure("second", "second.webp", 0)).unwrap();
        assert_eq!(
            assemble_book(&[first.clone(), second])
                .unwrap()
                .book
                .entries
                .len(),
            2
        );
        let duplicate = parse_gallery_page(&figure("first", "first.webp", 0)).unwrap();
        assert_eq!(
            assemble_book(&[first.clone(), duplicate])
                .unwrap()
                .book
                .entries
                .len(),
            1
        );
        let conflict = parse_gallery_page(&figure("first", "other.webp", 0)).unwrap();
        assert!(assemble_book(&[first.clone(), conflict]).is_err());
        let shared_file = parse_gallery_page(&figure("other", "first.webp", 0)).unwrap();
        assert!(assemble_book(&[first.clone(), shared_file]).is_err());
        let moved = parse_gallery_page(&home(&figure("first", "first.webp", 0), 3)).unwrap();
        assert!(!same_inventory(&first, &moved));
        let missing_page = home(&figure("first", "first.webp", 0), 3)
            .replace("data-page=\"2\"", "data-page=\"-2\"");
        assert!(parse_gallery_page(&missing_page).is_err());
    }

    #[test]
    fn shared_synthetic_fixtures_assemble_without_real_source_data() {
        let first = parse_gallery_page(include_str!("fixtures/nax_home.html")).unwrap();
        let second = parse_gallery_page(include_str!("fixtures/nax_page_2.html")).unwrap();
        let snapshot = assemble_book(&[first, second]).unwrap();
        assert_eq!(snapshot.book.entry_count, Some(2));
        assert_eq!(snapshot.book.entries[0].tags, "artist:Example Artist");
        assert_eq!(snapshot.book.entries[1].tags, "artist:Synthetic (Demo)");
    }

    #[test]
    fn rejects_truncation_wrong_gallery_and_unsafe_images() {
        let good = figure("first", "first.webp", 0);
        for bad in [
            good.replace("</figure>", ""),
            good.replace("data-display-name", "data-display-missing"),
            good.replace(GALLERY_ID, "other-gallery"),
            good.replace("/first.webp\"", "/unexpected.webp\""),
            good.replace("<figurecaption", "<caption")
                .replace("</figurecaption", "</caption"),
            good.replace("first.webp", "..%2Ffirst.webp"),
            good.replace("first.webp", "first%ZZ.webp"),
        ] {
            assert!(
                parse_gallery_page(&bad).is_err(),
                "accepted malformed page: {bad}"
            );
        }
        assert!(parse_gallery_page(&home(&good, 1).replace("</html>", "")).is_err());
        assert!(parse_gallery_page(
            &home(&good, 1).replace("NAI Diffusion V5 Full", "NAI Diffusion V4 Full")
        )
        .is_err());
        assert!(parse_gallery_page("<div>Temporarily unavailable</div>").is_err());
    }
}
