//! Material bundles for external prompting models, never executable agent instructions.
use super::store;
use crate::{
    creative,
    error::{AppError, AppResult},
    plugin,
    state::AppState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, io::Read, path::Path};

const MAX_BYTES: usize = 768 * 1024;
const PLUGIN_SNAPSHOT_PREFIX: &str = "plugin-skill-";
pub const NPE_REVISION: &str = "58f98acfbc9b7556258a294183ef46057e00afb1";
const NPE_FILES: &[&str] = &[
    "SKILL.md",
    "references/aesthetic-direction.md",
    "references/character-design.md",
    "references/color-design.md",
    "references/composition-design.md",
    "references/creative-studio.md",
    "references/effects-design.md",
    "references/english-typesetting.md",
    "references/fashion-design.md",
    "references/form-rhythm-design.md",
    "references/graphic-design.md",
    "references/lighting-design.md",
    "references/negative-quality-tags.md",
    "references/pose-gesture-design.md",
    "references/positive-quality-tags.md",
    "references/prompt-craft.md",
    "references/reading-synthesis.md",
    "references/render-medium-design.md",
    "references/scene-design.md",
    "references/sequential-comic-design.md",
    "references/spatial-reasoning.md",
    "references/subject-object-design.md",
    "references/tag-ontology.md",
    "references/user-semantic-understanding.md",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub text: String,
    pub source: String,
    pub builtin: bool,
    pub bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSkillSnapshot {
    pub version: u32,
    pub kind: String,
    pub enabled: bool,
    pub selected_id: String,
    pub skill: Option<Skill>,
}
#[derive(Deserialize)]
pub struct ImportRequest {
    pub path: String,
    pub name: Option<String>,
}
fn make(
    name: String,
    version: String,
    text: String,
    source: String,
    builtin: bool,
) -> AppResult<Skill> {
    if text.trim().is_empty()
        || text.len() > MAX_BYTES
        || name.trim().is_empty()
        || name.len() > 200
        || text.contains('\0')
    {
        return Err(AppError::bad_request(
            "技能名称/正文无效（单份最多 768 KiB）",
        ));
    }
    let sha256 = store::hash(text.as_bytes());
    Ok(Skill {
        id: if builtin {
            "nai5-prompting".into()
        } else {
            format!("skill-{}", &sha256[..24])
        },
        name,
        version,
        bytes: text.len(),
        text,
        sha256,
        source,
        builtin,
    })
}
fn builtin() -> Skill {
    make(
        "NAI5 提示词写法 · 内置简版".into(),
        creative::SKILL_VERSION.into(),
        creative::SKILL_TEXT.into(),
        "resources/prompting-guide/GUIDE.md".into(),
        true,
    )
    .expect("valid bundled skill")
}
pub fn list(root: &Path) -> AppResult<Vec<Skill>> {
    let mut out = vec![builtin()];
    let dir = root.join("studio/skills");
    if !dir.exists() {
        return Ok(out);
    }
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let s: Skill = serde_json::from_value(store::read(&p)?)?;
        if s.builtin
            || s.sha256 != store::hash(s.text.as_bytes())
            || s.bytes != s.text.len()
            || s.bytes > MAX_BYTES
        {
            return Err(AppError::io("导入的技能记录不完整或摘要不匹配"));
        }
        out.push(s);
    }
    out[1..].sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
pub fn bundles(root: &Path, ids: &[String]) -> AppResult<Vec<Skill>> {
    if ids.len() > 8 || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len() {
        return Err(AppError::bad_request("技能组合最多8项且不能重复"));
    }
    if ids.iter().any(|id| id == "plugin-active") {
        return Err(AppError::bad_request(
            "插件当前资料尚未冻结，请先同步所选 skill 后重新准备本轮实验",
        ));
    }
    let catalog = if ids.iter().any(|id| !id.starts_with(PLUGIN_SNAPSHOT_PREFIX)) {
        list(root)?
    } else {
        vec![]
    };
    let out = ids
        .iter()
        .map(|id| {
            if id.starts_with(PLUGIN_SNAPSHOT_PREFIX) {
                return load_plugin_snapshot(root, id);
            }
            catalog
                .iter()
                .find(|s| &s.id == id)
                .cloned()
                .ok_or_else(|| AppError::not_found(format!("未导入技能：{id}")))
        })
        .collect::<AppResult<Vec<_>>>()?;
    if out.iter().map(|s| s.bytes).sum::<usize>() > 1024 * 1024 {
        return Err(AppError::bad_request(
            "技能组合超过 1 MiB，请减少组合；不会截断资料",
        ));
    }
    Ok(out)
}
fn valid_plugin_kind(kind: &str) -> AppResult<()> {
    if !matches!(kind, "draw" | "reverse") {
        return Err(AppError::bad_request(
            "插件 skill 类型必须是 draw 或 reverse",
        ));
    }
    Ok(())
}
fn validate_plugin_snapshot(kind: &str, value: Value) -> AppResult<PluginSkillSnapshot> {
    if value.get("skill").is_none() {
        return Err(super::protocol("插件 skill 响应缺少资料字段"));
    }
    let snapshot: PluginSkillSnapshot = serde_json::from_value(value)
        .map_err(|_| super::protocol("插件 skill 响应字段不完整或类型不正确"))?;
    if snapshot.version != 1 || snapshot.kind != kind {
        return Err(super::protocol("插件 skill 版本或实验类型不匹配"));
    }
    if !snapshot.enabled {
        if kind != "reverse" || !snapshot.selected_id.is_empty() || snapshot.skill.is_some() {
            return Err(super::protocol("插件 skill 关闭状态不一致"));
        }
        return Ok(snapshot);
    }
    let skill = snapshot
        .skill
        .as_ref()
        .ok_or_else(|| super::protocol("插件已启用 skill，但没有返回正文"))?;
    if [
        &snapshot.selected_id,
        &skill.id,
        &skill.name,
        &skill.version,
    ]
    .iter()
    .any(|value| value.trim().is_empty() || value.len() > 200 || value.contains('\0'))
        || skill.id != snapshot.selected_id
        || skill.source.contains('\0')
        || skill.text.trim().is_empty()
        || skill.text.contains('\0')
        || skill.text.len() > MAX_BYTES
        || skill.bytes != skill.text.len()
        || skill.sha256 != store::hash(skill.text.as_bytes())
    {
        return Err(super::protocol(
            "插件 skill 正文、版本或摘要校验失败；未使用旧缓存或内置资料",
        ));
    }
    Ok(snapshot)
}
fn plugin_snapshot_id(snapshot: &PluginSkillSnapshot) -> AppResult<String> {
    // Include the selection and version as well as text: equal text with a new
    // upstream revision must never overwrite an older experiment's metadata.
    Ok(format!(
        "{PLUGIN_SNAPSHOT_PREFIX}{}",
        store::hash(&serde_json::to_vec(snapshot)?)
    ))
}
fn freeze_plugin_snapshot(root: &Path, kind: &str, value: Value) -> AppResult<PluginSkillSnapshot> {
    let mut snapshot = validate_plugin_snapshot(kind, value)?;
    if snapshot.enabled {
        let id = plugin_snapshot_id(&snapshot)?;
        snapshot.skill.as_mut().expect("validated enabled skill").id = id.clone();
        store::write_once(
            &store::path(root, "plugin_skills", &id)?,
            &serde_json::to_value(&snapshot)?,
        )?;
    }
    Ok(snapshot)
}
fn load_plugin_snapshot(root: &Path, id: &str) -> AppResult<Skill> {
    let value = store::read(&store::path(root, "plugin_skills", id)?)?;
    let frozen: PluginSkillSnapshot = serde_json::from_value(value)
        .map_err(|_| super::protocol("本轮插件 skill 快照损坏，未切换到当前版本"))?;
    valid_plugin_kind(&frozen.kind)?;
    let skill = frozen
        .skill
        .as_ref()
        .filter(|skill| skill.id == id && frozen.enabled)
        .ok_or_else(|| super::protocol("本轮插件 skill 快照缺少匹配的资料"))?;
    let mut remote = frozen.clone();
    remote.skill.as_mut().expect("validated frozen skill").id = frozen.selected_id.clone();
    validate_plugin_snapshot(&frozen.kind, serde_json::to_value(&remote)?)?;
    if plugin_snapshot_id(&remote)? != id {
        return Err(super::protocol(
            "本轮插件 skill 快照摘要不匹配，未切换到当前版本",
        ));
    }
    Ok(skill.clone())
}
pub async fn sync(state: &AppState, kind: &str) -> AppResult<PluginSkillSnapshot> {
    valid_plugin_kind(kind)?;
    let conn = plugin::plugin_conn(state).await?;
    // Every invocation reads the plugin selection. A failed request must not
    // become a successful response from a previous local snapshot.
    let value = super::call(
        state,
        &conn,
        reqwest::Method::GET,
        "skills",
        &[("kind", kind.to_owned())],
        None,
        60,
    )
    .await?;
    let root = state.data_dir.clone();
    let kind = kind.to_owned();
    tokio::task::spawn_blocking(move || freeze_plugin_snapshot(&root, &kind, value))
        .await
        .map_err(plugin::join_err)?
}
fn save(root: &Path, skill: Skill) -> AppResult<Skill> {
    store::write(
        &store::path(root, "skills", &skill.id)?,
        &serde_json::to_value(&skill)?,
    )?;
    Ok(skill)
}
fn collect(dir: &Path, base: &Path, out: &mut Vec<std::path::PathBuf>) -> AppResult<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_symlink() {
            continue;
        }
        let p = e.path();
        if t.is_dir() {
            if p.strip_prefix(base).unwrap().components().count() > 4 {
                continue;
            }
            collect(&p, base, out)?;
        } else if t.is_file()
            && p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("md"))
        {
            out.push(p);
        }
        if out.len() > 64 {
            return Err(AppError::bad_request("技能目录最多包含64份 Markdown"));
        }
    }
    Ok(())
}
pub fn import(root: &Path, req: ImportRequest) -> AppResult<Skill> {
    let source = Path::new(&req.path).canonicalize()?;
    let mut files = vec![];
    if source.is_dir() {
        collect(&source, &source, &mut files)?;
        files.sort();
    } else {
        files.push(source.clone());
    }
    if files.is_empty() {
        return Err(AppError::bad_request("未找到 Markdown 技能资料"));
    }
    let mut text = String::new();
    for p in files {
        if fs::metadata(&p)?.len() > MAX_BYTES as u64 {
            return Err(AppError::bad_request("技能文件过大"));
        }
        let label = p
            .strip_prefix(&source)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("SKILL.md"));
        let mut raw = Vec::new();
        fs::File::open(&p)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut raw)?;
        if raw.len() > MAX_BYTES {
            return Err(AppError::bad_request("技能文件读取时超过限制"));
        }
        let contents = std::str::from_utf8(&raw)
            .map_err(|_| AppError::bad_request("技能文件必须为 UTF-8 Markdown"))?;
        text.push_str(&format!("\n\n--- {} ---\n", label.display()));
        text.push_str(contents);
        if text.len() > MAX_BYTES {
            return Err(AppError::bad_request(
                "完整技能资料超过 768 KiB，未截断、未保存",
            ));
        }
    }
    let name = req
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            source
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let version = format!("local-{}", &store::hash(text.as_bytes())[..12]);
    save(
        root,
        make(
            name,
            version,
            text,
            "本机导入的 Markdown 材料".into(),
            false,
        )?,
    )
}
pub fn delete(root: &Path, id: &str) -> AppResult<()> {
    if id == "plugin-active" || id.starts_with(PLUGIN_SNAPSHOT_PREFIX) {
        return Err(AppError::bad_request(
            "已冻结的插件资料供旧实验重试使用，不能从普通资料列表删除",
        ));
    }
    if id == "nai5-prompting" {
        return Err(AppError::bad_request("内置技能不能删除"));
    }
    let p = store::path(root, "skills", id)?;
    if p.is_file() {
        fs::remove_file(p)?;
    }
    Ok(())
}
pub async fn fetch(state: &AppState, id: &str) -> AppResult<Skill> {
    if id != "npe" {
        return Err(AppError::bad_request("未知的固定版本技能来源"));
    }
    let mut text=String::from("NPE 固定版本材料包。包含主说明与设计参考；不包含可选画师库。为可复现实验一次提供所列全文，不执行工具调用。画风仍由应用指定。\n");
    for file in NPE_FILES {
        let url=format!("https://raw.githubusercontent.com/RefirmSaky/novelai-prompt-design-enhancer/{NPE_REVISION}/.agents/skills/npe/{file}");
        let mut resp = state
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?;
        if resp.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
            return Err(AppError::upstream("技能来源文件超出限制"));
        }
        let mut raw = Vec::new();
        while let Some(chunk) = resp.chunk().await? {
            if raw.len() + chunk.len() > MAX_BYTES {
                return Err(AppError::upstream("技能来源文件读取时超出限制"));
            }
            raw.extend_from_slice(&chunk);
        }
        text.push_str(&format!("\n\n--- {file} ---\n"));
        text.push_str(
            std::str::from_utf8(&raw).map_err(|_| AppError::upstream("技能文件不是 UTF-8"))?,
        );
        if text.len() > MAX_BYTES {
            return Err(AppError::upstream("完整技能超过限制，未截断"));
        }
    }
    save(
        &state.data_dir,
        make(
            "NPE · 完整设计参考".into(),
            NPE_REVISION.into(),
            text,
            "https://github.com/RefirmSaky/novelai-prompt-design-enhancer".into(),
            false,
        )?,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plugin_fixture(kind: &str, selected_id: &str, version: &str, text: &str) -> Value {
        json!({
            "version": 1,
            "kind": kind,
            "enabled": true,
            "selected_id": selected_id,
            "skill": {
                "id": selected_id,
                "name": "合成资料 / synthetic",
                "version": version,
                "sha256": store::hash(text.as_bytes()),
                "text": text,
                "source": "synthetic fixture",
                "builtin": selected_id == "builtin",
                "bytes": text.len()
            }
        })
    }

    #[test]
    fn plugin_versions_freeze_once_and_retries_keep_original_full_text() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = plugin_fixture("draw", "selected", "rev-one", "全文\nfirst version");
        let first = freeze_plugin_snapshot(dir.path(), "draw", v1.clone()).unwrap();
        let first_skill = first.skill.as_ref().unwrap();
        assert!(first_skill.id.starts_with(PLUGIN_SNAPSHOT_PREFIX));
        assert_eq!(first.selected_id, "selected");
        assert_eq!(first.version, 1);
        assert_eq!(first_skill.text, v1["skill"]["text"].as_str().unwrap());
        let again = freeze_plugin_snapshot(dir.path(), "draw", v1).unwrap();
        assert_eq!(again.skill.unwrap().id, first_skill.id);
        let v2 = plugin_fixture("draw", "selected", "rev-two", "全文\nsecond version");
        let second = freeze_plugin_snapshot(dir.path(), "draw", v2)
            .unwrap()
            .skill
            .unwrap();
        assert_ne!(first_skill.id, second.id);
        let same_text_new_revision = freeze_plugin_snapshot(
            dir.path(),
            "draw",
            plugin_fixture("draw", "selected", "rev-three", &second.text),
        )
        .unwrap()
        .skill
        .unwrap();
        assert_ne!(second.id, same_text_new_revision.id);
        let rows = bundles(dir.path(), &[first_skill.id.clone(), second.id.clone()]).unwrap();
        assert_eq!(rows[0].text, "全文\nfirst version");
        assert_eq!(rows[0].version, "rev-one");
        assert_eq!(rows[1].text, "全文\nsecond version");
        assert_eq!(rows[1].version, "rev-two");
        assert_eq!(list(dir.path()).unwrap().len(), 1);
        assert!(delete(dir.path(), &first_skill.id).is_err());
        assert_eq!(
            bundles(dir.path(), &[first_skill.id.clone()]).unwrap()[0].sha256,
            first_skill.sha256
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = store::path(dir.path(), "plugin_skills", &first_skill.id).unwrap();
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn disabled_reverse_is_explicit_empty_and_never_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let old = freeze_plugin_snapshot(
            dir.path(),
            "reverse",
            plugin_fixture("reverse", "chosen", "rev", "old reverse"),
        )
        .unwrap();
        let disabled =
            json!({"version":1,"kind":"reverse","enabled":false,"selected_id":"","skill":null});
        let response = freeze_plugin_snapshot(dir.path(), "reverse", disabled.clone()).unwrap();
        assert!(!response.enabled);
        assert!(response.skill.is_none());
        assert_eq!(response.selected_id, "");
        assert!(bundles(dir.path(), &[]).unwrap().is_empty());
        let mut missing_skill = disabled.clone();
        missing_skill.as_object_mut().unwrap().remove("skill");
        assert!(freeze_plugin_snapshot(dir.path(), "reverse", missing_skill).is_err());
        assert_eq!(
            bundles(dir.path(), &[old.skill.unwrap().id]).unwrap()[0].text,
            "old reverse"
        );
        let mut disabled_draw = disabled;
        disabled_draw["kind"] = json!("draw");
        assert!(freeze_plugin_snapshot(dir.path(), "draw", disabled_draw).is_err());
    }

    #[test]
    fn invalid_plugin_response_cannot_return_an_older_cached_skill() {
        let dir = tempfile::tempdir().unwrap();
        let valid = plugin_fixture("draw", "builtin", "fixture-revision", "中英 full text");
        let old = freeze_plugin_snapshot(dir.path(), "draw", valid.clone()).unwrap();
        for (path, bad) in [
            ("version", json!(2)),
            ("kind", json!("reverse")),
            ("enabled", json!(false)),
            ("selected_id", json!("different")),
            ("skill", Value::Null),
            ("skill.bytes", json!(1)),
            ("skill.sha256", json!("0".repeat(64))),
            ("skill.version", json!("")),
            ("skill.text", json!("")),
            ("skill.id", json!("different")),
        ] {
            let mut broken = valid.clone();
            if let Some(key) = path.strip_prefix("skill.") {
                broken["skill"][key] = bad;
            } else {
                broken[path] = bad;
            }
            let error = freeze_plugin_snapshot(dir.path(), "draw", broken).unwrap_err();
            assert_eq!(error.code, "studio_protocol", "{path}");
        }
        for empty in [Value::Null, json!({}), json!([])] {
            assert!(freeze_plugin_snapshot(dir.path(), "draw", empty).is_err());
        }
        let mut missing = valid.clone();
        missing["skill"].as_object_mut().unwrap().remove("bytes");
        assert!(freeze_plugin_snapshot(dir.path(), "draw", missing).is_err());
        let text = "x".repeat(MAX_BYTES + 1);
        assert!(freeze_plugin_snapshot(
            dir.path(),
            "draw",
            plugin_fixture("draw", "large", "rev", &text)
        )
        .is_err());
        assert_eq!(
            fs::read_dir(dir.path().join("studio/plugin_skills"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            bundles(dir.path(), &[old.skill.unwrap().id]).unwrap()[0].text,
            "中英 full text"
        );
        assert!(valid_plugin_kind("gallery").is_err());
    }

    #[test]
    fn plugin_snapshot_corruption_and_unresolved_alias_fail_explicitly() {
        let dir = tempfile::tempdir().unwrap();
        let frozen = freeze_plugin_snapshot(
            dir.path(),
            "draw",
            plugin_fixture("draw", "selected", "rev", "frozen text"),
        )
        .unwrap();
        let id = frozen.skill.unwrap().id;
        let path = store::path(dir.path(), "plugin_skills", &id).unwrap();
        let mut tampered = store::read(&path).unwrap();
        tampered["skill"]["version"] = json!("different version with same text");
        store::write(&path, &tampered).unwrap();
        assert_eq!(
            bundles(dir.path(), &[id.clone()]).unwrap_err().code,
            "studio_protocol"
        );
        fs::remove_file(path).unwrap();
        assert_eq!(bundles(dir.path(), &[id]).unwrap_err().code, "not_found");
        assert_eq!(
            bundles(dir.path(), &["plugin-active".into()])
                .unwrap_err()
                .code,
            "bad_request"
        );
        assert!(bundles(dir.path(), &["plugin-skill-../../escape".into()]).is_err());
    }

    #[test]
    fn simultaneous_draw_reverse_snapshots_do_not_share_mutable_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (draw, reverse) = std::thread::scope(|scope| {
            let draw = scope.spawn(|| {
                freeze_plugin_snapshot(
                    root,
                    "draw",
                    plugin_fixture("draw", "same", "same-rev", "shared text"),
                )
                .unwrap()
            });
            let reverse = scope.spawn(|| {
                freeze_plugin_snapshot(
                    root,
                    "reverse",
                    plugin_fixture("reverse", "same", "same-rev", "shared text"),
                )
                .unwrap()
            });
            (draw.join().unwrap(), reverse.join().unwrap())
        });
        let draw_id = draw.skill.unwrap().id;
        let reverse_id = reverse.skill.unwrap().id;
        assert_ne!(draw_id, reverse_id);
        assert_eq!(bundles(root, &[draw_id, reverse_id]).unwrap().len(), 2);
    }

    #[test]
    fn frozen_plugin_and_fixed_bundles_preserve_order_and_combined_limit() {
        let dir = tempfile::tempdir().unwrap();
        let frozen = freeze_plugin_snapshot(
            dir.path(),
            "draw",
            plugin_fixture("draw", "selected", "rev", "plugin full text"),
        )
        .unwrap()
        .skill
        .unwrap();
        let bundles =
            super::bundles(dir.path(), &[frozen.id.clone(), "nai5-prompting".into()]).unwrap();
        assert_eq!(bundles[0].id, frozen.id);
        assert_eq!(bundles[1].id, "nai5-prompting");
        assert!(super::bundles(dir.path(), &[frozen.id.clone(), frozen.id]).is_err());
        let text = "x".repeat(600 * 1024);
        let first = freeze_plugin_snapshot(
            dir.path(),
            "draw",
            plugin_fixture("draw", "large1", "rev", &text),
        )
        .unwrap()
        .skill
        .unwrap();
        let second = freeze_plugin_snapshot(
            dir.path(),
            "draw",
            plugin_fixture("draw", "large2", "rev", &text),
        )
        .unwrap()
        .skill
        .unwrap();
        assert!(super::bundles(dir.path(), &[first.id, second.id])
            .unwrap_err()
            .message
            .contains("1 MiB"));
    }

    #[test]
    fn plugin_generated_wire_fixtures_match_the_desktop_snapshot_contract() {
        let Some(fixtures) = std::env::var_os("STUDIO_SKILL_FIXTURE_DIR") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let mut frozen = vec![];
        for (name, kind, enabled) in [
            ("builtin", "draw", true),
            ("disabled", "reverse", false),
            ("selected", "draw", true),
            ("selected_reverse", "reverse", true),
            ("changed", "draw", true),
        ] {
            let payload: Value = serde_json::from_slice(
                &fs::read(Path::new(&fixtures).join(format!("{name}.json"))).unwrap(),
            )
            .unwrap();
            let response = freeze_plugin_snapshot(dir.path(), kind, payload.clone()).unwrap();
            assert_eq!(response.enabled, enabled);
            assert_eq!(response.selected_id, payload["selected_id"]);
            if let Some(skill) = response.skill {
                let loaded = bundles(dir.path(), &[skill.id.clone()]).unwrap();
                assert_eq!(loaded[0].text, payload["skill"]["text"]);
                assert_eq!(loaded[0].sha256, payload["skill"]["sha256"]);
                assert_eq!(loaded[0].version, payload["skill"]["version"]);
                frozen.push(skill.id);
            }
        }
        assert_eq!(
            frozen
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn imports_freeze_full_files_and_preserve_builtin() {
        let d = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("SKILL.md"), "Synthetic skill").unwrap();
        fs::create_dir(src.path().join("references")).unwrap();
        fs::write(src.path().join("references/a.md"), "Synthetic reference").unwrap();
        let s = import(
            d.path(),
            ImportRequest {
                path: src.path().display().to_string(),
                name: Some("fixture".into()),
            },
        )
        .unwrap();
        assert!(s.text.contains("Synthetic reference"));
        assert_eq!(list(d.path()).unwrap().len(), 2);
        assert_eq!(bundles(d.path(), &[]).unwrap().len(), 0);
        assert!(delete(d.path(), "nai5-prompting").is_err());
        let before = s.sha256.clone();
        fs::write(src.path().join("SKILL.md"), "changed").unwrap();
        assert_eq!(
            bundles(d.path(), &[s.id.clone()]).unwrap()[0].sha256,
            before
        );
        delete(d.path(), &s.id).unwrap();
        assert_eq!(list(d.path()).unwrap().len(), 1);
    }
}
