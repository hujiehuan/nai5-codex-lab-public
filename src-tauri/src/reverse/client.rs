//! 视觉模型调用（CONTRACT §4「送模型前预处理」）：文件 > 9 MB 才缩到最长边 2048 并按 JPEG 88→72→55 压到 ≤ 9 MB；
//! 请求体为 OpenAI 兼容 `chat/completions`（system=模板正文，user=图片 data URL + 固定文本），
//! `temperature=0`，`max_tokens=provider.max_tokens`，超时 180 s；响应取第一个 `{…}` JSON。
//! `request_json` 落库时图片用 sha256 与尺寸/字节数代替，不存 base64；v0.2.3 起多记 `base_url`
//! （插件端点填插件地址、`model` 填 `plugin:<provider_id>`，CONTRACT §10.1）。
//! v0.3.0（§12.6）：`PreparedImage` 另带**缩放前**的原图宽高（`original_width` / `original_height`），重画路由按它选尺寸
//! （与 插件 `/反推` 的 `_size_for_image` 同一函数）；解码失败时为 0，调用方当 null 送。

use std::path::Path;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

pub const MAX_IMAGE_BYTES: u64 = 9_000_000;
pub const MAX_SIDE: u32 = 2048;
pub const USER_TEXT: &str = "Generate the prompt for this image.";
pub const TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Clone)]
pub struct PreparedImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    /// 送模型那份字节的宽高（> 9 MB 时是缩过的）。
    pub width: u32,
    pub height: u32,
    pub sha256: String,
    pub original_bytes: u64,
    pub shrunk: bool,
    /// §12.6：缩放前的原图宽高；解码失败为 0（不缩时与 `width` / `height` 相同）。
    pub original_width: u32,
    pub original_height: u32,
}

fn mime_of(bytes: &[u8], path: &Path) -> String {
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => "image/png".into(),
        Ok(image::ImageFormat::Jpeg) => "image/jpeg".into(),
        Ok(image::ImageFormat::WebP) => "image/webp".into(),
        Ok(image::ImageFormat::Gif) => "image/gif".into(),
        _ => match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("png") => "image/png".into(),
            Some("webp") => "image/webp".into(),
            _ => "image/jpeg".into(),
        },
    }
}

/// 读取并按需压缩图片。
pub fn prepare_image(path: &Path) -> AppResult<PreparedImage> {
    let raw = std::fs::read(path).map_err(|e| AppError::io(format!("读图 {}: {e}", path.display())))?;
    let original_bytes = raw.len() as u64;
    let (bytes, mime, width, height, shrunk, original_width, original_height) = if original_bytes <= MAX_IMAGE_BYTES {
        let (w, h) = image::load_from_memory(&raw).map(|i| (i.width(), i.height())).unwrap_or((0, 0));
        (raw.clone(), mime_of(&raw, path), w, h, false, w, h)
    } else {
        let img = image::load_from_memory(&raw)?;
        let (w, h) = (img.width(), img.height());
        let scale = (MAX_SIDE as f64 / w.max(h) as f64).min(1.0);
        let img = if scale < 1.0 {
            img.resize((w as f64 * scale).max(1.0) as u32, (h as f64 * scale).max(1.0) as u32, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let rgb = img.to_rgb8();
        let mut out = Vec::new();
        for q in [88u8, 72, 55] {
            out.clear();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q);
            enc.encode_image(&rgb).map_err(|e| AppError::io(format!("jpeg encode: {e}")))?;
            if out.len() as u64 <= MAX_IMAGE_BYTES {
                break;
            }
        }
        (out, "image/jpeg".to_string(), rgb.width(), rgb.height(), true, w, h)
    };
    let sha256 = hex::encode(Sha256::digest(&bytes));
    Ok(PreparedImage { bytes, mime, width, height, sha256, original_bytes, shrunk, original_width, original_height })
}

/// 图片正文的 base64：data URL 与插件 `/lab/reverse` 的 `image_base64`（§10.1）共用同一份压缩后的字节。
pub fn image_base64(img: &PreparedImage) -> String {
    base64::engine::general_purpose::STANDARD.encode(&img.bytes)
}

pub fn data_url(img: &PreparedImage) -> String {
    format!("data:{};base64,{}", img.mime, image_base64(img))
}

/// 模板 `{extra}` 占位替换为空串。
pub fn render_template(body: &str) -> String {
    body.replace("{extra}", "")
}

pub fn build_messages(template_body: &str, img: &PreparedImage) -> Value {
    json!([
        { "role": "system", "content": render_template(template_body) },
        { "role": "user", "content": [
            { "type": "image_url", "image_url": { "url": data_url(img), "detail": "high" } },
            { "type": "text", "text": USER_TEXT }
        ] }
    ])
}

/// 落库用：图片以 sha256 代替；`base_url` / `model` 由调用方按端点种类给（插件端点：插件地址 + `plugin:<provider_id>`）。
pub fn build_request_log(template_body: &str, img: &PreparedImage, base_url: &str, model: &str, max_tokens: i64) -> Value {
    json!({
        "base_url": base_url,
        "model": model,
        "temperature": 0,
        "max_tokens": max_tokens,
        "messages": [
            { "role": "system", "content": render_template(template_body) },
            { "role": "user", "content": [
                { "type": "image_url", "image_url": { "url": format!("sha256:{}", img.sha256), "detail": "high" } },
                { "type": "text", "text": USER_TEXT }
            ] }
        ],
        "image": {
            "sha256": img.sha256, "mime": img.mime, "width": img.width, "height": img.height, "bytes": img.bytes.len(), "original_bytes": img.original_bytes, "shrunk": img.shrunk,
            "original_width": img.original_width, "original_height": img.original_height,
        },
    })
}

#[derive(Debug, Clone)]
pub struct ChatOutcome {
    pub text: String,
    /// `content` 缺失、兜底取了 `reasoning_content`（推理模型只回思考没回答案），调用方要标 warn（v0.1.1 #13）。
    pub from_reasoning: bool,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub latency_ms: i64,
    pub raw: Value,
}

pub fn chat_url(base_url: &str) -> String {
    let b = base_url.trim().trim_end_matches('/');
    if b.ends_with("/chat/completions") {
        b.to_string()
    } else {
        format!("{b}/chat/completions")
    }
}

/// 从 OpenAI 兼容响应里取文本（content 可能是字符串或分段数组）。
/// 返回 `(文本, 是否兜底取自 reasoning_content)`：`content` 缺失/为 null 时才读 `reasoning_content`。
pub fn extract_text(resp: &Value) -> Option<(String, bool)> {
    let msg = resp.pointer("/choices/0/message")?;
    match msg.get("content") {
        Some(Value::String(s)) => Some((s.clone(), false)),
        Some(Value::Array(parts)) => Some((
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>()
                .join(""),
            false,
        )),
        _ => msg.get("reasoning_content").and_then(|v| v.as_str()).map(|s| (s.to_string(), true)),
    }
}

pub async fn chat(http: &reqwest::Client, base_url: &str, api_key: &str, model: &str, max_tokens: i64, messages: Value) -> AppResult<ChatOutcome> {
    let url = chat_url(base_url);
    let body = json!({ "model": model, "messages": messages, "temperature": 0, "max_tokens": max_tokens });
    let started = Instant::now();
    let mut req = http.post(&url).timeout(Duration::from_secs(TIMEOUT_SECS)).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await.map_err(|e| AppError::network(format!("{url}: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| AppError::network(format!("{url}: {e}")))?;
    let latency_ms = started.elapsed().as_millis() as i64;
    if !status.is_success() {
        return Err(AppError::upstream(format!("HTTP {status}: {}", text.chars().take(500).collect::<String>())));
    }
    let raw: Value = serde_json::from_str(&text).map_err(|e| AppError::upstream(format!("响应非 JSON: {e}")))?;
    let (content, from_reasoning) = extract_text(&raw).ok_or_else(|| AppError::upstream("响应里没有 choices[0].message.content"))?;
    Ok(ChatOutcome {
        text: content,
        from_reasoning,
        prompt_tokens: raw.pointer("/usage/prompt_tokens").and_then(|v| v.as_i64()),
        completion_tokens: raw.pointer("/usage/completion_tokens").and_then(|v| v.as_i64()),
        latency_ms,
        raw,
    })
}

/// 取响应文本里第一个 `{…}`（贪婪到最后一个 `}`，与插件 `_parse_json` 一致），解析失败返回 None。
pub fn extract_json(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    let slice = &text[start..=end];
    if let Ok(v) = serde_json::from_str::<Value>(slice) {
        return Some(v);
    }
    // 退化：模型多输出了一段，尝试逐个 `}` 截断
    let mut cut = end;
    while let Some(pos) = text[start..cut].rfind('}') {
        let candidate = &text[start..start + pos + 1];
        if let Ok(v) = serde_json::from_str::<Value>(candidate) {
            return Some(v);
        }
        cut = start + pos;
        if cut <= start {
            break;
        }
    }
    None
}

/// `tags` 字段可能是字符串或数组。
pub fn tags_field(parsed: &Value) -> String {
    match parsed.get("tags") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_extraction_and_text_parts() {
        let v = extract_json("Sure! ```json\n{\"suitable\": true, \"tags\": \"a, b\"}\n``` done").unwrap();
        assert_eq!(v["suitable"], true);
        assert_eq!(tags_field(&v), "a, b");
        assert!(extract_json("no json here").is_none());
        let arr = serde_json::json!({"tags": ["x", "y"]});
        assert_eq!(tags_field(&arr), "x, y");
        let resp = serde_json::json!({"choices": [{"message": {"content": [{"type": "text", "text": "he"}, {"type": "text", "text": "llo"}]}}]});
        assert_eq!(extract_text(&resp), Some(("hello".to_string(), false)));
        // #13：content 为 null 才兜底 reasoning_content，并标出来
        let reasoning = serde_json::json!({"choices": [{"message": {"content": null, "reasoning_content": "thinking {\"tags\": \"a\"}"}}]});
        assert_eq!(extract_text(&reasoning), Some(("thinking {\"tags\": \"a\"}".to_string(), true)));
        let empty = serde_json::json!({"choices": [{"message": {"content": "", "reasoning_content": "x"}}]});
        assert_eq!(extract_text(&empty), Some((String::new(), false)), "content 是空串不算缺失");
        assert_eq!(extract_text(&serde_json::json!({"choices": []})), None);
        assert_eq!(chat_url("https://api.example.com/v1/"), "https://api.example.com/v1/chat/completions");
        assert_eq!(chat_url("https://api.example.com/v1/chat/completions"), "https://api.example.com/v1/chat/completions");
        assert_eq!(render_template("abc{extra}"), "abc");
    }

    #[test]
    fn small_image_is_sent_as_is_and_logged_by_sha() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.png");
        image::RgbImage::from_pixel(20, 10, image::Rgb([9, 9, 9])).save(&p).unwrap();
        let img = prepare_image(&p).unwrap();
        assert_eq!(img.mime, "image/png");
        assert_eq!((img.width, img.height), (20, 10));
        assert_eq!((img.original_width, img.original_height), (20, 10), "§12.6：不缩时原图宽高就是解码宽高");
        assert!(!img.shrunk);
        assert_eq!(img.bytes, std::fs::read(&p).unwrap());
        let msgs = build_messages("sys {extra}", &img);
        assert_eq!(msgs[0]["content"], "sys ");
        assert!(msgs[1]["content"][0]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        let log = build_request_log("sys", &img, "https://x/v1", "m", 1100);
        assert_eq!(log["image"]["sha256"], img.sha256);
        assert_eq!((log["image"]["original_width"].as_u64(), log["image"]["original_height"].as_u64()), (Some(20), Some(10)));
        assert_eq!((log["base_url"].as_str(), log["model"].as_str()), (Some("https://x/v1"), Some("m")));
        assert!(log.to_string().len() < 2000, "request_json 不应包含 base64");
        // 不是图片的文件：解码失败 → 宽高与原图宽高都是 0，调用方送 null
        let bad = dir.path().join("bad.png");
        std::fs::write(&bad, b"not an image").unwrap();
        let img = prepare_image(&bad).unwrap();
        assert_eq!((img.width, img.height, img.original_width, img.original_height), (0, 0, 0, 0));
    }

    /// §12.6：> 9 MB 的图会被缩到最长边 2048，`width` / `height` 是缩后的，`original_width` / `original_height` 仍是缩前的。
    #[test]
    fn oversized_image_keeps_original_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.png");
        // 噪声图压不动：2200×1500 RGB 随机像素的 PNG 约 9.9 MB，超过 9 MB
        let mut rng: u32 = 0x1234_5678;
        let mut buf = vec![0u8; 2200 * 1500 * 3];
        for b in buf.iter_mut() {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            *b = (rng & 0xff) as u8;
        }
        image::RgbImage::from_raw(2200, 1500, buf).unwrap().save(&p).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > MAX_IMAGE_BYTES, "测试图应大于 9 MB");
        let img = prepare_image(&p).unwrap();
        assert!(img.shrunk);
        assert_eq!(img.mime, "image/jpeg");
        assert!(img.width.max(img.height) <= MAX_SIDE && img.width >= 2040, "缩到最长边 2048（image 的 resize 会四舍五入一像素）: {}x{}", img.width, img.height);
        assert_eq!((img.original_width, img.original_height), (2200, 1500), "原图宽高不随缩放变");
        assert!(img.bytes.len() as u64 <= MAX_IMAGE_BYTES);
    }
}
