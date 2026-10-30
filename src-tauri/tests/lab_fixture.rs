//! 插件侧**真实输出**的形状回归（CONTRACT §9.1 / §9.3）。
//!
//! `fixtures/lab_run.sample.json`（一次画图运行的全量 `LabRun`）与 `fixtures/lab_run.extras.json`
//! （`preview` 的 `{ plans }`、`start` 的 `{ run_id, plans }`、一次带单张失败的 `LabRun`、`run/list` 的 `LabRunSummary[]`）
//! 的形状取自插件假夹具跑 `compose` / `start_run`（假 `wrapped_generate`，不碰网络 / NovelAI / LLM）的落盘输出；
//! 公开版把其中的提示词、图包路径等内容换成了合成数据，键与结构保持原样。插件端改了 DTO 就按新输出更新夹具。
//! `lab.rs` 里的单测用的是按契约手写的 fixture，这里补的是"插件真的这么吐"的那一份。

use nai5_codex_lab_lib::models::*;
use serde_json::Value;

const RUN: &str = include_str!("fixtures/lab_run.sample.json");
const EXTRAS: &str = include_str!("fixtures/lab_run.extras.json");

/// 数值按 f64 比（插件写 `64` / Rust 回写 `64.0`），其余键 / 结构逐一相同。
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w))),
        _ => a == b,
    }
}

#[test]
fn plugin_lab_run_sample_deserializes_and_round_trips() {
    let run: LabRun = serde_json::from_str(RUN).expect("插件真实 LabRun 应按 §9.1 反序列化");
    assert_eq!(run.mode, LabMode::Draw);
    assert_eq!(run.status, LabRunStatus::Done);
    assert_eq!(run.request.count, 2);
    assert_eq!(run.request.seed, Some(123456));
    assert_eq!(run.request.scope.len(), 2);
    assert_eq!(run.stamina_before, Some(144.0));
    assert!(run.finished_at.is_some() && run.error.is_none());
    assert_eq!(run.plugin_version, "v1.1.0", "插件 metadata.yaml 的 version 原样带回");
    assert_eq!(run.images.len(), 2);
    let first = &run.images[0];
    assert_eq!(first.status, LabImageStatus::Done);
    assert_eq!(first.plan.artist_source, LabArtistSource::Style);
    assert_eq!(first.plan.seed, Some(123456));
    assert_eq!(run.images[1].plan.seed, Some(123457), "固定种子每张 +i");
    assert_eq!(first.rating, Some(LabRating::Fav));
    assert_eq!(run.images[1].rating, Some(LabRating::Dis));
    assert_eq!(first.score, Some(7.5));
    assert_eq!(first.notes.len(), 2);
    assert!(!first.plan.dropped.is_empty() && first.plan.tags_raw != first.plan.tags_used, "筛选后 dropped 与 tags diff 可见");
    assert!(first.file.as_deref().is_some_and(|f| f.starts_with("lab_draw_") && f.ends_with("_1.png")));

    // 序列化回去键集合不多不少，数值按 f64 相等
    let back = serde_json::to_value(&run).unwrap();
    let expected: Value = serde_json::from_str(RUN).unwrap();
    assert!(same_json(&back, &expected), "round-trip 不一致:\n{back:#}");
    assert_eq!(back.as_object().unwrap().len(), 11);
    assert_eq!(back["images"][0].as_object().unwrap().len(), 23, "LabPlan 17 字段（含 §11.3 prose）+ LabImage 6 字段");
    assert!(back["images"][0].get("plan").is_none(), "LabImage 是扁平的");
}

#[test]
fn plugin_lab_extras_deserialize() {
    let ex: Value = serde_json::from_str(EXTRAS).unwrap();

    // POST run/preview → { plans }
    let plans: Vec<LabPlan> = serde_json::from_value(ex["preview"]["plans"].clone()).expect("preview.plans 应是 LabPlan[]");
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].idx, 0);
    assert_eq!(ex["preview"]["plans"][0].as_object().unwrap().len(), 17, "LabPlan 17 字段（含 §11.3 prose）");

    // POST run/start → { run_id, plans }
    let start: LabRunStart = serde_json::from_value(ex["start"].clone()).expect("start 应是 LabRunStart");
    assert!(!start.run_id.is_empty());
    assert_eq!(start.plans.len(), 2);

    // 单张失败的运行：整体 done，失败那张 status=error / file=null / error 有话
    let partial: LabRun = serde_json::from_value(ex["run_partial_error"].clone()).expect("run_partial_error 应是 LabRun");
    assert_eq!(partial.mode, LabMode::Draw);
    assert_eq!(partial.status, LabRunStatus::Done);
    let bad = partial.images.iter().find(|i| i.status == LabImageStatus::Error).expect("应有一张失败");
    assert!(bad.file.is_none() && bad.error.as_deref().is_some_and(|e| e.contains("GenerateError")));
    assert!(bad.score.is_none() && bad.notes.is_empty() && bad.rating.is_none());
    assert!(partial.images.iter().all(|i| i.plan.seed.is_none()), "随机 seed 时 LabImage.seed 保持 null");
    assert_eq!(partial.images[0].plan.artist_source, LabArtistSource::Pool);

    // GET run/list → LabRunSummary[]（新的在前）
    let list: Vec<LabRunSummary> = serde_json::from_value(ex["list"].clone()).expect("list 应是 LabRunSummary[]");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].run_id, partial.run_id, "新的在前");
    assert_eq!(list[0].done, 1);
    assert_eq!(list[0].best_score, None);
    assert_eq!(list[1].best_score, Some(7.5));
    assert_eq!(ex["list"][0].as_object().unwrap().len(), 9, "LabRunSummary 9 字段");
}
