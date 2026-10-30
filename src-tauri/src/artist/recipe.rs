//! `parseArtistRecipe` 的 Rust 移植（来源：Langbai `src/artist-recipe.ts`，MIT）。
//! 同时支持 `{}`/`[]` 层级强调与 V4 数值强调 `w::a, b::`（段内共享权重）；
//! 全角标点先 NFKC 归一再把 `，、；;` 视为逗号。分类正则见蓝图附录 J.3。
//! 本文件只做语法解析，画师词典相关的剥壳/补前缀在 `artist/mod.rs`。

use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::models::TokenKind;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedToken {
    pub raw: String,
    pub value: String,
    pub weight: f64,
    pub kind: TokenKind,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static regex"))
}

static QUALITY: OnceLock<Regex> = OnceLock::new();
static YEAR: OnceLock<Regex> = OnceLock::new();
static NEGATIVE: OnceLock<Regex> = OnceLock::new();
static STYLE: OnceLock<Regex> = OnceLock::new();
static ARTIST: OnceLock<Regex> = OnceLock::new();
static NUMERIC_START: OnceLock<Regex> = OnceLock::new();
static PAREN_WRAP: OnceLock<Regex> = OnceLock::new();
static WS: OnceLock<Regex> = OnceLock::new();

pub fn quality_pattern() -> &'static Regex {
    re(
        &QUALITY,
        r"(?i)^(masterpiece|best quality|amazing quality|very aesthetic|extremely detailed(?: cg)?|ultra[- ]?detailed|high quality)$",
    )
}
pub fn year_pattern() -> &'static Regex {
    re(&YEAR, r"(?i)^year[_ ]?\d{4}$")
}
pub fn negative_pattern() -> &'static Regex {
    re(&NEGATIVE, r"(?i)^(no\s+|negative\s+|avoid\s+|-\d+(?:\.\d+)?::)")
}
pub fn style_pattern() -> &'static Regex {
    re(
        &STYLE,
        r"(?i)(style|realism|impasto|illustration|painting|lineart|lighting|shading|cg|photorealistic|monochrome|sketch|watercolor)",
    )
}
pub fn artist_prefix() -> &'static Regex {
    re(&ARTIST, r"(?i)^artist\s*:\s*")
}

/// JS `Math.round(clamp(-10,10,v)*100)/100`（Math.round 对 .5 向 +∞ 取整）。
pub fn round_weight(value: f64) -> f64 {
    let clamped = value.clamp(-10.0, 10.0);
    (clamped * 100.0 + 0.5).floor() / 100.0
}

/// `canonicalArtistTagName`：NFKC → trim → 小写 → 空白折成 `_` → 别名表。
pub fn canonical_artist_tag_name(raw: &str) -> String {
    let normalized: String = raw.nfkc().collect::<String>().trim().to_lowercase();
    let underscored = re(&WS, r"\s+").replace_all(&normalized, "_").to_string();
    match underscored.as_str() {
        "channel_(_caststation)" => "channel_(caststation)".to_string(),
        "machi_(7769)" => "machi_(machi0910)".to_string(),
        _ => underscored,
    }
}

pub fn classify(value: &str) -> TokenKind {
    let normalized = value.trim();
    if artist_prefix().is_match(normalized) {
        return TokenKind::Artist;
    }
    if year_pattern().is_match(normalized) {
        return TokenKind::Year;
    }
    if quality_pattern().is_match(normalized) {
        return TokenKind::Quality;
    }
    if negative_pattern().is_match(normalized) {
        return TokenKind::Negative;
    }
    if style_pattern().is_match(normalized) {
        return TokenKind::Style;
    }
    TokenKind::Other
}

struct ParseState {
    output: Vec<ParsedToken>,
    token: String,
    curly: i32,
    square: i32,
    starting_level: Option<i32>,
    numeric_weight: Option<f64>,
}

impl ParseState {
    fn flush(&mut self) {
        let mut raw = self.token.trim().trim_matches(',').trim().to_string();
        self.token.clear();
        if raw.is_empty() || raw == "::" {
            self.starting_level = None;
            return;
        }
        let numeric_start = re(&NUMERIC_START, r"(?s)^(-?\d+(?:\.\d+)?)\s*::\s*(.*)$");
        if let Some(m) = numeric_start.captures(&raw) {
            if let Ok(w) = m[1].parse::<f64>() {
                self.numeric_weight = Some(w);
                raw = m[2].trim().to_string();
            }
        }
        let closes_numeric = raw.ends_with("::");
        if closes_numeric {
            raw = raw[..raw.len() - 2].trim().to_string();
        }
        if !raw.is_empty() {
            let weight = match self.numeric_weight {
                Some(w) => w,
                None => 1.05f64.powi(self.starting_level.unwrap_or(self.curly - self.square)),
            };
            let paren = re(&PAREN_WRAP, r"(?s)^\((.*)\)$");
            let value = match paren.captures(&raw) {
                Some(c) => c[1].trim().to_string(),
                None => raw.trim().to_string(),
            };
            let kind = if weight < 0.0 { TokenKind::Negative } else { classify(&value) };
            self.output.push(ParsedToken { raw: raw.clone(), value, weight: round_weight(weight), kind });
        }
        if closes_numeric {
            self.numeric_weight = None;
        }
        self.starting_level = None;
    }
}

/// 解析画师串 / 正面词串为 token 列表。输入不改动，调用方可原样保留。
pub fn parse_artist_recipe(input: &str) -> Vec<ParsedToken> {
    let source: String = input
        .nfkc()
        .map(|c| match c {
            '\u{00a0}' => ' ',
            '，' | '、' | '；' | ';' => ',',
            other => other,
        })
        .collect();
    let mut st = ParseState {
        output: Vec::new(),
        token: String::new(),
        curly: 0,
        square: 0,
        starting_level: None,
        numeric_weight: None,
    };
    for ch in source.chars() {
        match ch {
            '{' => {
                st.curly += 1;
                continue;
            }
            '}' => {
                st.curly = (st.curly - 1).max(0);
                continue;
            }
            '[' => {
                st.square += 1;
                continue;
            }
            ']' => {
                st.square = (st.square - 1).max(0);
                continue;
            }
            ',' => {
                st.flush();
                continue;
            }
            _ => {}
        }
        if st.starting_level.is_none() && !ch.is_whitespace() {
            st.starting_level = Some(st.curly - st.square);
        }
        st.token.push(ch);
    }
    st.flush();
    st.output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(tokens: &[ParsedToken]) -> Vec<&str> {
        tokens.iter().map(|t| t.value.as_str()).collect()
    }

    #[test]
    fn parses_full_width_bracket_and_numeric_emphasis() {
        // Langbai: "parses full-width punctuation, bracket emphasis, and numerical emphasis"
        let tokens = parse_artist_recipe("{{artist:foo，bar}}, 0.4::impasto ::，-2::no halo ::");
        assert_eq!(values(&tokens), vec!["artist:foo", "bar", "impasto", "no halo"]);
        assert!(tokens[0].weight > 1.0);
        assert_eq!(tokens[0].kind, TokenKind::Artist);
        assert_eq!(tokens[1].weight, round_weight(1.05f64.powi(2)));
        assert_eq!(tokens[2].weight, 0.4);
        assert_eq!(tokens[2].kind, TokenKind::Style);
        assert_eq!(tokens[3].weight, -2.0);
        assert_eq!(tokens[3].kind, TokenKind::Negative);
    }

    #[test]
    fn protects_every_token_inside_negative_numeric_segment() {
        // Langbai: "protects every token inside a numerical negative-emphasis segment"
        let tokens = parse_artist_recipe("-3::artist collaboration, text, variations::");
        let got: Vec<(&str, f64, TokenKind)> = tokens.iter().map(|t| (t.value.as_str(), t.weight, t.kind)).collect();
        assert_eq!(
            got,
            vec![
                ("artist collaboration", -3.0, TokenKind::Negative),
                ("text", -3.0, TokenKind::Negative),
                ("variations", -3.0, TokenKind::Negative),
            ]
        );
        // 段结束后权重恢复默认
        let after = parse_artist_recipe("-3::a, b::, c");
        assert_eq!(after[2].value, "c");
        assert_eq!(after[2].weight, 1.0);
        assert_eq!(after[2].kind, TokenKind::Other);
    }

    #[test]
    fn positive_shared_weight_segment_and_square_brackets() {
        let tokens = parse_artist_recipe("1.3::best quality,ultra-detailed,amazing quality::,[artist:pottsness],year 2025");
        assert_eq!(tokens[0].weight, 1.3);
        assert_eq!(tokens[0].kind, TokenKind::Quality);
        assert_eq!(tokens[1].weight, 1.3);
        assert_eq!(tokens[2].weight, 1.3);
        assert_eq!(tokens[3].value, "artist:pottsness");
        assert!(tokens[3].weight < 1.0);
        assert_eq!(tokens[4].kind, TokenKind::Year);
    }

    #[test]
    fn classify_covers_all_kinds() {
        assert_eq!(classify("artist: foo"), TokenKind::Artist);
        assert_eq!(classify("year_2026"), TokenKind::Year);
        assert_eq!(classify("ULTRA-DETAILED"), TokenKind::Quality);
        assert_eq!(classify("no text"), TokenKind::Negative);
        assert_eq!(classify("oil painting (medium)"), TokenKind::Style);
        assert_eq!(classify("cinematic lighting"), TokenKind::Style);
        assert_eq!(classify("looking at viewer"), TokenKind::Other);
    }

    #[test]
    fn canonical_name_normalizes_and_aliases() {
        assert_eq!(canonical_artist_tag_name("gochisousama (tanin050)"), "gochisousama_(tanin050)");
        assert_eq!(canonical_artist_tag_name(" Channel_(_Caststation) "), "channel_(caststation)");
        assert_eq!(canonical_artist_tag_name("machi_(7769)"), "machi_(machi0910)");
        assert_eq!(canonical_artist_tag_name("lumen brook   fieldnote"), "lumen_brook_fieldnote");
    }

    #[test]
    fn round_weight_matches_js_math_round() {
        assert_eq!(round_weight(0.125), 0.13);
        assert_eq!(round_weight(-0.125), -0.12);
        assert_eq!(round_weight(-2.125), -2.12);
        assert_eq!(round_weight(11.0), 10.0);
        assert_eq!(round_weight(0.4), 0.4);
    }

    #[test]
    fn parenthesised_token_and_double_commas() {
        let tokens = parse_artist_recipe("(masterpiece),, artist:a ,");
        assert_eq!(values(&tokens), vec!["masterpiece", "artist:a"]);
        assert_eq!(tokens[0].kind, TokenKind::Quality);
        assert!(parse_artist_recipe(" , :: , ").is_empty());
    }
}
