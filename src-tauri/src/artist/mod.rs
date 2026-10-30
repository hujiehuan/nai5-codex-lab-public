//! 画师串处理：解析器（`recipe.rs`）之上的词典专用规则——`<artist>`/`<style>` 剥壳与换行、
//! `单画师词典` 无前缀名补前缀（CONTRACT §3.1 第 1 条）、signature（蓝图 J.5：canonical 名 + 权重量化 0.25，
//! 排序后 `+` 连接）、`formatToken` / `core_of` / `compose` 序列化（权重 1 省略，`::` 前无空格，
//! QUALITY_SUFFIX 来自插件 artist_lab.py），以及图包导出用的 tags 清洗（CONTRACT §3.2）。
//! v0.2.4：散文提示词判定在 `prose.rs`（CONTRACT §11.1 `strip_artist_block` / `is_prose_prompt`），这里 re-export。

pub mod prose;
pub mod recipe;

use std::sync::OnceLock;

use regex::Regex;

use crate::models::{ArtistParse, ArtistWeighted, RecipeToken, TokenKind};
pub use prose::{artist_block_content, avg_words, is_prose_prompt, prose_kind, strip_artist_block, strip_style_tags, ProseKind};
pub use recipe::{canonical_artist_tag_name, parse_artist_recipe, round_weight, ParsedToken};

pub const QUALITY_SUFFIX: &str =
    "amazing quality, very aesthetic, absurdres, masterpiece, amazing, best quality, -2::artist collaboration::";
pub const COLLAB_GUARD: &str = "-2::artist collaboration::";
/// `path[0]` 为此值且 tags 不含 `artist:` 的条目按无前缀画师名处理。
pub const SINGLE_ARTIST_DIR: &str = "单画师词典";

/// 用于画风检索复制、重画与完整画风池的原始配方。
/// 质量词、线条词、正负权重与内部格式均属于配方，不经过画师解析器重写。
pub fn style_prompt(tags: &str) -> String {
    tags.trim().to_string()
}

#[cfg(test)]
pub(crate) const FULL_STYLE_RECIPE: &str = "0.75::artist:freng::, 0.8::artist:nyong nyong::, 0.8::artist:hiro (dismaless)::, 0.5::artist:morino hon::, 0.4::artist:kingin::, 0.6::artist:ebora::, 0.7::artist:ramanda::, 0.6::artist:modare::, -1::artist collaboration::, very aesthetic, amazing quality, ultra complexity, thick outlines, -3::monochrome::, -1::limited palette::, -1::flat color::, -1::pale colors::, -1::muted colors::,";

static WRAPPER: OnceLock<Regex> = OnceLock::new();
static RATING: OnceLock<Regex> = OnceLock::new();

/// 剥 `<artist>`、`</artist>`、`<style>`、`</style>`，换行视为分隔符（都替换成逗号，空 token 由解析器丢弃）。
pub fn strip_wrappers(tags: &str) -> String {
    let re = WRAPPER.get_or_init(|| Regex::new(r"(?i)</?artist>|</?style>|\r?\n").expect("regex"));
    re.replace_all(tags, ",").into_owned()
}

/// 数字按「最多两位小数、去尾零」输出（JS 模板字符串同款）。
pub fn fmt_weight(w: f64) -> String {
    let r = round_weight(w);
    if (r - r.trunc()).abs() < f64::EPSILON {
        format!("{}", r.trunc() as i64)
    } else {
        format!("{r}")
    }
}

/// `formatToken`：权重 1 输出原值，否则 `w::value::`（CONTRACT §3.2，无空格）。
pub fn format_token(value: &str, weight: f64) -> String {
    if (weight - 1.0).abs() < 0.001 {
        value.to_string()
    } else {
        format!("{}::{}::", fmt_weight(weight), value)
    }
}

/// `core_of`：`1.2::artist:a::, artist:b`。
pub fn core_of(artists: &[ArtistWeighted]) -> String {
    artists
        .iter()
        .filter(|a| !a.name.trim().is_empty())
        .map(|a| {
            let tok = format!("artist:{}", a.name.trim());
            if (a.weight - 1.0).abs() < 0.001 {
                tok
            } else {
                format!("{}::{}::", fmt_weight(a.weight), tok)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `compose(for_pool=True)`：core + QUALITY_SUFFIX；core 为空返回空串。
pub fn compose(artists: &[ArtistWeighted]) -> String {
    let core = core_of(artists);
    if core.is_empty() {
        String::new()
    } else {
        format!("{core}, {QUALITY_SUFFIX}")
    }
}

/// 蓝图 J.5：`canonical(name)@round(w*4)/4`，排序后 `+` 连接。
pub fn signature(artists: &[ArtistWeighted]) -> String {
    let mut parts: Vec<String> = artists
        .iter()
        .map(|a| {
            let q = (a.weight * 4.0 + 0.5).floor() / 4.0;
            format!("{}@{}", canonical_artist_tag_name(&a.name), fmt_plain(q))
        })
        .collect();
    parts.sort();
    parts.join("+")
}

fn fmt_plain(x: f64) -> String {
    if (x - x.trunc()).abs() < f64::EPSILON {
        format!("{}", x.trunc() as i64)
    } else {
        format!("{x}")
    }
}

fn to_recipe_token(t: &ParsedToken) -> RecipeToken {
    RecipeToken { value: t.value.clone(), weight: t.weight, kind: t.kind }
}

/// 词典条目 tags → 结构化解析（写入 `artist_parse`）。
/// `path0` 为条目 `path[0]`，用于无前缀名判定。
pub fn parse_entry_tags(tags: &str, path0: Option<&str>) -> ArtistParse {
    let src = strip_wrappers(tags);
    let tokens = parse_artist_recipe(&src);
    let mut warnings: Vec<String> = Vec::new();
    let has_artist_prefix = tokens.iter().any(|t| t.kind == TokenKind::Artist);
    let bare_mode = path0 == Some(SINGLE_ARTIST_DIR) && !has_artist_prefix;
    if bare_mode {
        warnings.push("bare-name: tags 无 artist: 前缀，按无前缀画师名补前缀".to_string());
    }
    let mut artists = Vec::new();
    let mut auxiliary = Vec::new();
    let mut negative_tail = Vec::new();
    for t in &tokens {
        if t.kind == TokenKind::Negative || t.weight < 0.0 {
            negative_tail.push(to_recipe_token(t));
            continue;
        }
        if t.kind == TokenKind::Artist {
            let name = recipe::artist_prefix().replace(&t.value, "").trim().to_string();
            if name.is_empty() {
                warnings.push(format!("empty artist name in token '{}'", t.raw));
            } else {
                artists.push(ArtistWeighted { name, weight: t.weight });
            }
            continue;
        }
        if bare_mode && matches!(t.kind, TokenKind::Other | TokenKind::Style) {
            artists.push(ArtistWeighted { name: t.value.trim().to_string(), weight: t.weight });
            continue;
        }
        if t.value.starts_with("::") {
            warnings.push(format!("unparsed weight syntax in token '{}'", t.raw));
        }
        auxiliary.push(to_recipe_token(t));
    }
    if artists.is_empty() {
        warnings.push("no artists parsed".to_string());
    }
    let signature = signature(&artists);
    ArtistParse { artists, auxiliary, negative_tail, signature, warnings }
}

/// CONTRACT §3.2 图包 tags 清洗：剔 artist/quality/negative/year 与 `rating:*`，其余按原顺序 formatToken。
pub fn clean_pack_tags(tags: &str) -> String {
    clean_pack_tokens(tags)
        .iter()
        .map(|t| format_token(&t.value, t.weight))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn clean_pack_tokens(tags: &str) -> Vec<RecipeToken> {
    let rating = RATING.get_or_init(|| Regex::new(r"(?i)^rating:").expect("regex"));
    parse_artist_recipe(&strip_wrappers(tags))
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Artist | TokenKind::Quality | TokenKind::Negative | TokenKind::Year))
        .filter(|t| !rating.is_match(&t.value))
        .map(to_recipe_token)
        .collect()
}

/// `t` 字段用：前 N 个非画师非质量 token 的 value（CONTRACT §3.2）。
pub fn first_content_values(tags: &str, n: usize) -> Vec<String> {
    parse_artist_recipe(&strip_wrappers(tags))
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Artist | TokenKind::Quality))
        .map(|t| t.value.clone())
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_style_recipe_retains_weights_auxiliary_tags_and_formatting() {
        assert_eq!(style_prompt(&format!("\n  {FULL_STYLE_RECIPE}\t")), FULL_STYLE_RECIPE);
        let precise = "1.2375::artist:Mixed Name::,\n thick outlines, -2.345::flat color::,";
        assert_eq!(style_prompt(precise), precise, "权重不取整、内部换行和大小写不重写");
        assert_eq!(style_prompt("  \n "), "");
    }

    fn names(p: &ArtistParse) -> Vec<(String, f64)> {
        p.artists.iter().map(|a| (a.name.clone(), a.weight)).collect()
    }

    #[test]
    fn strips_artist_and_style_wrappers() {
        let raw = "<artist>\n0.7::artist:mo_lantern153 ::, 1::artist:hollow_(mint046)::\n</artist>,\n\n<style>\n2::chibi::, year 2026, -3::artist collaboration::\n</style>";
        let p = parse_entry_tags(raw, Some("画风组词典"));
        assert_eq!(names(&p), vec![("mo_lantern153".to_string(), 0.7), ("hollow_(mint046)".to_string(), 1.0)]);
        assert!(p.auxiliary.iter().any(|t| t.value == "chibi" && t.weight == 2.0));
        assert!(p.auxiliary.iter().any(|t| t.kind == TokenKind::Year));
        assert_eq!(p.negative_tail.len(), 1);
        assert_eq!(p.negative_tail[0].value, "artist collaboration");
        // 无逗号衔接的壳：</artist> 与 <style> 之间不能把 token 粘起来
        let p2 = parse_entry_tags("<artist>\nartist:a,shio (shiofeifei)\n</artist>\n\n<style>\nyear 2025\n</style>", None);
        assert_eq!(p2.artists.len(), 1);
        assert!(p2.auxiliary.iter().any(|t| t.value == "shio (shiofeifei)"));
        assert!(p2.auxiliary.iter().any(|t| t.value == "year 2025"));
    }

    #[test]
    fn bare_name_in_single_artist_dir_gets_prefix() {
        let p = parse_entry_tags("lumen brook fieldnote", Some(SINGLE_ARTIST_DIR));
        assert_eq!(names(&p), vec![("lumen brook fieldnote".to_string(), 1.0)]);
        assert_eq!(core_of(&p.artists), "artist:lumen brook fieldnote");
        assert!(p.warnings.iter().any(|w| w.starts_with("bare-name")));
        // 其他目录的无前缀名不补
        let q = parse_entry_tags("lumen brook fieldnote", Some("画风组词典"));
        assert!(q.artists.is_empty());
        assert!(q.warnings.iter().any(|w| w == "no artists parsed"));
        // 已有 artist: 前缀的单画师目录条目不走无前缀名逻辑
        let r = parse_entry_tags("artist:inkwell, masterpiece", Some(SINGLE_ARTIST_DIR));
        assert_eq!(names(&r), vec![("inkwell".to_string(), 1.0)]);
        assert_eq!(r.auxiliary.len(), 1);
    }

    #[test]
    fn signature_sorts_and_quantizes() {
        let a = vec![
            ArtistWeighted { name: "Zeta Tan".into(), weight: 0.7 },
            ArtistWeighted { name: "alpha".into(), weight: 1.0 },
        ];
        assert_eq!(signature(&a), "alpha@1+zeta_tan@0.75");
        let b = vec![
            ArtistWeighted { name: "alpha".into(), weight: 1.1 },
            ArtistWeighted { name: "zeta_tan".into(), weight: 0.65 },
        ];
        // 权重量化到 0.25 后相同 → 同 signature
        assert_eq!(signature(&a), signature(&b));
        assert_eq!(signature(&[]), "");
    }

    #[test]
    fn core_and_compose_serialization() {
        let a = vec![
            ArtistWeighted { name: "kagawa yuusaku".into(), weight: 1.2 },
            ArtistWeighted { name: "asteroid ill".into(), weight: 1.0 },
            ArtistWeighted { name: "x".into(), weight: 0.999_9 },
        ];
        assert_eq!(core_of(&a), "1.2::artist:kagawa yuusaku::, artist:asteroid ill, artist:x");
        assert_eq!(
            compose(&a),
            "1.2::artist:kagawa yuusaku::, artist:asteroid ill, artist:x, amazing quality, very aesthetic, absurdres, masterpiece, amazing, best quality, -2::artist collaboration::"
        );
        assert_eq!(compose(&[]), "");
        assert_eq!(format_token("chibi", 2.0), "2::chibi::");
        assert_eq!(format_token("chibi", 1.0), "chibi");
        assert_eq!(format_token("x", 0.7), "0.7::x::");
        assert_eq!(fmt_weight(1.94), "1.94");
        assert_eq!(fmt_weight(0.5), "0.5");
    }

    #[test]
    fn pack_tags_cleaning_drops_artist_quality_year_rating() {
        let tags = "rating:general,artist:tessel,year 2025,2::chromatic_aberration::,1.3::best quality,realistic::,sunlight,rating:general,-1::blurry::,\nsoft focus, masterpiece, no text";
        let out = clean_pack_tags(tags);
        assert_eq!(out, "2::chromatic_aberration::, 1.3::realistic::, sunlight, soft focus");
        let vals = first_content_values(tags, 3);
        assert_eq!(vals, vec!["rating:general", "year 2025", "chromatic_aberration"]);
    }
}
