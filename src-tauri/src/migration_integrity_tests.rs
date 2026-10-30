//! Synthetic scale fixture only: no QuickTagCloud content or real user decisions.
use super::*;

fn snapshot(c:&Connection)->Vec<String>{
    let mut out=Vec::new();
    for sql in [
        "SELECT entry_id || '|' || library || '|' || decision || '|' || quote(note) || '|' || decided_at FROM library_membership ORDER BY entry_id",
        "SELECT id || '|' || entry_id || '|' || quote(from_decision) || '|' || to_decision || '|' || at || '|' || via FROM membership_audit ORDER BY id",
        "SELECT name || '|' || quote(reason) || '|' || added_at FROM artist_blacklist ORDER BY name",
        "SELECT key || '|' || value FROM app_state WHERE key='undone_audit_ids' ORDER BY key",
    ]{out.extend(c.prepare(sql).unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<Result<Vec<_>,_>>().unwrap());}
    out
}
fn seed(c:&Connection){
    let tx=c.unchecked_transaction().unwrap();
    for (id,n,library,includes) in [("artist_nai5_personal",1313,"artist",205),("nai5_community_pack",1878,"pack",297)]{
        let book_type = if library == "artist" { "string" } else { "pack" };
        tx.execute("INSERT INTO codex_book(codex_id,type,title,version,entry_count,tree_json) VALUES(?1,?3,'SYNTHETIC ONLY','fixture',?2,'[]')",params![id,n,book_type]).unwrap();
        for i in 0..n{
            let eid=format!("synthetic-{id}-{i}");
            tx.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release,removed_at) VALUES(?1,?2,?3,'SYNTHETIC ONLY','[]','fixture-token','safe','fixture','fixture',?4)",params![eid,id,i,if i%23==0{Some("2026-01-01")}else{None}]).unwrap();
            let decision=if i<includes{"include"}else if i%7==0{"exclude"}else{"undecided"};
            if i<includes||i%5!=0{
                let note=if i%13==0{None}else{Some(format!("fixture 备注🧪\n{}", "长文本".repeat(i as usize%11)))};
                tx.execute("INSERT INTO library_membership(entry_id,library,decision,note,decided_at) VALUES(?1,?2,?3,?4,'2026-01-01T00:00:00Z')",params![eid,library,decision,note]).unwrap();
                tx.execute("INSERT INTO membership_audit(entry_id,from_decision,to_decision,at,via) VALUES(?1,NULL,?2,'2026-01-01T00:00:00Z','fixture')",params![eid,decision]).unwrap();
            }
        }
    }
    tx.execute("INSERT INTO artist_blacklist VALUES('fixture-artist','合成黑名单','2026-01-01')",[]).unwrap();
    tx.execute("INSERT INTO app_state VALUES('undone_audit_ids','[1,3,7]')",[]).unwrap();tx.commit().unwrap();
}
#[test]
fn every_historical_version_preserves_3191_entries_and_502_includes(){
    for start in 1..=MIGRATIONS.len(){
        let c=Connection::open_in_memory().unwrap();c.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        apply_migrations(&c,&MIGRATIONS[..start]).unwrap();seed(&c);let before=snapshot(&c);
        for target in start..=MIGRATIONS.len(){
            apply_migrations(&c,&MIGRATIONS[..target]).unwrap();
            assert_eq!(snapshot(&c),before,"v{start} → v{target}: every decision, note, audit and undo marker must remain identical");
            assert_eq!(c.query_row("SELECT COUNT(*) FROM codex_entry",[],|r|r.get::<_,i64>(0)).unwrap(),3191);
            assert_eq!(c.query_row("SELECT COUNT(*) FROM library_membership WHERE decision='include'",[],|r|r.get::<_,i64>(0)).unwrap(),502);
            assert_eq!(c.query_row("PRAGMA integrity_check",[],|r|r.get::<_,String>(0)).unwrap(),"ok");
            assert!(!c.prepare("PRAGMA foreign_key_check").unwrap().exists([]).unwrap());
        }
        migrate(&c).unwrap();assert_eq!(snapshot(&c),before,"second migration is idempotent");
    }
}
