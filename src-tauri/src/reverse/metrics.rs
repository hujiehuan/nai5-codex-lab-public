//! 评测口径（CONTRACT §4 / 设计 §5.4）：`norm` 归一；truth = COALESCE(raw_tag, tags) ∪ characterPrompts[].prompt；
//! 剔除类（artist/quality/negative/year、扩展质量词、rating:*）不在分母；
//! expected_miss（角色名、外貌词、人数词、banned）保留在答案但不计分母；
//! hit/precision/recall/f1/extra/expected_hit/appearance_leak/banned_leak/ruleset_drop/contradiction 全部在此计算。
//! v0.2.4（CONTRACT §11.4）：`truth_kind_of` 判题目真值是不是散文（散文题不算指标）；`leaks` 单独算两个泄漏数（任何题都算）。

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::artist::{is_prose_prompt, parse_artist_recipe, strip_wrappers};
use crate::models::{CharacterPrompt, Metric, TokenKind, TRUTH_KIND_PROSE, TRUTH_KIND_TAGS};

use super::clean::CompiledRuleset;

const QUALITY_EXTRA: &[&str] = &["absurdres", "highres", "best illustration", "high complexity", "highly complexity", "ultra complexity", "no text"];
const PEOPLE_WORDS: &[&str] = &["1girl", "1boy", "solo", "2girls", "2boys", "multiple girls", "multiple boys"];
/// 角色提示词 / label 里「首个非通用 token」要跳过的通用词（`角色1` / `char1` 这类占位形式见 `is_generic_word`）。
const GENERIC_WORDS: &[&str] = &[
    "1girl", "1boy", "solo", "2girls", "2boys", "girl", "boy", "female", "male", "1 girl", "1 boy", "multiple girls", "multiple boys", "character", "角色", "人物",
];

/// 归一后的 token 是否为通用词：人数 / 性别词，以及 `character` / `角色` / `char1` / `角色1` 这类占位 label。
/// 评测（`build_truth`）与图包导出（`export::clean_pack_source`，CONTRACT §9.6）共用。
pub fn is_generic_word(v: &str) -> bool {
    static PLACEHOLDER: OnceLock<Regex> = OnceLock::new();
    let placeholder = PLACEHOLDER.get_or_init(|| Regex::new(r"^(?:char(?:acter)?|角色|人物)\s*\d*$").expect("regex"));
    GENERIC_WORDS.contains(&v) || placeholder.is_match(v)
}

/// 小写、`_`→空格、去首尾空白与残留权重语法（`::`、`{}`、`[]`、括号强调）。
pub fn norm(t: &str) -> String {
    static WEIGHT: OnceLock<Regex> = OnceLock::new();
    static WS: OnceLock<Regex> = OnceLock::new();
    let weight = WEIGHT.get_or_init(|| Regex::new(r"^\s*-?\d+(?:\.\d+)?\s*::\s*|\s*::\s*$").expect("regex"));
    let ws = WS.get_or_init(|| Regex::new(r"\s+").expect("regex"));
    let mut s = t.trim().to_lowercase().replace('_', " ");
    s = weight.replace_all(&s, "").to_string();
    s = s.trim_matches(|c: char| c == '{' || c == '}' || c == '[' || c == ']').trim().to_string();
    ws.replace_all(&s, " ").trim().to_string()
}

fn is_rating(v: &str) -> bool {
    v.starts_with("rating:")
}

/// `\(.+\)$` 括号型（Danbooru 角色带作品名；`v` 须已 `norm`）。图包导出建主标签词表（`export::pack_tag_vocabulary`）时用它把括号型排除在外。
pub fn is_paren_form(v: &str) -> bool {
    static PAREN: OnceLock<Regex> = OnceLock::new();
    let paren = PAREN.get_or_init(|| Regex::new(r"\(.+\)$").expect("regex"));
    paren.is_match(v)
}

/// 角色名判定（`v` 须已 `norm`）：`\(.+\)$` 括号型或 aliases 表命中。
/// 评测与图包导出（CONTRACT §9.6）共用。
pub fn is_character_name(v: &str, aliases: &HashSet<String>) -> bool {
    is_paren_form(v) || aliases.contains(v)
}

#[derive(Debug, Clone)]
pub struct TruthInput<'a> {
    pub raw_tag: Option<&'a str>,
    pub tags: &'a str,
    pub character_prompts: &'a [CharacterPrompt],
    pub negative: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct TruthSets {
    /// 完整答案（归一去重，保持首次出现顺序）
    pub truth: Vec<String>,
    /// 剔除类
    pub excluded: Vec<String>,
    pub expected_miss: Vec<String>,
    /// truth − 剔除类 − expected_miss
    pub truth_eval: Vec<String>,
    /// negative ∪ characterPrompts[].negative 归一
    pub negatives: Vec<String>,
    pub truth_source: String,
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !s.is_empty() && !v.iter().any(|x| *x == s) {
        v.push(s);
    }
}

/// §11.4 truth_kind：题目真值文本（`COALESCE(raw_tag, tags)`，与 `build_truth` 取的是同一段）是散文（§11.1）→ `prose`，
/// 否则 `tags`；本地图片题没有真值，由调用方直接给 `none`。
pub fn truth_kind_of(raw_tag: Option<&str>, tags: &str) -> &'static str {
    if is_prose_prompt(raw_tag.unwrap_or(tags)) {
        TRUTH_KIND_PROSE
    } else {
        TRUTH_KIND_TAGS
    }
}

/// §11.4：`appearance_leak` / `banned_leak` 只看 `pred_raw`，与真值无关，任何 truth_kind 都算。
pub fn leaks(pred_raw: &[String], ruleset: &CompiledRuleset) -> (i64, i64) {
    let appearance = pred_raw.iter().filter(|p| ruleset.is_appearance(p)).count() as i64;
    let banned = pred_raw.iter().filter(|p| ruleset.is_banned(p)).count() as i64;
    (appearance, banned)
}

pub fn truth_source(raw_tag: Option<&str>, character_prompts: &[CharacterPrompt]) -> String {
    let base = if raw_tag.is_some() { "rawTag" } else { "tags" };
    if character_prompts.iter().any(|c| !c.prompt.trim().is_empty()) {
        format!("{base}+characterPrompts")
    } else {
        base.to_string()
    }
}

pub fn build_truth(input: &TruthInput<'_>, ruleset: &CompiledRuleset, aliases: &HashSet<String>) -> TruthSets {
    let mut sets = TruthSets { truth_source: truth_source(input.raw_tag, input.character_prompts), ..Default::default() };
    let mut excluded: HashSet<String> = HashSet::new();
    let mut expected: HashSet<String> = HashSet::new();

    let mut sources: Vec<String> = vec![input.raw_tag.unwrap_or(input.tags).to_string()];
    for cp in input.character_prompts {
        sources.push(cp.prompt.clone());
    }
    for (idx, src) in sources.iter().enumerate() {
        let tokens = parse_artist_recipe(&strip_wrappers(src));
        let mut first_char_done = false;
        for t in &tokens {
            let v = norm(&t.value);
            if v.is_empty() {
                continue;
            }
            let quality_extra = QUALITY_EXTRA.contains(&v.as_str());
            let is_excluded = matches!(t.kind, TokenKind::Artist | TokenKind::Quality | TokenKind::Negative | TokenKind::Year)
                || t.weight < 0.0
                || quality_extra
                || is_rating(&v);
            push_unique(&mut sets.truth, v.clone());
            if is_excluded {
                excluded.insert(v);
                continue;
            }
            let mut miss = false;
            if is_character_name(&v, aliases) || PEOPLE_WORDS.contains(&v.as_str()) || ruleset.is_banned(&v) || ruleset.is_appearance(&v) {
                miss = true;
            }
            // characterPrompts：每个 label 的首个非通用 token 视为角色名
            if idx > 0 && !first_char_done && !is_generic_word(&v) {
                first_char_done = true;
                miss = true;
            }
            if miss {
                expected.insert(v);
            }
        }
    }
    for v in &sets.truth {
        if excluded.contains(v) {
            sets.excluded.push(v.clone());
        } else if expected.contains(v) {
            sets.expected_miss.push(v.clone());
        } else {
            sets.truth_eval.push(v.clone());
        }
    }
    let mut negs: Vec<String> = Vec::new();
    if let Some(n) = input.negative {
        negs.push(n.to_string());
    }
    for cp in input.character_prompts {
        if let Some(n) = &cp.negative {
            negs.push(n.clone());
        }
    }
    for n in negs {
        for t in parse_artist_recipe(&strip_wrappers(&n)) {
            push_unique(&mut sets.negatives, norm(&t.value));
        }
    }
    sets
}

/// 模型原始 tags → pred_raw（parseArtistRecipe 后归一去重）。
pub fn pred_raw_from_tags(tags: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in parse_artist_recipe(tags) {
        push_unique(&mut out, norm(&t.value));
    }
    out
}

/// cleaned_tags ∪ {framing} 归一去重。
pub fn pred_from_cleaned(cleaned: &[String], framing: &str) -> Vec<String> {
    let mut out = Vec::new();
    for c in cleaned {
        push_unique(&mut out, norm(c));
    }
    let f = norm(framing);
    if !f.is_empty() {
        push_unique(&mut out, f);
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct MetricDetail {
    pub metric: Metric,
    pub hits: Vec<String>,
    pub misses: Vec<String>,
    pub extras: Vec<String>,
    pub expected_hits: Vec<String>,
    pub contradictions: Vec<String>,
}

pub fn compute(truth: &TruthSets, pred_raw: &[String], pred: &[String], ruleset: &CompiledRuleset) -> MetricDetail {
    let truth_all: HashSet<&str> = truth.truth.iter().map(|s| s.as_str()).collect();
    let eval: HashSet<&str> = truth.truth_eval.iter().map(|s| s.as_str()).collect();
    let expected: HashSet<&str> = truth.expected_miss.iter().map(|s| s.as_str()).collect();
    let negs: HashSet<&str> = truth.negatives.iter().map(|s| s.as_str()).collect();
    let pred_set: HashSet<&str> = pred.iter().map(|s| s.as_str()).collect();

    let hits: Vec<String> = pred.iter().filter(|p| eval.contains(p.as_str())).cloned().collect();
    let misses: Vec<String> = truth.truth_eval.iter().filter(|t| !pred_set.contains(t.as_str())).cloned().collect();
    let extras: Vec<String> = pred.iter().filter(|p| !truth_all.contains(p.as_str())).cloned().collect();
    let expected_hits: Vec<String> = pred.iter().filter(|p| expected.contains(p.as_str())).cloned().collect();
    let contradictions: Vec<String> = pred.iter().filter(|p| negs.contains(p.as_str())).cloned().collect();

    let hit = hits.len() as i64;
    let precision = if pred.is_empty() { 0.0 } else { hit as f64 / pred.len() as f64 };
    let recall = if truth.truth_eval.is_empty() { None } else { Some(hit as f64 / truth.truth_eval.len() as f64) };
    let f1 = recall.map(|r| if precision + r > 0.0 { 2.0 * precision * r / (precision + r) } else { 0.0 });
    let (appearance_leak, banned_leak) = leaks(pred_raw, ruleset);
    let ruleset_drop = (pred_raw.len() as i64 - pred.len() as i64).max(0);

    MetricDetail {
        metric: Metric {
            hit,
            missed: misses.len() as i64,
            extra: extras.len() as i64,
            expected_hit: expected_hits.len() as i64,
            precision: Some(precision),
            recall,
            f1,
            appearance_leak,
            banned_leak,
            ruleset_drop,
            contradiction: contradictions.len() as i64,
        },
        hits,
        misses,
        extras,
        expected_hits,
        contradictions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reverse::clean::builtin_ruleset;

    fn rs() -> CompiledRuleset {
        CompiledRuleset::compile(&builtin_ruleset()).unwrap()
    }

    #[test]
    fn norm_strips_weights_and_underscores() {
        assert_eq!(norm("1.2::Looking_At_Viewer::"), "looking at viewer");
        assert_eq!(norm("  {{smile}} "), "smile");
        assert_eq!(norm("-2::artist collaboration::"), "artist collaboration");
        assert_eq!(norm("Cowboy   Shot"), "cowboy shot");
    }

    /// 图包导出（CONTRACT §9.6）与评测共用的两个判定。
    #[test]
    fn generic_word_and_character_name_helpers() {
        for g in ["1girl", "solo", "multiple girls", "character", "角色", "人物", "角色1", "char2", "character 3"] {
            assert!(is_generic_word(g), "{g} 应是通用词");
        }
        for n in ["aria", "vera (sample)", "cowboy shot", "charlotte", "characters"] {
            assert!(!is_generic_word(n), "{n} 不是通用词");
        }
        let aliases = crate::reverse::builtin::character_aliases();
        assert!(is_character_name("aria (sample series)", aliases));
        assert!(is_character_name("vera (sample series)", aliases));
        assert!(is_character_name("someone (some series)", aliases), "括号型不需要在 aliases 表里");
        assert!(!is_character_name("cowboy shot", aliases));
        assert!(!is_character_name("aria", aliases), "无前缀名不在表里、也不是括号型");
        assert!(is_paren_form("photo (medium)") && is_paren_form("someone (some series)"));
        assert!(!is_paren_form("cowboy shot") && !is_paren_form("(open) mouth"), "括号必须收尾");
    }

    #[test]
    fn truth_partitions_into_excluded_expected_and_eval() {
        let cps = vec![CharacterPrompt { label: "char1".into(), prompt: "1girl, lumi (sample series), sitting, blue eyes".into(), negative: Some("blurry".into()) }];
        let input = TruthInput {
            raw_tag: Some("artist:foo, masterpiece, absurdres, year 2025, rating:general, -1::text::, looking at viewer, smile, long hair, hair ribbon, realistic, solo, anna (sample series)"),
            tags: "should not be used",
            character_prompts: &cps,
            negative: Some("lowres, bad hands"),
        };
        let aliases = crate::reverse::builtin::character_aliases();
        let t = build_truth(&input, &rs(), aliases);
        assert_eq!(t.truth_source, "rawTag+characterPrompts");
        for ex in ["artist:foo", "masterpiece", "absurdres", "year 2025", "rating:general", "text"] {
            assert!(t.excluded.contains(&ex.to_string()), "{ex} should be excluded");
        }
        for em in ["long hair", "realistic", "solo", "anna (sample series)", "1girl", "lumi (sample series)", "blue eyes"] {
            assert!(t.expected_miss.contains(&em.to_string()), "{em} should be expected_miss: {:?}", t.expected_miss);
        }
        assert_eq!(t.truth_eval, vec!["looking at viewer", "smile", "hair ribbon", "sitting"]);
        assert_eq!(t.negatives, vec!["lowres", "bad hands", "blurry"]);
        // 只用 tags 时
        let t2 = build_truth(&TruthInput { raw_tag: None, tags: "smile", character_prompts: &[], negative: None }, &rs(), aliases);
        assert_eq!(t2.truth_source, "tags");
        assert_eq!(t2.truth_eval, vec!["smile"]);
    }

    /// §11.4：truth_kind 看 COALESCE(raw_tag, tags)（与 build_truth 同一段文本）；泄漏数只看 pred_raw。
    #[test]
    fn truth_kind_classification_and_leaks() {
        assert_eq!(truth_kind_of(None, "1girl, smile, sitting"), TRUTH_KIND_TAGS);
        assert_eq!(truth_kind_of(None, "<scene>\nA girl by the window.\n</scene>"), TRUTH_KIND_PROSE);
        assert_eq!(truth_kind_of(None, "```\nprompt\n```"), TRUTH_KIND_PROSE);
        assert_eq!(truth_kind_of(None, "<artwork>\n<identity>\nx\n</identity>\n</artwork>"), TRUTH_KIND_PROSE);
        // raw_tag 优先：主 tags 是散文但这张图的 raw_tag 是标签 → tags；反之亦然
        assert_eq!(truth_kind_of(Some("1girl, smile"), "<artwork>\nx\n</artwork>"), TRUTH_KIND_TAGS);
        assert_eq!(truth_kind_of(Some("An anime style illustration of a young woman standing under a big old tree by the river"), "1girl, smile"), TRUTH_KIND_PROSE);
        // <artist> 块与 <style> 块都不算散文
        assert_eq!(truth_kind_of(None, "<artist>\nartist:a\n</artist>\n<style>\nyear 2025\n</style>\n1girl, smile"), TRUTH_KIND_TAGS);
        let ruleset = rs();
        let pred_raw = pred_raw_from_tags("Looking at viewer, smile, long hair, realistic, blurry, standing");
        assert_eq!(leaks(&pred_raw, &ruleset), (1, 1), "long hair 外貌、realistic 禁词");
        assert_eq!(leaks(&[], &ruleset), (0, 0));
    }

    #[test]
    fn metrics_hit_expected_hit_leak_contradiction() {
        let cps: Vec<CharacterPrompt> = vec![];
        let input = TruthInput { raw_tag: None, tags: "looking at viewer, smile, sitting, long hair, solo, masterpiece", character_prompts: &cps, negative: Some("blurry, bad hands") };
        let aliases = crate::reverse::builtin::character_aliases();
        let ruleset = rs();
        let truth = build_truth(&input, &ruleset, aliases);
        assert_eq!(truth.truth_eval, vec!["looking at viewer", "smile", "sitting"]);
        assert_eq!(truth.expected_miss, vec!["long hair", "solo"]);
        // 模型原样输出：含外貌词、禁词、拒答；清洗后 pred
        let raw = "Looking at viewer, smile, long hair, realistic, blurry, standing, cowboy shot";
        let pred_raw = pred_raw_from_tags(raw);
        let cleaned = ruleset.clean_tags(raw, "", "cowboy shot");
        assert_eq!(cleaned, vec!["Looking at viewer", "smile", "blurry", "standing"]);
        let pred = pred_from_cleaned(&cleaned, "cowboy shot");
        let d = compute(&truth, &pred_raw, &pred, &ruleset);
        assert_eq!(d.hits, vec!["looking at viewer", "smile"]);
        assert_eq!(d.misses, vec!["sitting"]);
        assert_eq!(d.extras, vec!["blurry", "standing", "cowboy shot"]);
        assert!(d.expected_hits.is_empty());
        assert_eq!(d.contradictions, vec!["blurry"]);
        let m = &d.metric;
        assert_eq!(m.hit, 2);
        assert_eq!(m.missed, 1);
        assert_eq!(m.extra, 3);
        assert_eq!(m.expected_hit, 0);
        assert_eq!(m.precision, Some(0.4));
        assert!((m.recall.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert!((m.f1.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(m.appearance_leak, 1);
        assert_eq!(m.banned_leak, 1);
        assert_eq!(m.ruleset_drop, 2);
        assert_eq!(m.contradiction, 1);
        // expected_hit：pred 里出现了 expected_miss 的词
        let pred2 = pred_from_cleaned(&["long hair".to_string(), "smile".to_string()], "");
        let d2 = compute(&truth, &pred2, &pred2, &ruleset);
        assert_eq!(d2.expected_hits, vec!["long hair"]);
        assert_eq!(d2.metric.expected_hit, 1);
        assert_eq!(d2.metric.hit, 1);
        // 空 pred：precision 0；分母 0：recall null
        let d3 = compute(&truth, &[], &[], &ruleset);
        assert_eq!(d3.metric.precision, Some(0.0));
        let empty_truth = build_truth(&TruthInput { raw_tag: None, tags: "masterpiece", character_prompts: &cps, negative: None }, &ruleset, aliases);
        let d4 = compute(&empty_truth, &pred, &pred, &ruleset);
        assert_eq!(d4.metric.recall, None);
        assert_eq!(d4.metric.f1, None);
    }
}
