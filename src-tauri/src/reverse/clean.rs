//! 规则集清洗（CONTRACT §4「规则集清洗」，对应插件 `_clean_tags`）：按逗号/换行切分 → trim 与去引号 →
//! 去重（含 always_drop 初始集合、角色名、framing）→ 丢弃长度 > max_len / 词数 > max_words / 命中 refusal_regex →
//! 丢弃 banned 子串命中 → 丢弃 appearance_regex 命中且不命中 accessory_ok_regex。
//! 规则集里的正则按大小写不敏感编译（插件用 re.I）。

use std::collections::HashSet;

use regex::{Regex, RegexBuilder};

use crate::error::{AppError, AppResult};
use crate::models::ReverseRuleset;

pub struct CompiledRuleset {
    pub banned: Vec<String>,
    pub appearance: Regex,
    pub accessory_ok: Regex,
    pub refusal: Regex,
    pub always_drop: HashSet<String>,
    pub max_words: usize,
    pub max_len: usize,
}

fn build(pattern: &str, what: &str) -> AppResult<Regex> {
    if pattern.trim().is_empty() {
        // 空正则视为「永不命中」
        return Ok(Regex::new(r"[^\s\S]").expect("never-match regex"));
    }
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| AppError::bad_request(format!("{what} 正则无效: {e}")))
}

impl CompiledRuleset {
    pub fn compile(rs: &ReverseRuleset) -> AppResult<Self> {
        Ok(CompiledRuleset {
            banned: rs.banned.iter().map(|b| b.to_lowercase()).filter(|b| !b.is_empty()).collect(),
            appearance: build(&rs.appearance_regex, "appearance_regex")?,
            accessory_ok: build(&rs.accessory_ok_regex, "accessory_ok_regex")?,
            refusal: build(&rs.refusal_regex, "refusal_regex")?,
            always_drop: rs.always_drop.iter().map(|s| s.to_lowercase()).collect(),
            max_words: rs.max_words.max(1) as usize,
            max_len: rs.max_len.max(1) as usize,
        })
    }

    pub fn is_banned(&self, lower: &str) -> bool {
        self.banned.iter().any(|b| lower.contains(b.as_str()))
    }

    pub fn is_appearance(&self, lower: &str) -> bool {
        self.appearance.is_match(lower) && !self.accessory_ok.is_match(lower)
    }

    /// 插件 `_clean_tags(raw, character, framing)`。
    pub fn clean_tags(&self, raw: &str, character: &str, framing: &str) -> Vec<String> {
        let mut seen: HashSet<String> = self.always_drop.clone();
        if !character.trim().is_empty() {
            seen.insert(character.trim().to_lowercase());
        }
        if !framing.trim().is_empty() {
            seen.insert(framing.trim().to_lowercase());
        }
        let mut kept = Vec::new();
        for piece in raw.replace('\n', ",").split(',') {
            let t = piece.trim().trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == ' ').trim_end_matches('.');
            let tl = t.to_lowercase();
            if t.is_empty() || seen.contains(&tl) || t.chars().count() > self.max_len {
                continue;
            }
            if self.refusal.is_match(t) || t.split_whitespace().count() > self.max_words {
                continue;
            }
            if self.is_banned(&tl) {
                continue;
            }
            if self.is_appearance(&tl) {
                continue;
            }
            seen.insert(tl);
            kept.push(t.to_string());
        }
        kept
    }
}

pub fn builtin_ruleset() -> ReverseRuleset {
    use super::builtin::*;
    ReverseRuleset {
        ruleset_id: 0,
        name: BUILTIN_NAME.into(),
        version: BUILTIN_VERSION,
        banned: BANNED.iter().map(|s| s.to_string()).collect(),
        appearance_regex: APPEARANCE_REGEX.into(),
        accessory_ok_regex: ACCESSORY_OK_REGEX.into(),
        refusal_regex: REFUSAL_REGEX.into(),
        always_drop: ALWAYS_DROP.iter().map(|s| s.to_string()).collect(),
        max_words: MAX_WORDS,
        max_len: MAX_LEN,
        created_at: String::new(),
        note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_tags_matches_plugin_behaviour() {
        let rs = CompiledRuleset::compile(&builtin_ruleset()).unwrap();
        let raw = "1girl, solo, `looking at viewer`, \"smile\", long hair, hair ribbon, blue eyes, Cowboy Shot, cowboy shot, \
                   I'm sorry but I can't describe this image in detail, realistic, cat ears, sitting on a very old wooden chair by the window, \
                   school uniform., dog tags, wide shot, vera (sample series)\nsunlight, twintails, hairband";
        let out = rs.clean_tags(raw, "vera (sample series)", "cowboy shot");
        assert_eq!(out, vec!["looking at viewer", "smile", "hair ribbon", "school uniform", "sunlight", "hairband"]);
        // 空规则集正则不炸
        let mut empty = builtin_ruleset();
        empty.appearance_regex.clear();
        empty.refusal_regex.clear();
        let rs2 = CompiledRuleset::compile(&empty).unwrap();
        assert_eq!(rs2.clean_tags("long hair, smile", "", ""), vec!["long hair", "smile"]);
        assert!(CompiledRuleset::compile(&ReverseRuleset { appearance_regex: "(".into(), ..builtin_ruleset() }).is_err());
    }
}
