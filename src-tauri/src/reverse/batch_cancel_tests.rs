//! 真正经过 reqwest、插件登录/反推/重画传输与 SQLite 的停止回归；全部 HTTP 仅绑定本机随机端口。

use super::*;
use crate::plugin::PluginConn;
use crate::reverse::control::BatchController;
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

type RequestLog = Arc<Mutex<Vec<(String, Value)>>>;
type Reply = Arc<dyn Fn(&str, &Value) -> Option<Value> + Send + Sync>;

struct FakePlugin {
    base: String,
    requests: RequestLog,
    received: mpsc::UnboundedReceiver<String>,
    task: tokio::task::JoinHandle<()>,
}

impl FakePlugin {
    async fn start(reply: impl Fn(&str, &Value) -> Option<Value> + Send + Sync + 'static) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let reply: Reply = Arc::new(reply);
        let (sent, received) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    incoming = listener.accept() => {
                        let (mut socket, _) = incoming.unwrap();
                        let log = log.clone();
                        let reply = reply.clone();
                        let sent = sent.clone();
                        connections.spawn(async move {
                            let mut bytes = Vec::new();
                            let header_end = loop {
                                let mut buf = [0u8; 4096];
                                let n = socket.read(&mut buf).await.unwrap_or(0);
                                if n == 0 { return; }
                                bytes.extend_from_slice(&buf[..n]);
                                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") { break end + 4; }
                                assert!(bytes.len() < 1024 * 1024);
                            };
                            let headers = String::from_utf8_lossy(&bytes[..header_end]);
                            let path = headers.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_string();
                            let length = headers.lines().filter_map(|line| line.split_once(':'))
                                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                .map(|(_, length)| length.trim().parse::<usize>().unwrap()).unwrap_or(0);
                            while bytes.len() < header_end + length {
                                let mut buf = [0u8; 4096];
                                let n = socket.read(&mut buf).await.unwrap_or(0);
                                if n == 0 { return; }
                                bytes.extend_from_slice(&buf[..n]);
                            }
                            let body = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap_or(Value::Null);
                            log.lock().push((path.clone(), body.clone()));
                            let _ = sent.send(path.clone());
                            let Some(body) = reply(&path, &body) else {
                                std::future::pending::<()>().await;
                                return;
                            };
                            let body = body.to_string();
                            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                            let _ = socket.write_all(response.as_bytes()).await;
                        });
                    },
                    _ = connections.join_next(), if !connections.is_empty() => {},
                }
            }
        });
        Self { base, requests, received, task }
    }

    async fn wait_for(&mut self, suffix: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(path) = self.received.recv().await {
                if path.split('?').next().is_some_and(|path| path.ends_with(suffix)) { return; }
            }
            panic!("fake HTTP server exited");
        }).await.expect("expected fake HTTP request");
    }
}

impl Drop for FakePlugin {
    fn drop(&mut self) { self.task.abort(); }
}

fn success(path: &str, _: &Value) -> Option<Value> {
    if path.ends_with("/login") {
        Some(json!({ "data": { "token": "local-test-token" } }))
    } else if path.ends_with("/reverse") {
        Some(json!({ "data": { "text": "{\"tags\":\"smile, standing, ribbon, gloves, microphone\",\"framing\":\"upper body\"}", "latency_ms": 1 } }))
    } else { None }
}

struct Fixture {
    _dir: tempfile::TempDir,
    ctx: Arc<BatchCtx>,
    prepared: Prepared,
    lease: BatchLease,
    controller: Arc<BatchController>,
    events: Arc<Mutex<Vec<RunEvent>>>,
    received: mpsc::UnboundedReceiver<RunEvent>,
}

fn fixture(server: &FakePlugin, providers: &[&str], question_count: usize, render: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("q.png");
    image::RgbImage::from_pixel(8, 8, image::Rgb([100, 150, 200])).save(&image_path).unwrap();
    let db = Arc::new(Db::open_memory().unwrap());
    db.with(|c| super::super::builtin::ensure_builtin(c)).unwrap();
    let ids = providers.iter().map(|name| db.with(|c| store::provider_save(c, &ReverseProviderInput {
        provider_id: None, name: name.to_string(), base_url: String::new(), model: String::new(), max_tokens: 100,
        enabled: true, kind: PROVIDER_KIND_PLUGIN.into(), plugin_provider_id: Some(name.to_string()),
    })).unwrap().provider_id).collect::<Vec<_>>();
    let req: RunBatchRequest = serde_json::from_value(json!({
        "question_ids": vec![format!("local:{}", image_path.display()); question_count],
        "template_id": 1, "ruleset_id": 1, "provider_ids": ids, "render": render,
    })).unwrap();
    let controller = Arc::new(BatchController::default());
    let lease = controller.reserve().unwrap();
    let prepared = prepare(&db, dir.path(), &req).unwrap();
    lease.bind(&prepared.batch_id);
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let (sent, received) = mpsc::unbounded_channel();
    let ctx = Arc::new(BatchCtx {
        db, http: reqwest::Client::new(), data_dir: dir.path().to_path_buf(), cancel: lease.cancel.clone(),
        emit: Arc::new(move |event| { log.lock().push(event.clone()); let _ = sent.send(event); }),
        plugin: Some(Arc::new(PluginSession::new(PluginConn {
            base: server.base.clone(), username: "local-test-user".into(), password: "local-test-password".into(), library_token: None,
        }))),
        style: Arc::new(StyleState::new()),
    });
    Fixture { _dir: dir, ctx, prepared, lease, controller, events, received }
}

async fn wait_event(received: &mut mpsc::UnboundedReceiver<RunEvent>, status: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = received.recv().await {
            if event.status == status { return; }
        }
        panic!("batch events ended before {status}");
    }).await.expect("expected batch event");
}

fn assert_cancelled(ctx: &BatchCtx, events: &Mutex<Vec<RunEvent>>, batch_id: &str, expected_rows: usize) {
    let result = ctx.db.with(|c| store::batch_results(c, batch_id)).unwrap();
    assert_eq!(result.batch.status, "cancelled");
    assert!(result.batch.finished_at.is_some());
    assert_eq!(result.runs.len(), expected_rows);
    assert!(result.runs.iter().all(|row| row.status == "ok"), "停止不得伪装成模型失败");
    let events = events.lock();
    assert_eq!(events.last().unwrap().status, "batch_cancelled");
    assert_eq!(events.last().unwrap().done, expected_rows as i64);
    assert!(!events.iter().any(|event| matches!(event.status.as_str(), "batch_done" | "batch_failed" | "run_failed")));
}

#[tokio::test]
async fn stop_before_dispatch_makes_no_http_requests_and_releases_slot() {
    let server = FakePlugin::start(success).await;
    let f = fixture(&server, &["first"], 1, false);
    let batch_id = f.prepared.batch_id.clone();
    f.controller.cancel(&batch_id);
    tokio::time::timeout(Duration::from_secs(2), run(f.ctx.clone(), f.prepared, f.lease)).await.unwrap();
    assert_cancelled(&f.ctx, &f.events, &batch_id, 0);
    assert!(server.requests.lock().is_empty());
    assert!(f.controller.reserve().is_ok());
}

#[tokio::test]
async fn stop_while_logging_in_never_dispatches_queued_providers() {
    let mut server = FakePlugin::start(|_, _| None).await;
    let f = fixture(&server, &["first", "second"], 2, true);
    let batch_id = f.prepared.batch_id.clone();
    let task = tokio::spawn(run(f.ctx.clone(), f.prepared, f.lease));
    server.wait_for("/login").await;
    f.controller.cancel(&batch_id);
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert_cancelled(&f.ctx, &f.events, &batch_id, 0);
    assert_eq!(server.requests.lock().len(), 1);
    assert!(f.controller.reserve().is_ok());
}

#[tokio::test]
async fn stop_while_one_provider_waits_preserves_fast_result_and_skips_next_question() {
    let mut server = FakePlugin::start(|path, body| {
        if body.get("provider_id").and_then(Value::as_str) == Some("slow") { None } else { success(path, body) }
    }).await;
    let mut f = fixture(&server, &["fast", "slow"], 2, false);
    let batch_id = f.prepared.batch_id.clone();
    let task = tokio::spawn(run(f.ctx.clone(), f.prepared, f.lease));
    server.wait_for("/reverse").await;
    server.wait_for("/reverse").await;
    wait_event(&mut f.received, "run_done").await;
    f.controller.cancel(&batch_id);
    f.controller.cancel(&batch_id);
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert_cancelled(&f.ctx, &f.events, &batch_id, 1);
    assert_eq!(server.requests.lock().iter().filter(|(path, _)| path.ends_with("/reverse")).count(), 2);
    assert!(f.controller.reserve().is_ok());
}

#[tokio::test]
async fn stop_on_last_result_still_finishes_cancelled() {
    let server = FakePlugin::start(success).await;
    let mut f = fixture(&server, &["last"], 1, false);
    let batch_id = f.prepared.batch_id.clone();
    let controller = f.controller.clone();
    let normal_emit = f.ctx.emit.clone();
    Arc::get_mut(&mut f.ctx).unwrap().emit = Arc::new(move |event| {
        if event.status == "run_done" { controller.cancel(&event.batch_id); }
        normal_emit(event);
    });
    tokio::time::timeout(Duration::from_secs(10), run(f.ctx.clone(), f.prepared, f.lease)).await.unwrap();
    assert_cancelled(&f.ctx, &f.events, &batch_id, 1);
}

#[tokio::test]
async fn stop_during_render_preserves_all_model_outputs_and_prevents_queued_render() {
    let mut server = FakePlugin::start(success).await; // /lab/render 接受后一直等待。
    let f = fixture(&server, &["first", "second"], 2, true);
    let batch_id = f.prepared.batch_id.clone();
    let task = tokio::spawn(run(f.ctx.clone(), f.prepared, f.lease));
    server.wait_for("/render").await;
    f.controller.cancel(&batch_id);
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert_cancelled(&f.ctx, &f.events, &batch_id, 2);
    let requests = server.requests.lock();
    assert_eq!(requests.iter().filter(|(path, _)| path.ends_with("/reverse")).count(), 2);
    assert_eq!(requests.iter().filter(|(path, _)| path.ends_with("/render")).count(), 1);
    assert!(!requests.iter().any(|(path, _)| path.contains("/thumb")));
    drop(requests);
    let result = f.ctx.db.with(|c| store::batch_results(c, &batch_id)).unwrap();
    assert_eq!(result.runs.iter().filter(|row| row.render_error.as_deref() == Some("已停止（已保存的结果保留）")).count(), 1);
    assert!(f.controller.reserve().is_ok());
}

#[tokio::test]
async fn completed_batch_is_done_and_repeated_stop_is_harmless() {
    let server = FakePlugin::start(success).await;
    let f = fixture(&server, &["first", "second"], 2, false);
    let batch_id = f.prepared.batch_id.clone();
    tokio::time::timeout(Duration::from_secs(10), run(f.ctx.clone(), f.prepared, f.lease)).await.unwrap();
    f.controller.cancel(&batch_id);
    let result = f.ctx.db.with(|c| store::batch_results(c, &batch_id)).unwrap();
    assert_eq!(result.batch.status, "done");
    assert_eq!(result.runs.len(), 4);
    assert_eq!(f.events.lock().last().unwrap().status, "batch_done");
    assert!(f.controller.reserve().is_ok());
}

#[tokio::test]
async fn stop_during_image_fetch_preserves_already_saved_render_metadata() {
    let mut server = FakePlugin::start(|path, body| {
        if path.ends_with("/render") {
            Some(json!({ "data": { "file": "render.png", "seed": 42, "prompt": "saved prompt", "quality_score": 8 } }))
        } else { success(path, body) }
    }).await;
    let f = fixture(&server, &["first"], 1, true);
    let batch_id = f.prepared.batch_id.clone();
    let task = tokio::spawn(run(f.ctx.clone(), f.prepared, f.lease));
    server.wait_for("/thumb").await;
    f.controller.cancel(&batch_id);
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert_cancelled(&f.ctx, &f.events, &batch_id, 1);
    let result = f.ctx.db.with(|c| store::batch_results(c, &batch_id)).unwrap();
    assert_eq!(result.runs[0].render_seed, Some(42));
    assert_eq!(result.runs[0].render_prompt.as_deref(), Some("saved prompt"));
    assert_eq!(result.runs[0].quality_score, Some(8.0));
    assert!(result.runs[0].render_error.as_deref().unwrap().contains("已停止"));
}

#[tokio::test]
async fn failed_batch_releases_slot_and_does_not_call_http() {
    let server = FakePlugin::start(success).await;
    let mut f = fixture(&server, &["first"], 1, false);
    let batch_id = f.prepared.batch_id.clone();
    f.prepared.ruleset.appearance_regex = "[".into();
    run(f.ctx.clone(), f.prepared, f.lease).await;
    let result = f.ctx.db.with(|c| store::batch_results(c, &batch_id)).unwrap();
    assert_eq!(result.batch.status, "failed");
    assert!(result.batch.finished_at.is_some());
    assert_eq!(f.events.lock().last().unwrap().status, "batch_failed");
    assert!(server.requests.lock().is_empty());
    assert!(f.controller.reserve().is_ok());
}
