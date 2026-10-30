//! Private JSON records. No library schema or membership writes.
use crate::error::{AppError, AppResult};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn valid_id(id: &str) -> AppResult<()> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AppError::bad_request("实验记录 ID 不合法"));
    }
    Ok(())
}
pub fn path(root: &Path, kind: &str, id: &str) -> AppResult<PathBuf> {
    valid_id(kind)?;
    valid_id(id)?;
    Ok(root.join("studio").join(kind).join(format!("{id}.json")))
}
pub fn write(path: &Path, value: &Value) -> AppResult<()> {
    bytes(path, &serde_json::to_vec(value)?)
}
/// Atomically create a frozen snapshot. A retry may confirm the same snapshot,
/// but must never replace another request/plan that has already been persisted.
pub fn write_once(path: &Path, value: &Value) -> AppResult<()> {
    let data = serde_json::to_vec(value)?;
    publish(path, &data, true)
}
pub fn bytes(path: &Path, data: &[u8]) -> AppResult<()> {
    publish(path, data, false)
}
fn publish(path: &Path, data: &[u8], immutable: bool) -> AppResult<()> {
    fs::create_dir_all(path.parent().ok_or_else(|| AppError::io("没有记录目录"))?)?;
    let temp = path.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> AppResult<()> {
        let mut opts = fs::OpenOptions::new();
        opts.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&temp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        if immutable {
            match fs::hard_link(&temp, path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if read(path)? != serde_json::from_slice::<Value>(data)? {
                        return Err(AppError::new(
                            "studio_snapshot_conflict",
                            "冻结记录已存在且内容不同，未覆盖原记录",
                        ));
                    }
                }
                Err(e) => return Err(e.into()),
            }
            fs::remove_file(&temp)?;
        } else {
            fs::rename(&temp, path)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn read(path: &Path) -> AppResult<Value> {
    if !path.is_file() {
        return Err(AppError::not_found("本地实验记录不存在"));
    }
    // These are our private snapshots. Full reference text must remain readable
    // after it has been accepted and saved, regardless of its aggregate size.
    let data = fs::read(path)?;
    serde_json::from_slice(&data).map_err(|_| AppError::io("本地实验记录损坏，未覆盖原文件"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_rejected_and_replacement_atomic() {
        for id in ["", "../x", "a/b", "a.json", "/tmp"] {
            assert!(valid_id(id).is_err());
        }
        let d = tempfile::tempdir().unwrap();
        let p = path(d.path(), "runs", "abc-1").unwrap();
        write(&p, &serde_json::json!({"a":1})).unwrap();
        write(&p, &serde_json::json!({"a":2})).unwrap();
        assert_eq!(read(&p).unwrap()["a"], 2);
        fs::write(&p, b"bad").unwrap();
        assert!(read(&p).is_err());
    }
    #[test]
    fn frozen_snapshot_cannot_be_replaced_even_by_retry() {
        let d = tempfile::tempdir().unwrap();
        let p = path(d.path(), "inputs", "same").unwrap();
        let first = serde_json::json!({"requirements":"original", "seed":123});
        write_once(&p, &first).unwrap();
        write_once(&p, &first).unwrap();
        assert_eq!(
            write_once(&p, &serde_json::json!({"requirements":"changed"}))
                .unwrap_err()
                .code,
            "studio_snapshot_conflict"
        );
        assert_eq!(read(&p).unwrap(), first);
        assert_eq!(fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
        assert!(path(d.path(), "../elsewhere", "id").is_err());
    }
    #[test]
    fn full_snapshot_larger_than_previous_read_cap_is_restored_without_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let p = path(dir.path(), "inputs", "large").unwrap();
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut file = fs::File::create(&p).unwrap();
        file.write_all(b"{\"references\":[{\"tags\":\"begin")
            .unwrap();
        let block = vec![b'x'; 1024 * 1024];
        for _ in 0..64 {
            file.write_all(&block).unwrap();
        }
        file.write_all(b"end\"}]}").unwrap();
        drop(file);
        assert!(fs::metadata(&p).unwrap().len() > 64 * 1024 * 1024);
        let restored = read(&p).unwrap();
        let text = restored["references"][0]["tags"].as_str().unwrap();
        assert_eq!(text.len(), 64 * 1024 * 1024 + 8);
        assert!(text.starts_with("begin") && text.ends_with("end"));
        assert!(text.as_bytes()[5..text.len() - 3]
            .iter()
            .all(|c| *c == b'x'));
    }
}
