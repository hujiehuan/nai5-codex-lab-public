use std::path::{Path, PathBuf};
use crate::{db::Db, error::AppResult};

/// A separate, uniquely named consistency snapshot; never prunes user backups.
pub fn before_install(db: &Db, data_dir: &Path) -> AppResult<PathBuf> {
    db.backup_into(&data_dir.join("backups"), Some(&format!("preupdate-{}", uuid::Uuid::new_v4())), usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrade_backup_preserves_every_decision_and_does_not_replace_live_data() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("library.db")).unwrap();
        db.with(|c| {
            let tx = c.transaction()?;
            for (book, count) in [("artist", 1313), ("pack", 1878)] {
                tx.execute("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES (?1,?1,'fixture','1','[]')", [book])?;
                for idx in 0..count {
                    let id = format!("{book}-{idx}");
                    tx.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release) VALUES (?1,?2,?3,'synthetic','[]','synthetic','safe','1','1')", rusqlite::params![id,book,idx])?;
                    if idx < 251 {
                        tx.execute("INSERT INTO library_membership(entry_id,library,decision,note,decided_at) VALUES (?1,?2,'include',?3,'2026-09-21T00:00:00Z')", rusqlite::params![id,book,format!("人工备注-{idx}")])?;
                    }
                }
            }
            tx.commit()?; Ok(())
        }).unwrap();
        let backup = before_install(&db, root.path()).unwrap();
        let saved = rusqlite::Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let count: i64 = saved.query_row("SELECT COUNT(*) FROM library_membership WHERE decision='include'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 502);
        let source: String = db.with(|c| Ok(c.query_row("SELECT group_concat(entry_id || library || decision || note || decided_at, '|') FROM (SELECT * FROM library_membership ORDER BY entry_id)", [], |r| r.get(0))?)).unwrap();
        let copied: String = saved.query_row("SELECT group_concat(entry_id || library || decision || note || decided_at, '|') FROM (SELECT * FROM library_membership ORDER BY entry_id)", [], |r| r.get(0)).unwrap();
        assert_eq!(source, copied);
        db.with(|c| { c.execute("UPDATE library_membership SET decision='exclude' WHERE entry_id='pack-0'", [])?; Ok(()) }).unwrap();
        assert_eq!(saved.query_row::<String,_,_>("SELECT decision FROM library_membership WHERE entry_id='pack-0'", [], |r| r.get(0)).unwrap(), "include");
        assert_eq!(db.with(|c| Ok(c.query_row::<String,_,_>("SELECT decision FROM library_membership WHERE entry_id='pack-0'", [], |r| r.get(0))?)).unwrap(), "exclude");
    }
    #[test]
    fn failed_backup_is_an_error_and_keeps_the_database() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("library.db")).unwrap();
        db.app_state_set("sentinel", "unchanged").unwrap();
        std::fs::write(root.path().join("backups"), "not a directory").unwrap();
        assert!(before_install(&db, root.path()).is_err());
        assert_eq!(db.app_state_get("sentinel").unwrap().as_deref(), Some("unchanged"));
    }
}
