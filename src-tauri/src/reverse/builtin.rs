//! 内置 `plugin v1` 模板与规则集，原文来自插件 `astrbot_plugin_vivian/src/reverse_draw.py`
//! （`DEFAULT_REVERSE_PROMPT`、`_APPEARANCE`、`_ACCESSORY_OK`、`_BANNED`、`_REFUSAL_TEXT`、`_clean_tags` 的 seen 初始集合）。
//! Python 正则改写为 Rust `regex` 语法（大小写不敏感由编译时 `case_insensitive` 提供，不写进串里）。
//! 首次启动 `reverse_template` / `reverse_ruleset` 为空时插入（CONTRACT §1 末尾）。
//! 角色名词表来自 `character_aliases.txt`（合成示例表，右侧 Danbooru 风格标签）。

use std::collections::HashSet;
use std::sync::OnceLock;

use rusqlite::{params, Connection};

use crate::db::now_iso;
use crate::error::AppResult;

pub const BUILTIN_NAME: &str = "plugin";
pub const BUILTIN_VERSION: i64 = 1;
pub const BUILTIN_NOTE: &str = "来自插件 reverse_draw.py（2026-09-06 版本）";

pub const DEFAULT_REVERSE_PROMPT: &str = concat!(
    "你是 Danbooru 标签反推器。仔细观察这张图片，输出一个 JSON 对象：\n",
    "{\"image_type\": \"<anime_illustration|anime_scenery|real_photo|3d_render|meme|text_heavy|other 之一>\", ",
    "\"subject\": \"<human_character|scenery|animal|object|other 之一>\", ",
    "\"suitable\": true或false, \"reason\": \"<不适合时用 15 字以内说明>\", ",
    "\"has_character\": true或false, \"framing\": \"<portrait|upper body|cowboy shot|full body|close-up 之一>\", ",
    "\"tags\": \"逗号分隔的英文 Danbooru tag\"}\n",
    "suitable 的判定：只有二次元插画/动漫风格（含二次元风景）才为 true；真人照片、3D 渲染、梗图/表情包、以文字为主、",
    "以动物或物品为主体且没有可借鉴的人物构图的图片，一律 false。\n",
    "当 suitable 为 true 时，tags 必须覆盖：构图与镜头角度、姿势、手势、表情、视线、服装与配饰（尽量完整精确：款式、颜色、材质、图案、细节）、",
    "手持道具、背景与场景物件、光线、氛围、季节/天气、画风或媒介（若明显）。18-30 个 tag。\n",
    "严格禁止输出以下内容（角色将被替换，这些会破坏新角色的设计）：发色、发型、发长、刘海、发饰以外的头部特征、瞳色、肤色、",
    "体型/胸围、角色名、作品名、画师名、1girl/1boy/solo 等人数 tag、质量词、以及 realistic/photo/3d/no humans 这类写实或无人类 tag。\n",
    "如果是二次元风景（没有人物），has_character 填 false，tags 只描述场景、物件、光线与氛围，",
    "并在 framing 里给出适合把一个角色放进该场景的构图。\n",
    "优先使用确实存在的 Danbooru tag，不要自创词组。只输出 JSON，不要任何其它文字。{extra}"
);

/// `_APPEARANCE`（re.I）
pub const APPEARANCE_REGEX: &str = r"\b(hair|eyes?|braids?|twintails?|ponytail|bangs|ahoge|bob cut|breasts?|flat chest|skin|tall|petite|1girl|1boy|solo|2girls|multiple girls)\b";
/// `_ACCESSORY_OK`（re.I）
pub const ACCESSORY_OK_REGEX: &str = r"\b(ribbon|ornament|clip|band|flower|pin|bow|hat|cap|hood|veil|tiara|crown|headdress)\b";
/// `_BANNED`（子串匹配，小写）
pub const BANNED: &[&str] = &[
    "wide shot", "very wide shot", "scenery only", "panorama", "from a distance", "tiny figure", "faceless",
    "no humans", "realistic", "photorealistic", "photo (medium)", "photo background", "3d", "animal focus",
    "meme", "dog", "cat", "animal", "pet", "text focus",
];
/// `_REFUSAL_TEXT`（re.I）
pub const REFUSAL_REGEX: &str = concat!(
    r"\b(?:i can'?t|i cannot|i am not able|i'?m not able|i won'?t|i am unable|i'?m unable",
    r"|i can not|i must decline|i'?m sorry|i am sorry|sorry, (?:but|i)|unable to (?:help|assist|comply)",
    r"|can'?t (?:help|assist|provide|comply)|cannot (?:help|assist|provide|comply)",
    r"|not able to (?:help|assist|provide)|against (?:my|the) (?:policy|guidelines)",
    r"|violates? (?:my|the)|content (?:policy|guidelines)",
    r"|inappropriate content|i'?d be happy to|happy to help with|instead,? i)",
    r"|不予|拒绝|无法(?:协助|提供|处理)|抱歉|很遗憾|不能(?:协助|提供)|违反"
);
/// `_clean_tags` 的 seen 初始集合（角色名由调用方另加）
pub const ALWAYS_DROP: &[&str] = &["1girl", "1boy", "solo"];
pub const MAX_WORDS: i64 = 6;
pub const MAX_LEN: i64 = 40;

const CHARACTER_ALIASES_TXT: &str = include_str!("character_aliases.txt");

/// `character_aliases.txt` 右侧角色标签，归一（小写、`_`→空格）后的集合。
pub fn character_aliases() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        CHARACTER_ALIASES_TXT
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once('=').map(|(_, r)| r.trim()))
            .filter(|r| !r.is_empty())
            .map(|r| super::metrics::norm(r))
            .collect()
    })
}

pub fn ensure_builtin(conn: &Connection) -> AppResult<()> {
    let now = now_iso();
    let templates: i64 = conn.query_row("SELECT COUNT(*) FROM reverse_template", [], |r| r.get(0))?;
    if templates == 0 {
        conn.execute(
            "INSERT INTO reverse_template(name, version, body, parent_id, created_at, note) VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
            params![BUILTIN_NAME, BUILTIN_VERSION, DEFAULT_REVERSE_PROMPT, now, BUILTIN_NOTE],
        )?;
        tracing::info!("inserted builtin reverse_template plugin v1");
    }
    let rulesets: i64 = conn.query_row("SELECT COUNT(*) FROM reverse_ruleset", [], |r| r.get(0))?;
    if rulesets == 0 {
        conn.execute(
            "INSERT INTO reverse_ruleset(name, version, banned_json, appearance_regex, accessory_ok_regex, refusal_regex, always_drop_json, max_words, max_len, created_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                BUILTIN_NAME,
                BUILTIN_VERSION,
                serde_json::to_string(BANNED)?,
                APPEARANCE_REGEX,
                ACCESSORY_OK_REGEX,
                REFUSAL_REGEX,
                serde_json::to_string(ALWAYS_DROP)?,
                MAX_WORDS,
                MAX_LEN,
                now,
                BUILTIN_NOTE,
            ],
        )?;
        tracing::info!("inserted builtin reverse_ruleset plugin v1");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_regexes_compile_and_match_like_python() {
        let app = regex::RegexBuilder::new(APPEARANCE_REGEX).case_insensitive(true).build().unwrap();
        let ok = regex::RegexBuilder::new(ACCESSORY_OK_REGEX).case_insensitive(true).build().unwrap();
        let refusal = regex::RegexBuilder::new(REFUSAL_REGEX).case_insensitive(true).build().unwrap();
        assert!(app.is_match("long Hair"));
        assert!(app.is_match("blue eyes"));
        assert!(!app.is_match("hairband")); // \b 边界：hairband 不命中 hair
        assert!(ok.is_match("hair ribbon"));
        assert!(refusal.is_match("I'm sorry, but I can't help with that"));
        assert!(refusal.is_match("抱歉，无法处理"));
        assert!(!refusal.is_match("looking at viewer"));
        assert!(character_aliases().contains("anna (sample series)"));
        assert!(character_aliases().contains("sample heroine"), "非括号型的别名也收");
        assert!(!character_aliases().iter().any(|t| t.contains('=')));
        assert!(DEFAULT_REVERSE_PROMPT.contains("{extra}"));
        assert_eq!(BANNED.len(), 20);
    }

    #[test]
    fn ensure_builtin_inserts_once() {
        let db = crate::db::Db::open_memory().unwrap();
        db.with(|c| {
            ensure_builtin(c)?;
            ensure_builtin(c)?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM reverse_template", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            let n: i64 = c.query_row("SELECT COUNT(*) FROM reverse_ruleset", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            let (name, version): (String, i64) = c.query_row("SELECT name, version FROM reverse_ruleset", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            assert_eq!((name.as_str(), version), ("plugin", 1));
            Ok(())
        })
        .unwrap();
    }
}
