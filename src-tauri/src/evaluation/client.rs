//! One bounded Responses request per case. No redirects, tool execution or retries.
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use zeroize::Zeroizing;

pub(super) const MAX_OUTPUT_BYTES: usize = 24 * 1024;
const MAX_RESPONSE_BYTES: usize = 768 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct PreparedTarget {
    pub target: super::types::EvaluationTarget,
    pub channel_name: String,
    pub model_alias: String,
    pub endpoint: String,
    pub key: Zeroizing<String>,
}
pub(super) struct Answer {
    pub text: String,
    pub elapsed_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
pub(super) struct RequestError {
    pub message: String,
    pub cancelled: bool,
}
impl RequestError {
    fn new(message: &str) -> Self {
        Self {
            message: message.into(),
            cancelled: false,
        }
    }
    fn cancelled() -> Self {
        Self {
            message: "评测已取消，后续请求未发出。".into(),
            cancelled: true,
        }
    }
}
pub(super) fn redact(value: &str, key: &str, limit: usize) -> String {
    let redacted = if key.is_empty() {
        value.into()
    } else {
        value.replace(key, "[REDACTED]")
    };
    if redacted.len() <= limit {
        return redacted;
    }
    let mut end = limit;
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[输出已截断]", &redacted[..end])
}
pub(super) fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| "无法初始化评测网络请求。".into())
}
pub(super) async fn request(
    client: &reqwest::Client,
    target: &PreparedTarget,
    prompt: &str,
    mut cancel: watch::Receiver<bool>,
) -> Result<Answer, RequestError> {
    if *cancel.borrow() {
        return Err(RequestError::cancelled());
    }
    let endpoint = crate::diagnostics::normalize_endpoint(&target.endpoint)
        .map_err(|_| RequestError::new("评测渠道地址无效。"))?;
    let mut body = json!({"model":target.target.model_id,"input":prompt,"stream":false,"store":false,"max_output_tokens":8192});
    if let Some(effort) = &target.target.reasoning_effort {
        body["reasoning"] = json!({"effort":effort});
    }
    let started = Instant::now();
    let response = tokio::select! {
        biased;
        _ = cancel.changed() => return Err(RequestError::cancelled()),
        result = client.post(format!("{endpoint}/responses")).bearer_auth(target.key.as_str()).json(&body).send() =>
            result.map_err(|_| RequestError::new("请求未完成或已超时，未自动重试。"))?
    };
    let status = response.status();
    if !status.is_success() {
        return Err(RequestError::new(match status.as_u16() {
            401 | 403 => "渠道鉴权失败，请检查密钥和模型权限。",
            429 => "渠道限流或余额不足，未自动重试。",
            300..=399 => "渠道返回重定向，已停止发送凭据。",
            _ => "渠道返回错误，未保存服务商原始错误内容。",
        }));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(RequestError::new("评测响应超过大小限制。"));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Zeroizing::new(Vec::new());
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel.changed() => return Err(RequestError::cancelled()),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|_| RequestError::new("评测响应接收中断，未自动重试。"))?;
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(RequestError::new("评测响应超过大小限制。"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|_| RequestError::new("渠道未返回有效的 Responses JSON。"))?;
    if payload.get("error").is_some_and(|value| !value.is_null())
        || payload
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "completed")
    {
        return Err(RequestError::new("模型没有完整完成回答，本题未评分。"));
    }
    let mut output = String::new();
    for item in payload
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        for content in item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if content.get("type").and_then(Value::as_str) == Some("output_text") {
                if let Some(text) = content.get("text").and_then(Value::as_str) {
                    output.push_str(text);
                    output.push('\n');
                }
            }
        }
    }
    if output.trim().is_empty() {
        return Err(RequestError::new(
            "模型未返回可评分的文本；本模块不会执行模型生成的工具调用。",
        ));
    }
    if output.len() > MAX_OUTPUT_BYTES {
        return Err(RequestError::new("回答超过评测输出上限，本题未评分。"));
    }
    Ok(Answer {
        text: redact(output.trim(), &target.key, MAX_OUTPUT_BYTES),
        elapsed_ms: started.elapsed().as_millis() as u64,
        input_tokens: payload
            .pointer("/usage/input_tokens")
            .and_then(Value::as_u64),
        output_tokens: payload
            .pointer("/usage/output_tokens")
            .and_then(Value::as_u64),
    })
}
