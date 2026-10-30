//! Immutable reference images for Studio. No downloads, thumbnails, or library writes.
use super::store;
use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 32 * 1024 * 1024;

pub(super) struct Source {
    entry_id: String,
    raw_tag: Option<String>,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    entry_id: String,
    ordinal: u32,
    mime: String,
    sha256: String,
    raw_tag: Option<String>,
    bytes: u64,
}

fn invalid(id: &str, reason: &str) -> AppError {
    AppError::new(
        "studio_reference_image",
        format!("图包 {id} 的参考图{reason}；未删减参考，也未调用模型"),
    )
}

/// Resolve exactly one primary reference per selected text entry. All paths come
/// from the existing local library, never from the browser or a remote URL.
pub(super) fn sources(conn: &rusqlite::Connection, refs: &[Value]) -> AppResult<Vec<Source>> {
    let mut statement = conn.prepare(
        "SELECT original_local_path, local_path, raw_tag FROM entry_image WHERE entry_id=?1 AND ordinal=0",
    )?;
    let mut result = Vec::with_capacity(refs.len());
    for reference in refs {
        let id = super::field(reference, "entry_id")?;
        let row: Option<(Option<String>, Option<String>, Option<String>)> = statement
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let (original, display, raw_tag) =
            row.ok_or_else(|| invalid(id, "尚未下载，请先在图库同步或下载展示图"))?;
        let path = display
            .iter()
            .chain(original.iter())
            .map(PathBuf::from)
            .find(|p| p.is_absolute() && p.is_file())
            .ok_or_else(|| invalid(id, "不在本地，请先在图库同步或下载展示图"))?;
        result.push(Source {
            entry_id: id.to_owned(),
            raw_tag,
            path,
        });
    }
    Ok(result)
}

fn extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn read_bytes(path: &Path, id: &str) -> AppResult<Vec<u8>> {
    let file = fs::File::open(path).map_err(|_| invalid(id, "无法读取"))?;
    if file.metadata().map_err(|_| invalid(id, "无法读取"))?.len() > MAX_IMAGE_BYTES {
        return Err(invalid(id, "超过单张图片 12 MiB 校验上限"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid(id, "无法读取"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(invalid(id, "超过单张图片 12 MiB 校验上限"));
    }
    Ok(bytes)
}

fn inspect(bytes: &[u8], id: &str) -> AppResult<&'static str> {
    let format = image::guess_format(bytes).map_err(|_| invalid(id, "格式无效"))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => return Err(invalid(id, "仅支持 PNG、JPEG、WebP")),
    };
    let dimensions = image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| invalid(id, "尺寸无法读取"))?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_IMAGE_PIXELS
    {
        return Err(invalid(id, "超过单张图片 32M 像素校验上限"));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| invalid(id, "损坏或无法解码"))?;
    Ok(mime)
}

fn directory(root: &Path, input_id: &str) -> AppResult<PathBuf> {
    store::valid_id(input_id)?;
    Ok(root.join("studio/reference_images").join(input_id))
}

/// Copy bytes unchanged, one file at a time. Publishing the directory only after
/// every image validates prevents incomplete inputs becoming usable plans.
pub(super) fn freeze(
    root: &Path,
    input_id: &str,
    sources: Vec<Source>,
) -> AppResult<Vec<Snapshot>> {
    let destination = directory(root, input_id)?;
    let parent = destination.parent().unwrap();
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{input_id}-{}.part", uuid::Uuid::new_v4()));
    fs::create_dir(&temporary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    }
    let result = (|| {
        let mut snapshots = Vec::with_capacity(sources.len());
        let mut written = HashSet::new();
        for source in sources {
            let bytes = read_bytes(&source.path, &source.entry_id)?;
            let mime = inspect(&bytes, &source.entry_id)?;
            let sha256 = store::hash(&bytes);
            if written.insert(sha256.clone()) {
                let path = temporary.join(format!("{sha256}.{}", extension(mime).unwrap()));
                let mut options = fs::OpenOptions::new();
                options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(path)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
            }
            snapshots.push(Snapshot {
                entry_id: source.entry_id,
                ordinal: 0,
                mime: mime.into(),
                sha256,
                raw_tag: source.raw_tag,
                bytes: bytes.len() as u64,
            });
        }
        if destination.exists() {
            return Err(AppError::new(
                "studio_snapshot_conflict",
                "参考图片快照已存在，未覆盖原记录",
            ));
        }
        fs::rename(&temporary, &destination)?;
        Ok(snapshots)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

/// Verify the exact frozen images for every model/skill row or explicit retry.
/// Only lightweight manifests leave this loop, so a full pool is never encoded
/// into one enormous HTTP body. The current library is never consulted here.
pub(super) fn manifest(
    root: &Path,
    input_id: &str,
    saved: &Value,
    refs: &[Value],
) -> AppResult<Vec<Value>> {
    let metadata = saved.get("reference_image_snapshots").ok_or_else(|| {
        AppError::new(
            "studio_reference_images_required",
            "旧实验输入没有冻结参考图片，请重新构建对照方案；已有方案与图片仍可查看",
        )
    })?;
    let snapshots: Vec<Snapshot> = serde_json::from_value(metadata.clone())
        .map_err(|_| super::protocol("冻结参考图片记录损坏"))?;
    if snapshots.len() != refs.len() || snapshots.is_empty() {
        return Err(super::protocol("冻结参考图片数量与图包不一致，未调用模型"));
    }
    let dir = directory(root, input_id)?;
    let mut wire = Vec::with_capacity(snapshots.len());
    let mut ids = HashSet::new();
    for (snapshot, reference) in snapshots.into_iter().zip(refs) {
        let id = super::field(reference, "entry_id")?;
        let ext = extension(&snapshot.mime).ok_or_else(|| invalid(id, "快照 MIME 无效"))?;
        if snapshot.entry_id != id
            || snapshot.ordinal != 0
            || !ids.insert(id)
            || snapshot.sha256.len() != 64
            || !snapshot.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            || snapshot.bytes > MAX_IMAGE_BYTES
        {
            return Err(invalid(id, "快照来源、序号或摘要无效"));
        }
        let bytes = read_bytes(&dir.join(format!("{}.{}", snapshot.sha256, ext)), id)?;
        if bytes.len() as u64 != snapshot.bytes || store::hash(&bytes) != snapshot.sha256 {
            return Err(invalid(id, "快照摘要不匹配，请重新构建对照方案"));
        }
        wire.push(json!({
            "entry_id": id, "ordinal": snapshot.ordinal, "mime": snapshot.mime,
            "sha256": snapshot.sha256, "raw_tag": snapshot.raw_tag,
        }));
    }
    Ok(wire)
}

/// Encode one already validated immutable image only when the plugin cache
/// misses it. Recheck the hash so local corruption cannot be uploaded silently.
pub(super) fn upload(root: &Path, input_id: &str, manifest: &Value) -> AppResult<(Value, u64)> {
    let id = super::field(manifest, "entry_id")?;
    let mime = super::field(manifest, "mime")?;
    let hash = super::field(manifest, "sha256")?;
    let ext = extension(mime).ok_or_else(|| invalid(id, "快照 MIME 无效"))?;
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid(id, "快照摘要无效"));
    }
    let bytes = read_bytes(
        &directory(root, input_id)?.join(format!("{hash}.{ext}")),
        id,
    )?;
    if store::hash(&bytes) != hash {
        return Err(invalid(id, "快照摘要不匹配，请重新构建对照方案"));
    }
    let size = bytes.len() as u64;
    Ok((
        json!({"sha256":hash,"mime":mime,"data_base64":STANDARD.encode(bytes)}),
        size,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(color: u8) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            3,
            2,
            image::Rgb([color, 42, 77]),
        ))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
        out.into_inner()
    }

    fn fixture(
        conn: &rusqlite::Connection,
        id: &str,
        display: Option<&Path>,
        original: Option<&Path>,
        raw: Option<&str>,
    ) {
        conn.execute("INSERT OR IGNORE INTO codex_book(codex_id,type,title,version,tree_json) VALUES('nai5_community_pack','string','synthetic','1','[]')", []).unwrap();
        conn.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release) VALUES(?1,'nai5_community_pack',0,?1,'[]','full original text','safe','1','1')", [id]).unwrap();
        conn.execute("INSERT INTO library_membership(entry_id,library,decision,decided_at,note) VALUES(?1,'pack','include','now','human choice')", [id]).unwrap();
        conn.execute("INSERT INTO entry_image(entry_id,ordinal,image_file,local_path,original_local_path,raw_tag) VALUES(?1,0,'fixture.png',?2,?3,?4)", rusqlite::params![id, display.map(|p|p.to_str().unwrap()), original.map(|p|p.to_str().unwrap()),raw]).unwrap();
    }

    #[test]
    fn primary_display_and_matching_raw_tag_survive_source_change_and_restore() {
        let temp = tempfile::tempdir().unwrap();
        let display = temp.path().join("display.dat");
        let original = temp.path().join("original.png");
        let bytes = png(10);
        fs::write(&display, &bytes).unwrap();
        fs::write(&original, png(200)).unwrap();
        let db = crate::db::Db::open_memory().unwrap();
        let refs = vec![json!({"entry_id":"entry","tags":"full original text"})];
        let raw = "  exact image tags\nline two  ";
        let selected = db.with(|c| {
            fixture(c,"entry",Some(&display),Some(&original),Some(raw));
            c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file,local_path,raw_tag) VALUES('entry',1,'other.png',?1,'wrong image tags')", [original.to_str().unwrap()])?;
            sources(c, &refs)
        }).unwrap();
        let snapshots = freeze(temp.path(), "input1", selected).unwrap();
        let saved = json!({"reference_image_snapshots":snapshots});
        let record = store::path(temp.path(), "inputs", "input1").unwrap();
        store::write_once(&record, &saved).unwrap();
        fs::remove_file(&display).unwrap();
        fs::write(&original, b"now corrupted").unwrap();
        let restored = store::read(&record).unwrap();
        let images = manifest(temp.path(), "input1", &restored, &refs).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0],
            json!({"entry_id":"entry","ordinal":0,"mime":"image/png","sha256":store::hash(&bytes),"raw_tag":raw})
        );
        assert!(images[0].get("data_base64").is_none());
        let (request, size) = upload(temp.path(), "input1", &images[0]).unwrap();
        assert_eq!(
            STANDARD
                .decode(request["data_base64"].as_str().unwrap())
                .unwrap(),
            bytes
        );
        assert_eq!(size, bytes.len() as u64);
        db.with(|c| {
            let choice: (String, String) = c.query_row(
                "SELECT decision,note FROM library_membership WHERE entry_id='entry'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(choice, ("include".into(), "human choice".into()));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn complete_1367_entry_manifest_keeps_every_pair_without_embedding_images_in_plan() {
        let temp = tempfile::tempdir().unwrap();
        let display = temp.path().join("fixture.png");
        let bytes = png(23);
        fs::write(&display, &bytes).unwrap();
        let refs: Vec<_> = (0..1367)
            .map(|i| json!({"entry_id":format!("synthetic-{i}"),"tags":format!("full text {i}")}))
            .collect();
        let selected = (0..1367)
            .map(|i| Source {
                entry_id: format!("synthetic-{i}"),
                raw_tag: Some(format!("raw image tags {i}")),
                path: display.clone(),
            })
            .collect();
        let snapshots = freeze(temp.path(), "large", selected).unwrap();
        assert_eq!(snapshots.len(), 1367);
        let saved = json!({"reference_image_snapshots":snapshots});
        let images = manifest(temp.path(), "large", &saved, &refs).unwrap();
        assert_eq!(images.len(), 1367);
        for (idx, image) in images.iter().enumerate() {
            assert_eq!(image["entry_id"], refs[idx]["entry_id"]);
            assert_eq!(image["raw_tag"], format!("raw image tags {idx}"));
            assert_eq!(image["sha256"], store::hash(&bytes));
        }
        // Identical fixture bytes share local storage, but remain one distinct
        // reference attachment per entry in the ordered model manifest.
        assert_eq!(
            fs::read_dir(directory(temp.path(), "large").unwrap())
                .unwrap()
                .count(),
            1
        );
        assert!(serde_json::to_string(&images).unwrap().len() < 1024 * 1024);
    }

    #[test]
    fn original_fallback_is_valid_but_thumbnails_or_secondary_images_are_never_substituted() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original.png");
        fs::write(&original, png(9)).unwrap();
        let db = crate::db::Db::open_memory().unwrap();
        db.with(|c| {
            fixture(c,"fallback",None,Some(&original),None);
            assert_eq!(sources(c,&[json!({"entry_id":"fallback"})])?.remove(0).path,original);
            fixture(c,"missing",None,None,None);
            c.execute("UPDATE entry_image SET thumb_path=?1 WHERE entry_id='missing'",[original.to_str().unwrap()])?;
            c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file,local_path) VALUES('missing',1,'other.png',?1)",[original.to_str().unwrap()])?;
            let error=sources(c,&[json!({"entry_id":"missing"})]).err().unwrap();
            assert_eq!(error.code,"studio_reference_image");
            assert!(error.message.contains("missing"));
            Ok(())
        }).unwrap();
    }

    #[test]
    fn corrupted_or_oversized_images_abort_the_entire_snapshot_and_clean_partial_files() {
        let temp = tempfile::tempdir().unwrap();
        let valid = temp.path().join("good.png");
        let invalid_file = temp.path().join("bad.png");
        fs::write(&valid, png(7)).unwrap();
        fs::write(&invalid_file, b"corrupt").unwrap();
        let selected = vec![
            Source {
                entry_id: "good".into(),
                path: valid,
                raw_tag: None,
            },
            Source {
                entry_id: "bad".into(),
                path: invalid_file.clone(),
                raw_tag: None,
            },
        ];
        assert!(freeze(temp.path(), "broken", selected)
            .unwrap_err()
            .message
            .contains("bad"));
        assert_eq!(
            fs::read_dir(temp.path().join("studio/reference_images"))
                .unwrap()
                .count(),
            0
        );
        let file = fs::File::create(&invalid_file).unwrap();
        file.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        assert!(read_bytes(&invalid_file, "large")
            .unwrap_err()
            .message
            .contains("12 MiB"));
    }

    #[test]
    fn missing_legacy_snapshot_mismatched_identity_and_tampering_never_produce_text_only_input() {
        let temp = tempfile::tempdir().unwrap();
        let refs = vec![json!({"entry_id":"one"})];
        assert_eq!(
            manifest(temp.path(), "old", &json!({}), &refs)
                .unwrap_err()
                .code,
            "studio_reference_images_required"
        );
        let display = temp.path().join("image.png");
        fs::write(&display, png(1)).unwrap();
        let snapshots = freeze(
            temp.path(),
            "one",
            vec![Source {
                entry_id: "one".into(),
                raw_tag: None,
                path: display,
            }],
        )
        .unwrap();
        let saved = json!({"reference_image_snapshots":snapshots});
        assert!(manifest(temp.path(), "one", &saved, &[json!({"entry_id":"another"})]).is_err());
        assert!(manifest(temp.path(), "one", &saved, &[]).is_err());
        let file = directory(temp.path(), "one").unwrap().join(format!(
            "{}.png",
            saved["reference_image_snapshots"][0]["sha256"]
                .as_str()
                .unwrap()
        ));
        fs::write(file, png(2)).unwrap();
        assert!(manifest(temp.path(), "one", &saved, &refs)
            .unwrap_err()
            .message
            .contains("摘要不匹配"));
    }
}
