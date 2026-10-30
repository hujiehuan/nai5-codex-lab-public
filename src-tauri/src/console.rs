//! Plugin read bridges: the shared model shortlist and the reverse-run source image. No arbitrary URLs.
use std::{path::Path, time::Duration};
use crate::models::SourceImageResult as SourceImage;
use serde_json::Value;
use rusqlite::OptionalExtension;
use crate::{error::{AppError, AppResult}, state::AppState, plugin};

/// The only plugin route this bridge reads: the LLM shortlist shared by the plugin and the desktop (§17).
const MODEL_SELECTION_ENDPOINT: &str = "model-selection";

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_model_selection(state: tauri::State<'_, AppState>) -> AppResult<Value> {
    let _activity = state.activity.enter()?;
    let conn = plugin::plugin_conn_with_library(&state, false).await?;
    let path = format!("{}/{}", plugin::PLUGIN_PAGE_PATH, MODEL_SELECTION_ENDPOINT);
    let (status, text) = plugin::plugin_request(&state.http, &conn, reqwest::Method::GET, &path, &[], None, Duration::from_secs(90)).await?;
    decode(status, &text)
}

fn decode(status: reqwest::StatusCode, text: &str) -> AppResult<Value> {
    if !status.is_success() { return Err(AppError::plugin_rejected(format!("HTTP {status}: {}", plugin::truncate(text, 500)))); }
    let value: Value = serde_json::from_str(text).map_err(|_| AppError::upstream("插件返回的不是 JSON"))?;
    if value.get("status").and_then(Value::as_str) == Some("error") {
        return Err(AppError::plugin_rejected(value.get("message").and_then(Value::as_str).unwrap_or("插件拒绝请求")));
    }
    Ok(value.get("data").cloned().unwrap_or(value))
}

pub fn source_image(conn: &rusqlite::Connection, run_id: i64) -> AppResult<SourceImage> {
    let row = conn.query_row("SELECT source_kind, source_ref, COALESCE(image_ordinal,0) FROM reverse_run WHERE run_id=?1", [run_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?))).optional()?.ok_or_else(|| AppError::not_found("运行记录不存在"))?;
    let paths = if row.0 == "local_file" || row.0 == "local" {
        Some((Some(row.1), None, None))
    } else {
        conn.query_row("SELECT original_local_path, local_path, thumb_path FROM entry_image WHERE entry_id=?1 AND ordinal=?2", rusqlite::params![row.1,row.2], |r| Ok((r.get::<_,Option<String>>(0)?, r.get::<_,Option<String>>(1)?, r.get::<_,Option<String>>(2)?))).optional()?
    };
    if let Some((original,display,thumbnail)) = paths {
        for (path,kind) in [(original,"original"),(display,"display"),(thumbnail,"thumbnail")] {
            if let Some(path) = path.filter(|p| Path::new(p).is_file()) { return Ok(SourceImage { path: Some(path), kind: kind.into() }); }
        }
    }
    Ok(SourceImage { path: None, kind: "missing".into() })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reverse_source_image(state: tauri::State<'_, AppState>, run_id: i64) -> AppResult<SourceImage> {
    let _activity = state.activity.enter()?;
    crate::commands::blocking(state.db.clone(), move |c| source_image(c, run_id)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_selection_decodes_plugin_envelope_and_rejections() {
        assert_eq!(MODEL_SELECTION_ENDPOINT, "model-selection");
        assert!(decode(reqwest::StatusCode::OK, r#"{"status":"error","message":"拒绝"}"#).is_err());
        assert!(decode(reqwest::StatusCode::UNAUTHORIZED, r#"{"data":{}}"#).is_err());
        assert_eq!(decode(reqwest::StatusCode::OK, r#"{"status":"ok","data":[]}"#).unwrap(), serde_json::json!([]));
    }

    #[test]
    fn source_view_uses_best_existing_file_without_changing_inference_path() {
        let c=rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&c).unwrap();
        let dir=tempfile::tempdir().unwrap();
        let original=dir.path().join("original.png");let display=dir.path().join("display.jpg");let thumb=dir.path().join("thumb.jpg");
        for p in [&original,&display,&thumb]{std::fs::write(p,b"synthetic").unwrap();}
        c.execute_batch("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES('fixture','string','fixture','1','[]');
          INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release) VALUES('entry','fixture',0,'fixture','[]','','safe','1','1');
          INSERT INTO reverse_batch(batch_id,created_at,template_id,ruleset_id,provider_ids_json,question_count,status) VALUES('batch','now',1,1,'[]',1,'ok');
          INSERT INTO reverse_run(run_id,batch_id,created_at,source_kind,source_ref,image_ordinal,image_sha256,template_id,ruleset_id,provider_id,request_json,status) VALUES(1,'batch','now','artist_entry','entry',0,'fixture',1,1,1,'{}','ok');").unwrap();
        c.execute("INSERT INTO entry_image(entry_id,ordinal,image_file,original_local_path,local_path,thumb_path) VALUES('entry',0,'fixture',?1,?2,?3)",rusqlite::params![original.to_str(),display.to_str(),thumb.to_str()]).unwrap();
        assert_eq!(source_image(&c,1).unwrap().kind,"original");
        std::fs::remove_file(&original).unwrap();assert_eq!(source_image(&c,1).unwrap().kind,"display");
        std::fs::remove_file(&display).unwrap();assert_eq!(source_image(&c,1).unwrap().kind,"thumbnail");
        std::fs::remove_file(&thumb).unwrap();assert_eq!(source_image(&c,1).unwrap().kind,"missing");
        // The view never rewrites the path used by the inference pipeline.
        assert_eq!(c.query_row("SELECT local_path FROM entry_image",[],|r|r.get::<_,String>(0)).unwrap(),display.to_str().unwrap());
        c.execute("UPDATE reverse_run SET source_kind='local_file',source_ref=?1",[dir.path().join("local.png").to_str()]).unwrap();
        assert_eq!(source_image(&c,1).unwrap().kind,"missing");
        std::fs::write(dir.path().join("local.png"),b"fixture").unwrap();assert_eq!(source_image(&c,1).unwrap().kind,"original");
        assert!(source_image(&c,999).is_err());
    }
}
