//! 反推实验室（设计 §5、CONTRACT §4）：
//! - `builtin`  插件 `plugin v1` 模板与规则原文、角色名词表
//! - `client`   OpenAI 兼容 chat/completions 调用与图片预处理
//! - `clean`    规则集清洗（对应插件 `_clean_tags`）
//! - `metrics`  评测口径（truth / expected_miss / 各指标）
//! - `store`    数据层（模板、规则集、端点、题库、批次、比较、评分）
//! - `batch`    后台批次任务 + RunEvent
//! - `render`   反推重画对照（CONTRACT §11.3 / §11.4）：插件重画 + 评分 + 取图落盘，`reverse_render_run` 单条重画

pub mod batch;
pub mod builtin;
pub mod clean;
pub mod client;
pub mod control;
pub mod metrics;
pub mod render;
pub mod store;
