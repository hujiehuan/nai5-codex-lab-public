//! 散文提示词判定（CONTRACT §11.1，与插件 `lab_sync.is_prose_prompt` 同一语义）。
//! 社区图包里有一批条目不是逗号分隔的标签，而是 XML 结构（`<artwork>…`、`<scene>…</scene>`）、代码围栏或整段自然语言；
//! 这些条目导出时不切分、不清洗（§11.2），反推评测时不算标签题（§11.4 `truth_kind='prose'`）。
//! - `strip_artist_block`：删除 `<artist>…</artist>`（大小写不敏感、跨行；没有闭合标签的原样保留，不猜到哪结束）。
//! - `artist_block_content`（v0.3.1，§12.7）：反过来只取 `<artist>…</artist>` 块里的内容，散文图包的画师串只从这里解析。
//! - `is_prose_prompt`：对 strip 后文本，满足任一即 true：含 `<artwork>`；含 "```"；存在独立成行的 XML 标签
//!   `^\s*<[a-z_]+>\s*$`（`<style>` 除外；正则只认小写标签名与不带属性的形式，照契约原文）；
//!   按 `[,\n]` 切分后非空 token 的平均词数 > 5。`<style>…</style>` 的内容按标签处理：只去掉标签本身，内容照常参与切分。

use std::sync::OnceLock;

use regex::Regex;

/// 平均词数阈值（§11.1：> 5 为散文）。
pub const PROSE_AVG_WORDS: f64 = 5.0;

static ARTIST_BLOCK: OnceLock<Regex> = OnceLock::new();
static ARTIST_BLOCK_CAPTURE: OnceLock<Regex> = OnceLock::new();
static XML_LINE: OnceLock<Regex> = OnceLock::new();
static STYLE_TAG: OnceLock<Regex> = OnceLock::new();

fn artist_block() -> &'static Regex {
    ARTIST_BLOCK.get_or_init(|| Regex::new(r"(?is)<artist>.*?</artist>").expect("regex"))
}

/// 契约原文 `^\s*<[a-z_]+>\s*$`（多行模式）；捕获组 1 是标签名。
fn xml_line() -> &'static Regex {
    XML_LINE.get_or_init(|| Regex::new(r"(?m)^\s*<([a-z_]+)>\s*$").expect("regex"))
}

fn style_tag() -> &'static Regex {
    STYLE_TAG.get_or_init(|| Regex::new(r"(?i)</?style>").expect("regex"))
}

fn artist_block_capture() -> &'static Regex {
    ARTIST_BLOCK_CAPTURE.get_or_init(|| Regex::new(r"(?is)<artist>(.*?)</artist>").expect("regex"))
}

/// 删除 `<artist>…</artist>`（大小写不敏感、跨行、多块都删）。
pub fn strip_artist_block(t: &str) -> String {
    artist_block().replace_all(t, "").into_owned()
}

/// §12.7：`<artist>…</artist>` 块的内容（与 `strip_artist_block` 同一匹配：大小写不敏感、跨行；多块用逗号接起来）；
/// 没有闭合的块或块里只有空白 → None。散文图包的画师串只从这里解析，块外的正文是散文、不当画师。
pub fn artist_block_content(t: &str) -> Option<String> {
    let parts: Vec<String> = artist_block_capture().captures_iter(t).filter_map(|c| c.get(1)).map(|m| m.as_str().trim().to_string()).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// 去掉 `<style>` / `</style>` 标签本身（内容保留）。
pub fn strip_style_tags(t: &str) -> String {
    style_tag().replace_all(t, "").into_owned()
}

/// 按 `[,\n]` 切分后非空 token 的平均词数（空白分词）；没有 token 为 0。
pub fn avg_words(t: &str) -> f64 {
    let mut tokens = 0usize;
    let mut words = 0usize;
    for tok in t.split(|c| c == ',' || c == '\n').map(str::trim).filter(|s| !s.is_empty()) {
        tokens += 1;
        words += tok.split_whitespace().count();
    }
    if tokens == 0 {
        0.0
    } else {
        words as f64 / tokens as f64
    }
}

/// `is_prose_prompt` 命中的那条规则（按 §11.1 的顺序判），给诊断 / 测试用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProseKind {
    /// 含 `<artwork>`
    Artwork,
    /// 含 "```"
    Fence,
    /// 有独立成行的 XML 标签（`<style>` 除外）
    XmlLine,
    /// 平均词数 > 5
    LongTokens,
}

/// 散文判定的诊断版本：None = 标签题。
pub fn prose_kind(t: &str) -> Option<ProseKind> {
    let s = strip_artist_block(t);
    if s.contains("<artwork>") {
        return Some(ProseKind::Artwork);
    }
    if s.contains("```") {
        return Some(ProseKind::Fence);
    }
    if xml_line().captures_iter(&s).any(|c| &c[1] != "style") {
        return Some(ProseKind::XmlLine);
    }
    if avg_words(&strip_style_tags(&s)) > PROSE_AVG_WORDS {
        return Some(ProseKind::LongTokens);
    }
    None
}

/// CONTRACT §11.1 `is_prose_prompt`。
pub fn is_prose_prompt(t: &str) -> bool {
    prose_kind(t).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn strip_artist_block_is_case_insensitive_and_multiline() {
        assert_eq!(strip_artist_block("<artist>\nartist:a, artist:b\n</artist>\n\n1girl, smile"), "\n\n1girl, smile");
        assert_eq!(strip_artist_block("<ARTIST>x</Artist>, solo"), ", solo");
        assert_eq!(strip_artist_block("<artist>a</artist>, x, <artist>b</artist>"), ", x, ");
        // 没有闭合标签：原样保留（不猜到哪结束）
        assert_eq!(strip_artist_block("<artist>a, b, 1girl"), "<artist>a, b, 1girl");
        assert_eq!(strip_artist_block("no block"), "no block");
        // 跨行且块里夹着 XML 行：整块一起删，不会把块里的 <scene> 当散文
        assert!(!is_prose_prompt("<artist>\n<scene>\nartist:a\n</scene>\n</artist>\n1girl, smile"));
    }

    /// §12.7 `artist_block_content`：只取块内、去首尾空白；大小写不敏感、跨行；多块逗号接；没块 / 未闭合 / 只有空白 → None。
    #[test]
    fn artist_block_content_takes_only_the_block() {
        assert_eq!(artist_block_content("<artist>\nartist:a, 1.2::artist:b::\n</artist>\n\n<scene>\nA girl.\n</scene>").as_deref(), Some("artist:a, 1.2::artist:b::"));
        assert_eq!(artist_block_content("<ARTIST>x</Artist>, solo").as_deref(), Some("x"));
        assert_eq!(artist_block_content("<artist>a</artist>, x, <artist>b</artist>").as_deref(), Some("a, b"));
        assert_eq!(artist_block_content("<artist>a, b, 1girl"), None, "没有闭合标签：不猜到哪结束");
        assert_eq!(artist_block_content("<artist>  \n </artist>1girl"), None, "只有空白当没有");
        assert_eq!(artist_block_content("artist:a, 1girl"), None);
        assert_eq!(artist_block_content(""), None);
    }

    #[test]
    fn prose_rules_in_contract_order() {
        assert_eq!(prose_kind("<artwork>\n  <scene>a</scene>\n</artwork>"), Some(ProseKind::Artwork));
        assert_eq!(prose_kind("1girl, smile\n```\na girl\n```"), Some(ProseKind::Fence));
        assert_eq!(prose_kind("1girl, smile\n<scene>\nA girl by the window.\n</scene>"), Some(ProseKind::XmlLine));
        assert_eq!(prose_kind("  <pose_and_biomechanics>  \nstanding\n"), Some(ProseKind::XmlLine), "前后空白也算独立成行");
        assert_eq!(
            prose_kind("An anime-style illustration of a young woman standing in a field of flowers, she wears a long white dress"),
            Some(ProseKind::LongTokens)
        );
        // 平均词数：(7 + 1) / 2 = 4 → 标签；(12 + 1) / 2 = 6.5 → 散文
        assert_eq!(prose_kind("a girl standing under a big tree, smile"), None);
        assert!(is_prose_prompt("a girl standing under a big old tree by the river bank, smile"));
        assert_eq!(prose_kind(""), None);
        assert_eq!(prose_kind("1girl, solo, looking at viewer, cowboy shot"), None);
        // 中文散文没有空格：平均词数规则不命中（契约原文按空白分词），只靠 XML / 围栏规则
        assert_eq!(prose_kind("1girl, solo, \n飞行在天空之中, 下面是城市夜景, 背景放着烟花"), None);
    }

    #[test]
    fn style_block_is_tags_not_prose() {
        // 独立成行的 <style> / </style> 不算 XML 标签；内容去掉标签后照常切分
        let t = "<artist>\nartist:a\n</artist>\n\n<style>\nyear 2025, fine fabric emphasis, 2::cute::\n</style>\n\n1girl, solo, smile";
        assert_eq!(prose_kind(t), None);
        // 同一行的 <style> … </style> 也不算
        assert_eq!(prose_kind("<style> year 2025, round face </style>1girl, loli, smile"), None);
        assert_eq!(strip_style_tags("<STYLE>a</style>"), "a");
        // 大写标签名不在 [a-z_] 里、带属性的标签也不匹配（契约原文；<artwork> 另有整块判定）
        assert_eq!(prose_kind("<Scene>\nx\n</Scene>"), None);
        assert_eq!(prose_kind("<scene bbox=\"[0,0,1,1]\">\nx\n</scene>"), None);
        assert_eq!(prose_kind("<artwork>\n<scene bbox=\"[0,0,1,1]\">\nx\n</scene>\n</artwork>"), Some(ProseKind::Artwork));
        // avg_words
        assert_eq!(avg_words(""), 0.0);
        assert_eq!(avg_words(" , ,\n"), 0.0);
        assert!((avg_words("a b c, d\ne f") - 2.0).abs() < 1e-9);
    }

    /// 上游社区图包全量样本（1898 条，不在仓库里）跑一遍：路径来自 `NAI5_PACK_FIXTURE`，缺省是调度脚本的 scratchpad；
    /// 文件不存在静默跳过。`cargo test fixture_prose -- --nocapture` 可看精确计数；同时把散文 id 列表写到同目录
    /// `prose_ids_rust.json`，给插件端 `lab_sync.is_prose_prompt` 对拍。
    ///
    /// 按 §11.1 原文实现，这份样本得到 159 条散文（8 artwork + 74 围栏 + 44 XML 行 + 33 长句）；契约里写的
    /// 「232 条 XML 散文、29 条长句」用 §11.1 的规则复现不出来（把独立成行的 `<style>` 也算 XML 才会到 209 + 29），
    /// 断言窗口按本实现的实测值给，差异记在 docs/CONTRACT_CHANGES.md。
    #[test]
    fn fixture_prose_count_when_sample_present() {
        const SCRATCH: &str = "";
        let path = std::env::var("NAI5_PACK_FIXTURE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(SCRATCH));
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let entries: Vec<serde_json::Value> = serde_json::from_str(&text).expect("fixture 应是条目数组");
        let style_line = Regex::new(r"(?m)^\s*<style>\s*$").unwrap();
        let mut ids: Vec<String> = Vec::new();
        let mut kinds = [0usize; 4];
        let mut style_line_only = 0usize;
        for e in &entries {
            let tags = e["tags"].as_str().unwrap_or("");
            match prose_kind(tags) {
                Some(k) => {
                    kinds[k as usize] += 1;
                    ids.push(e["id"].as_str().unwrap_or("").to_string());
                }
                None => {
                    if style_line.is_match(&strip_artist_block(tags)) {
                        style_line_only += 1;
                    }
                }
            }
        }
        let prose = ids.len();
        println!(
            "[prose fixture] entries={} prose={} (artwork={} fence={} xml_line={} long_tokens={}) tags={}; entries excluded only because their own-line tag is <style>: {}",
            entries.len(),
            prose,
            kinds[0],
            kinds[1],
            kinds[2],
            kinds[3],
            entries.len() - prose,
            style_line_only
        );
        let out = path.with_file_name("prose_ids_rust.json");
        std::fs::write(&out, serde_json::to_string_pretty(&ids).unwrap()).expect("写 prose_ids_rust.json");
        println!("[prose fixture] ids -> {}", out.display());
        assert!((150..=175).contains(&prose), "按 §11.1 原文实现的散文数应在 150..=175（实测 159），得到 {prose}");
    }
}
