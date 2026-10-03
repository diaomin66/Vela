//! Bounded, read-only discovery of a channel's model catalog and account allowance.
//!
//! A returned model is advertised by the service; it is not a compatibility claim.
//! Credentials and upstream error bodies never enter public results. Automatic
//! billing adapters deliberately retain quota units instead of assuming dollars.

use chrono::Utc;
use futures_util::StreamExt;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, time::Duration};
use tokio::sync::watch;
use url::{Host, Url};
use zeroize::Zeroize;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_MODELS: usize = 4096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceConfig {
    #[serde(default = "auto_mode")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<f64>,
}

fn auto_mode() -> String {
    "auto".into()
}

impl Default for BalanceConfig {
    fn default() -> Self {
        Self {
            mode: auto_mode(),
            path: None,
            value_path: None,
            unit: None,
            multiplier: None,
        }
    }
}

/// Intentionally neither Debug nor Serialize, including when a probe fails.
pub struct DiscoveryInput {
    pub base_url: String,
    pub key: String,
    pub balance_config: BalanceConfig,
}

impl Drop for DiscoveryInput {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceSnapshot {
    pub status: String,
    pub remaining: Option<f64>,
    pub unit: String,
    pub source: String,
    pub checked_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Default for BalanceSnapshot {
    fn default() -> Self {
        balance_status("unsupported", Some("尚未查询余额。"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult {
    pub resolved_base_url: Option<String>,
    pub models: Vec<DiscoveredModel>,
    pub models_status: String,
    pub balance: BalanceSnapshot,
    pub checked_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for DiscoveryResult {
    fn default() -> Self {
        Self {
            resolved_base_url: None,
            models: Vec::new(),
            models_status: "unsupported".into(),
            balance: BalanceSnapshot::default(),
            checked_at: Utc::now().to_rfc3339(),
            error: None,
        }
    }
}

fn balance_status(status: &str, message: Option<&str>) -> BalanceSnapshot {
    BalanceSnapshot {
        status: status.into(),
        remaining: None,
        unit: "额度".into(),
        source: String::new(),
        checked_at: Utc::now().to_rfc3339(),
        message: message.map(str::to_owned),
    }
}

fn available(value: f64, unit: &str, source: &str) -> BalanceSnapshot {
    BalanceSnapshot {
        status: "available".into(),
        remaining: Some(value),
        unit: unit.into(),
        source: source.into(),
        checked_at: Utc::now().to_rfc3339(),
        message: None,
    }
}

/// Accept a base URL or a familiar resource URL, preserving any custom prefix.
pub fn normalize_base_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) || value.contains('\\') {
        return Err("请输入完整的 API 地址，且不要包含控制字符或反斜线。".into());
    }
    let mut url = Url::parse(value)
        .map_err(|_| "API 地址无法解析，请填写包含 https:// 的完整地址。".to_owned())?;
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if !url.username().is_empty() || url.password().is_some() || authority.contains('@') {
        return Err("API 地址不能包含用户名、密码或 Key，请使用独立的密钥输入框。".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("API 地址不能包含查询参数或 # 片段。".into());
    }
    if url.host().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && is_loopback(&url)))
    {
        return Err("远程 API 必须使用 HTTPS；HTTP 仅支持 localhost 或回环地址。".into());
    }
    let mut path = url.path().trim_end_matches('/').to_owned();
    for suffix in ["/chat/completions", "/responses", "/models"] {
        if path.ends_with(suffix) {
            path.truncate(path.len() - suffix.len());
            break;
        }
    }
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// First respect the supplied base, then try its single /v1 alternative.
pub fn model_base_candidates(normalized: &str) -> Vec<String> {
    let mut candidates = vec![normalized.to_owned()];
    let alternative = normalized
        .strip_suffix("/v1")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{normalized}/v1"));
    if alternative != normalized {
        candidates.push(alternative);
    }
    candidates
}

/// Validate this while saving so an invalid balance adapter never becomes active.
pub fn validate_balance_config(base: &str, config: &BalanceConfig) -> Result<(), String> {
    if !["auto", "disabled", "custom"].contains(&config.mode.as_str()) {
        return Err("余额查询方式无效。".into());
    }
    if config.mode != "custom" {
        return Ok(());
    }
    let base =
        Url::parse(&normalize_base_url(base)?).map_err(|_| "API 地址无法解析。".to_owned())?;
    custom_balance_url(&base, config.path.as_deref().unwrap_or(""))?;
    let path = config.value_path.as_deref().unwrap_or("").trim();
    if path.is_empty() || path.len() > 256 || path.chars().any(char::is_control) {
        return Err("请填写余额字段路径，例如 data.balance 或 /data/balance。".into());
    }
    let multiplier = config.multiplier.unwrap_or(1.0);
    if !multiplier.is_finite() || multiplier <= 0.0 {
        return Err("余额换算倍率必须是大于零的有限数值。".into());
    }
    if config
        .unit
        .as_deref()
        .is_some_and(|unit| unit.len() > 48 || unit.chars().any(char::is_control))
    {
        return Err("余额单位过长或包含不支持的字符。".into());
    }
    Ok(())
}

fn custom_balance_url(base: &Url, path: &str) -> Result<Url, String> {
    let path = path.trim();
    let invalid_segment = path.split('/').any(|segment| {
        let lowered = segment.to_ascii_lowercase().replace("%2e", ".");
        lowered == "." || lowered == ".."
    });
    if path.is_empty()
        || path.len() > 2048
        || path.starts_with("//")
        || path.contains(['\\', '?', '#'])
        || path.chars().any(char::is_control)
        || path.to_ascii_lowercase().contains("%2f")
        || path.to_ascii_lowercase().contains("%5c")
        || invalid_segment
        || Url::parse(path).is_ok()
    {
        return Err(
            "余额接口必须是同一服务的相对路径，不能包含完整网址、查询参数或路径跳转。".into(),
        );
    }
    let mut directory = base.clone();
    directory.set_path(&format!("{}/", base.path().trim_end_matches('/')));
    let resolved = directory
        .join(path)
        .map_err(|_| "无法解析余额接口路径。".to_owned())?;
    if resolved.origin() != base.origin()
        || !resolved.username().is_empty()
        || resolved.password().is_some()
    {
        return Err("余额接口必须与 API 地址同源。".into());
    }
    Ok(resolved)
}

#[derive(Clone, Copy, Debug)]
enum FetchError {
    Unsupported,
    Http(u16),
    Redirect,
    Transport,
    Timeout,
    Cancelled,
    TooLarge,
    Json,
}

impl FetchError {
    fn message(self) -> &'static str {
        match self {
            Self::Unsupported => "服务未提供可识别的查询接口。",
            Self::Http(401) => "认证失败，请检查此渠道的 API Key。",
            Self::Http(403) => "服务拒绝查询，请检查 Key 权限或服务商访问限制。",
            Self::Http(429) => "服务暂时限流，请稍后刷新。",
            Self::Http(_) => "服务返回错误状态，请稍后刷新或检查服务商状态。",
            Self::Redirect => "查询接口发生重定向；为避免密钥转发，已停止请求，请核对 API 地址。",
            Self::Transport => "无法连接查询接口，请检查网络、代理和服务商地址。",
            Self::Timeout => "查询超时，请稍后刷新。",
            Self::Cancelled => "已取消查询。",
            Self::TooLarge => "查询响应超过大小限制，请检查服务商接口。",
            Self::Json => "查询接口未返回可识别的 JSON 数据。",
        }
    }

    fn can_try_alternative(self) -> bool {
        matches!(self, Self::Unsupported | Self::Json)
    }
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            // A dropped sender does not mean cancellation. Timeout still bounds us.
            std::future::pending::<()>().await;
        }
    }
}

async fn fetch_json(
    client: &reqwest::Client,
    origin: &Url,
    url: Url,
    authorization: &HeaderValue,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Value, FetchError> {
    if url.origin() != origin.origin() {
        return Err(FetchError::Redirect);
    }
    let request = async {
        let response = client
            .get(url)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, authorization.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    FetchError::Timeout
                } else {
                    FetchError::Transport
                }
            })?;
        let status = response.status();
        if status.is_redirection() {
            return Err(FetchError::Redirect);
        }
        if matches!(status.as_u16(), 404 | 405 | 410 | 501) {
            return Err(FetchError::Unsupported);
        }
        if !status.is_success() {
            return Err(FetchError::Http(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err(FetchError::TooLarge);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| FetchError::Transport)?;
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let parsed = serde_json::from_slice(&body).map_err(|_| FetchError::Json);
        body.zeroize();
        parsed
    };
    tokio::select! {
        biased;
        _ = cancelled(cancel) => Err(FetchError::Cancelled),
        result = tokio::time::timeout(REQUEST_TIMEOUT, request) => {
            result.unwrap_or(Err(FetchError::Timeout))
        }
    }
}

fn safe_catalog_text(value: &str, key: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || (!key.is_empty() && value.contains(key))
    {
        None
    } else {
        Some(value.to_owned())
    }
}

fn parse_models(value: &Value, key: &str) -> Option<Vec<DiscoveredModel>> {
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let array = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.pointer("/data/models").and_then(Value::as_array))
        .or_else(|| value.as_array())?;
    let mut models: Vec<DiscoveredModel> = Vec::new();
    let mut indices = HashMap::<String, usize>::new();
    for entry in array.iter().take(MAX_MODELS) {
        let id = entry
            .as_str()
            .or_else(|| entry.get("id").and_then(Value::as_str))
            .or_else(|| entry.get("model").and_then(Value::as_str))
            .and_then(|id| safe_catalog_text(id, key));
        let Some(id) = id else { continue };
        let name = entry
            .get("name")
            .or_else(|| entry.get("display_name"))
            .and_then(Value::as_str)
            .and_then(|name| safe_catalog_text(name, key))
            .filter(|name| name != &id);
        if let Some(index) = indices.get(&id) {
            if models[*index].name.is_none() {
                models[*index].name = name;
            }
        } else {
            indices.insert(id.clone(), models.len());
            models.push(DiscoveredModel { id, name });
        }
    }
    if !array.is_empty() && models.is_empty() {
        None
    } else {
        Some(models)
    }
}

fn nonnegative_number(value: &Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))?;
    (number.is_finite() && number >= 0.0).then_some(number)
}

fn configured_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.starts_with('/') {
        return value.pointer(path);
    }
    let mut current = value;
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() || parts.len() > 32 {
        return None;
    }
    for part in parts {
        if part.is_empty() {
            return None;
        }
        current = if current.is_array() {
            current.get(part.parse::<usize>().ok()?)?
        } else {
            current.get(part)?
        };
    }
    Some(current)
}

fn parse_token_balance(value: &Value) -> Option<BalanceSnapshot> {
    if value
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0)
        || value.get("code").and_then(Value::as_bool) == Some(false)
        || value.get("success").and_then(Value::as_bool) == Some(false)
    {
        return None;
    }
    let data = value.get("data")?;
    if data.get("unlimited_quota").and_then(Value::as_bool) == Some(true) {
        let mut snapshot = balance_status("unlimited", Some("此 API Key 未设置额度上限。"));
        snapshot.source = "new-api-token".into();
        return Some(snapshot);
    }
    let remaining = data.get("total_available").and_then(nonnegative_number)?;
    let mut snapshot = available(remaining, "额度", "new-api-token");
    snapshot.message = Some("API Key 剩余额度；未使用未知兑换倍率换算货币。".into());
    Some(snapshot)
}

fn parse_credit_balance(value: &Value) -> Option<BalanceSnapshot> {
    if value.get("object").and_then(Value::as_str) != Some("credit_summary") {
        return None;
    }
    let remaining = value.get("total_available").and_then(nonnegative_number)?;
    // Compatible servers do not consistently declare this legacy endpoint's unit.
    let unit = value
        .get("currency")
        .and_then(Value::as_str)
        .filter(|currency| matches!(*currency, "USD" | "CNY" | "EUR" | "GBP"))
        .unwrap_or("额度");
    let mut snapshot = available(remaining, unit, "credit-grants");
    if unit == "额度" {
        snapshot.message = Some("服务未声明货币单位，按接口原始额度显示。".into());
    }
    Some(snapshot)
}

async fn discover_balance(
    client: &reqwest::Client,
    origin: &Url,
    base: &str,
    authorization: &HeaderValue,
    config: &BalanceConfig,
    cancel: &mut watch::Receiver<bool>,
) -> BalanceSnapshot {
    if config.mode == "disabled" {
        return balance_status("unsupported", Some("已关闭此渠道的余额查询。"));
    }
    let base = match Url::parse(base) {
        Ok(base) => base,
        Err(_) => return balance_status("error", Some("API 地址无法解析。")),
    };
    if config.mode == "custom" {
        let url = match custom_balance_url(&base, config.path.as_deref().unwrap_or("")) {
            Ok(url) => url,
            Err(_) => return balance_status("error", Some("余额接口路径无效。")),
        };
        return match fetch_json(client, origin, url, authorization, cancel).await {
            Ok(value) => {
                let remaining =
                    configured_value(&value, config.value_path.as_deref().unwrap_or(""))
                        .and_then(nonnegative_number)
                        .map(|value| value * config.multiplier.unwrap_or(1.0))
                        .filter(|value| value.is_finite() && *value >= 0.0);
                match remaining {
                    Some(remaining) => {
                        let unit = config
                            .unit
                            .as_deref()
                            .map(str::trim)
                            .filter(|unit| !unit.is_empty())
                            .unwrap_or("额度");
                        available(remaining, unit, "custom")
                    }
                    None => balance_status(
                        "error",
                        Some("未找到有效余额数值，请检查字段路径和换算倍率。"),
                    ),
                }
            }
            Err(error) => balance_status("error", Some(error.message())),
        };
    }

    // Public protocol references:
    // github.com/QuantumNous/new-api/blob/main/router/api-router.go
    // github.com/QuantumNous/new-api/blob/main/controller/billing.go
    // github.com/QuantumNous/new-api/blob/main/controller/token.go
    // github.com/songquanpeng/one-api/blob/main/controller/billing.go
    // Management endpoints are origin-relative; model custom prefixes are not removed.
    for (path, adapter) in [
        ("/api/usage/token/", 0),
        ("/dashboard/billing/credit_grants", 1),
        ("/dashboard/billing/subscription", 2),
    ] {
        let Ok(url) = origin.join(path) else { continue };
        match fetch_json(client, origin, url, authorization, cancel).await {
            Ok(value) => {
                let snapshot = match adapter {
                    0 => parse_token_balance(&value),
                    1 => parse_credit_balance(&value),
                    _ => None,
                };
                if let Some(snapshot) = snapshot {
                    return snapshot;
                }
                if adapter == 2
                    && value.get("object").and_then(Value::as_str) == Some("billing_subscription")
                {
                    let limit = value.get("hard_limit_usd").and_then(nonnegative_number);
                    if let Some(limit) = limit {
                        let Ok(usage_url) = origin.join("/dashboard/billing/usage") else {
                            continue;
                        };
                        match fetch_json(client, origin, usage_url, authorization, cancel).await {
                            Ok(usage) => {
                                if let Some(used) =
                                    usage.get("total_usage").and_then(nonnegative_number)
                                {
                                    // New API's billing compatibility endpoint scales usage by 100.
                                    // Its configured display units may be money OR tokens.
                                    let remaining = (limit - used / 100.0).max(0.0);
                                    let mut snapshot =
                                        available(remaining, "站点计费单位", "subscription-usage");
                                    snapshot.message = Some("按服务的订阅额度与用量计算；该兼容接口未可靠声明货币单位。".into());
                                    return snapshot;
                                }
                            }
                            Err(error) if error.can_try_alternative() => {}
                            Err(error) => return balance_status("error", Some(error.message())),
                        }
                    }
                }
            }
            Err(error) if error.can_try_alternative() => {}
            Err(error) => return balance_status("error", Some(error.message())),
        }
    }
    balance_status(
        "unsupported",
        Some("此服务未提供已识别的余额接口，可在高级设置配置查询路径。"),
    )
}

/// Performs at most two model requests and four automatic balance requests.
/// All requests are GET, share an origin, do not redirect, and obey cancellation.
pub async fn discover(input: DiscoveryInput, mut cancel: watch::Receiver<bool>) -> DiscoveryResult {
    let mut result = DiscoveryResult {
        resolved_base_url: None,
        models: Vec::new(),
        models_status: "error".into(),
        balance: balance_status("unsupported", Some("尚未查询余额。")),
        checked_at: Utc::now().to_rfc3339(),
        error: None,
    };
    let base = match normalize_base_url(&input.base_url) {
        Ok(base) => base,
        Err(error) => {
            result.error = Some(error);
            return result;
        }
    };
    if let Err(error) = validate_balance_config(&base, &input.balance_config) {
        result.error = Some(error);
        return result;
    }
    if input.key.trim().is_empty() || input.key.chars().any(char::is_control) {
        result.error = Some("请填写有效的 API Key。".into());
        return result;
    }
    let mut authorization_text = format!("Bearer {}", input.key.trim());
    let authorization = HeaderValue::from_str(&authorization_text);
    authorization_text.zeroize();
    let mut authorization = match authorization {
        Ok(value) => value,
        Err(_) => {
            result.error = Some("API Key 包含不支持的字符。".into());
            return result;
        }
    };
    authorization.set_sensitive(true);
    let origin = match Url::parse(&base) {
        Ok(url) => url,
        Err(_) => {
            result.error = Some("API 地址无法解析。".into());
            return result;
        }
    };
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(REQUEST_TIMEOUT)
        .user_agent("Vela/0.2");
    if is_loopback(&origin) {
        builder = builder.no_proxy();
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => {
            result.error = Some("无法创建安全网络连接。".into());
            return result;
        }
    };
    let work = async {
        result.models_status = "unsupported".into();
        for candidate in model_base_candidates(&base) {
            let Ok(url) = Url::parse(&format!("{candidate}/models")) else {
                continue;
            };
            match fetch_json(&client, &origin, url, &authorization, &mut cancel).await {
                Ok(value) => {
                    if let Some(models) = parse_models(&value, input.key.trim()) {
                        result.resolved_base_url = Some(candidate);
                        result.models = models;
                        result.models_status = "ready".into();
                        break;
                    }
                }
                Err(error) if error.can_try_alternative() => {}
                Err(error) => {
                    result.models_status = "error".into();
                    result.error = Some(error.message().into());
                    result.balance =
                        balance_status("error", Some("模型查询未完成，余额请求已停止。"));
                    return;
                }
            }
        }
        if result.models_status == "unsupported" {
            result.error = Some("此服务未提供可识别的模型列表；仍可手动填写模型名称。".into());
        }
        result.balance = discover_balance(
            &client,
            &origin,
            result.resolved_base_url.as_deref().unwrap_or(&base),
            &authorization,
            &input.balance_config,
            &mut cancel,
        )
        .await;
    };
    if tokio::time::timeout(DISCOVERY_TIMEOUT, work).await.is_err() {
        if result.models_status != "ready" {
            result.models_status = "error".into();
            result.error = Some(FetchError::Timeout.message().into());
        }
        result.balance = balance_status("error", Some("余额查询超时，已保留已发现的模型。"));
    }
    result.checked_at = Utc::now().to_rfc3339();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Instant,
    };

    struct Reply {
        status: u16,
        body: String,
        headers: String,
        delay: Duration,
    }

    impl Reply {
        fn json(status: u16, value: Value) -> Self {
            Self {
                status,
                body: value.to_string(),
                headers: String::new(),
                delay: Duration::ZERO,
            }
        }
    }

    struct Server {
        base: String,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl Server {
        fn new(replies: Vec<Reply>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let thread_requests = requests.clone();
            let thread_stop = stop.clone();
            let thread = thread::spawn(move || {
                let mut replies = replies.into_iter();
                while !thread_stop.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(stream) => stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(3));
                            continue;
                        }
                        Err(_) => break,
                    };
                    // Windows accepted sockets inherit listener nonblocking mode.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 1024];
                    let mut complete = false;
                    loop {
                        match stream.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(length) => bytes.extend_from_slice(&buffer[..length]),
                        }
                        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                            complete = true;
                            break;
                        }
                    }
                    if !complete {
                        continue;
                    }
                    thread_requests
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&bytes).into_owned());
                    let Some(reply) = replies.next() else { break };
                    let until = Instant::now() + reply.delay;
                    while Instant::now() < until && !thread_stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(3));
                    }
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}", reply.status, reply.body.len(), reply.headers, reply.body);
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(150)));
                    while matches!(stream.read(&mut buffer), Ok(count) if count > 0) {}
                }
            });
            Self {
                base,
                requests,
                stop,
                thread: Some(thread),
            }
        }

        fn paths(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.split_whitespace().nth(1).unwrap().into())
                .collect()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    fn input(base: &str) -> DiscoveryInput {
        DiscoveryInput {
            base_url: base.into(),
            key: "fake-discovery-key".into(),
            balance_config: BalanceConfig {
                mode: "disabled".into(),
                ..Default::default()
            },
        }
    }

    async fn run(input: DiscoveryInput) -> DiscoveryResult {
        let (_sender, cancel) = watch::channel(false);
        discover(input, cancel).await
    }

    #[test]
    fn normalizes_resource_suffix_and_retains_custom_path() {
        for suffix in ["", "/", "/responses", "/models", "/chat/completions"] {
            assert_eq!(
                normalize_base_url(&format!("https://example.com/proxy/v1{suffix}")).unwrap(),
                "https://example.com/proxy/v1"
            );
        }
        assert_eq!(
            normalize_base_url("https://example.com/responses").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            model_base_candidates("https://example.com/v1"),
            ["https://example.com/v1", "https://example.com"]
        );
        assert_eq!(
            model_base_candidates("https://example.com/proxy"),
            ["https://example.com/proxy", "https://example.com/proxy/v1"]
        );
        for invalid in [
            "http://example.com",
            "https://user:password@example.com",
            "https://example.com?key=secret",
            "https://example.com#key",
            "https://example.com\\path",
        ] {
            assert!(normalize_base_url(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn model_catalog_deduplicates_preserving_names_and_rejects_secret_echoes() {
        let models = parse_models(&json!({"data":[{"id":"gpt-codex"},{"id":"gpt-codex","name":"Coding model"},"other",{"id":"secret-key"},{"id":"bad\nmodel"}]}), "secret-key").unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name.as_deref(), Some("Coding model"));
        assert!(parse_models(&json!({"data":[{"name":"a label without id"}]}), "").is_none());
        assert!(parse_models(&json!({"data":[]}), "").unwrap().is_empty());
    }

    #[test]
    fn custom_adapter_rejects_external_and_ambiguous_paths() {
        let base = Url::parse("https://example.com/custom/v1").unwrap();
        for invalid in [
            "https://evil.example/balance",
            "//evil.example/balance",
            "/a/../b",
            "/a/%2e%2e/b",
            "/a%2fb",
            "/balance?key=secret",
            "/balance#x",
            "/a\\b",
        ] {
            assert!(custom_balance_url(&base, invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            custom_balance_url(&base, "/balance").unwrap().as_str(),
            "https://example.com/balance"
        );
        assert_eq!(
            custom_balance_url(&base, "balance").unwrap().as_str(),
            "https://example.com/custom/v1/balance"
        );
        let body = json!({"data":{"items":[{"balance":"25.2"}]}});
        assert_eq!(
            configured_value(&body, "data.items.0.balance").and_then(nonnegative_number),
            Some(25.2)
        );
        assert_eq!(
            configured_value(&body, "/data/items/0/balance").and_then(nonnegative_number),
            Some(25.2)
        );
        for invalid in [json!("NaN"), json!("inf"), json!(-1), json!(null)] {
            assert!(nonnegative_number(&invalid).is_none());
        }
    }

    #[tokio::test]
    async fn falls_back_to_v1_only_after_missing_root_catalog() {
        let server = Server::new(vec![
            Reply::json(404, json!({})),
            Reply::json(200, json!({"data":[{"id":"code","name":"Code"}]})),
        ]);
        let result = run(input(&server.base)).await;
        assert_eq!(result.models_status, "ready");
        assert_eq!(
            result.resolved_base_url,
            Some(format!("{}/v1", server.base))
        );
        assert_eq!(server.paths(), ["/models", "/v1/models"]);
    }

    #[tokio::test]
    async fn v1_can_fall_back_to_root_without_duplicating_version_segment() {
        let server = Server::new(vec![
            Reply::json(404, json!({})),
            Reply::json(200, json!({"data":["code"]})),
        ]);
        let result = run(input(&format!("{}/custom/v1/models", server.base))).await;
        assert_eq!(result.models_status, "ready");
        assert_eq!(
            result.resolved_base_url,
            Some(format!("{}/custom", server.base))
        );
        assert_eq!(server.paths(), ["/custom/v1/models", "/custom/models"]);
    }

    #[tokio::test]
    async fn unidentified_catalog_does_not_invent_models_or_resolved_base() {
        let server = Server::new(vec![
            Reply::json(200, json!({"message":"request accepted"})),
            Reply::json(404, json!({})),
        ]);
        let result = run(input(&server.base)).await;
        assert_eq!(result.models_status, "unsupported");
        assert_eq!(result.resolved_base_url, None);
        assert!(result.models.is_empty());
    }

    #[tokio::test]
    async fn successful_root_and_custom_catalog_do_not_probe_alternatives() {
        for path in ["", "/custom/prefix", "/custom/v1/responses"] {
            let server = Server::new(vec![Reply::json(200, json!({"models":["code"]}))]);
            let result = run(input(&format!("{}{path}", server.base))).await;
            assert_eq!(
                result.models_status,
                "ready",
                "path={path}, result={result:?}, requests={:?}",
                server.paths()
            );
            let path = path.strip_suffix("/responses").unwrap_or(path);
            assert_eq!(server.paths(), [format!("{path}/models")]);
        }
    }

    #[tokio::test]
    async fn authentication_and_rate_limits_stop_all_probes_without_echoing_body() {
        for status in [401, 403, 429] {
            let server = Server::new(vec![Reply::json(
                status,
                json!({"error":"Bearer fake-discovery-key"}),
            )]);
            let mut request = input(&server.base);
            request.balance_config = BalanceConfig::default();
            let result = run(request).await;
            assert_eq!(server.paths(), ["/models"]);
            assert_eq!(result.models_status, "error");
            assert!(!serde_json::to_string(&result)
                .unwrap()
                .contains("fake-discovery-key"));
        }
    }

    #[tokio::test]
    async fn redirect_is_never_followed_or_retried() {
        let mut redirect = Reply::json(307, json!({}));
        redirect.headers = "Location: http://127.0.0.1:9/steal\r\n".into();
        let server = Server::new(vec![redirect]);
        let result = run(input(&server.base)).await;
        assert_eq!(server.paths(), ["/models"]);
        assert_eq!(result.models_status, "error");
        assert!(result.error.unwrap().contains("重定向"));
    }

    #[tokio::test]
    async fn new_api_uses_actual_remaining_quota_without_inventing_currency() {
        let server = Server::new(vec![
            Reply::json(200, json!({"data":["code"]})),
            Reply::json(
                200,
                json!({"code":true,"data":{"object":"token_usage","total_available":1500000,"total_granted":2000000,"total_used":500000,"unlimited_quota":false}}),
            ),
        ]);
        let mut request = input(&server.base);
        request.balance_config = BalanceConfig::default();
        let result = run(request).await;
        assert_eq!(result.balance.remaining, Some(1500000.0));
        assert_eq!(result.balance.unit, "额度");
        assert_eq!(result.balance.source, "new-api-token");
        assert_eq!(server.paths(), ["/models", "/api/usage/token/"]);
    }

    #[test]
    fn unlimited_is_distinct_from_zero_and_credit_grants_preserves_raw_quota() {
        let balance = parse_token_balance(
            &json!({"code":0,"data":{"total_available":0,"unlimited_quota":true}}),
        )
        .unwrap();
        assert_eq!(balance.status, "unlimited");
        assert_eq!(balance.remaining, None);
        let balance =
            parse_credit_balance(&json!({"object":"credit_summary","total_available":500000}))
                .unwrap();
        assert_eq!(balance.remaining, Some(500000.0));
        assert_eq!(balance.unit, "额度");
        assert!(parse_token_balance(&json!({"code":1,"data":{"total_available":0}})).is_none());
    }

    #[tokio::test]
    async fn unsupported_balances_never_become_zero_or_break_model_discovery() {
        let server = Server::new(vec![
            Reply::json(200, json!({"data":[]})),
            Reply::json(404, json!({})),
            Reply::json(404, json!({})),
            Reply::json(404, json!({})),
        ]);
        let mut request = input(&server.base);
        request.balance_config = BalanceConfig::default();
        let result = run(request).await;
        assert_eq!(result.models_status, "ready");
        assert_eq!(result.balance.status, "unsupported");
        assert_eq!(result.balance.remaining, None);
        assert_eq!(server.paths().len(), 4);
    }

    #[tokio::test]
    async fn custom_balance_applies_only_explicit_units_and_multiplier() {
        let server = Server::new(vec![
            Reply::json(200, json!({"data":["code"]})),
            Reply::json(200, json!({"data":{"remaining":"2500"}})),
        ]);
        let mut request = input(&format!("{}/custom/v1", server.base));
        request.balance_config = BalanceConfig {
            mode: "custom".into(),
            path: Some("/account/balance".into()),
            value_path: Some("data.remaining".into()),
            unit: Some("CNY".into()),
            multiplier: Some(0.01),
        };
        let result = run(request).await;
        assert_eq!(result.balance.remaining, Some(25.0));
        assert_eq!(result.balance.unit, "CNY");
        assert_eq!(server.paths(), ["/custom/v1/models", "/account/balance"]);
    }

    #[tokio::test]
    async fn subscription_adapter_uses_documented_usage_scaling_without_dollar_assumption() {
        let server = Server::new(vec![
            Reply::json(200, json!({"data":["code"]})),
            Reply::json(404, json!({})),
            Reply::json(404, json!({})),
            Reply::json(
                200,
                json!({"object":"billing_subscription","hard_limit_usd":20}),
            ),
            Reply::json(200, json!({"object":"list","total_usage":650})),
        ]);
        let mut request = input(&server.base);
        request.balance_config = BalanceConfig::default();
        let result = run(request).await;
        assert_eq!(result.balance.remaining, Some(13.5));
        assert_eq!(result.balance.unit, "站点计费单位");
        assert_eq!(result.balance.source, "subscription-usage");
        assert_eq!(server.paths().len(), 5);
    }

    #[tokio::test]
    async fn bounded_body_does_not_leak_or_attempt_other_routes() {
        let mut oversized = Reply::json(200, json!({}));
        oversized.body = " ".repeat(MAX_RESPONSE_BYTES + 1);
        let server = Server::new(vec![oversized]);
        let result = run(input(&server.base)).await;
        assert_eq!(result.models_status, "error");
        assert!(result.error.unwrap().contains("大小限制"));
        assert_eq!(server.paths().len(), 1);
    }

    #[tokio::test]
    async fn cancellation_interrupts_active_request() {
        let mut slow = Reply::json(200, json!({"data":[]}));
        slow.delay = Duration::from_secs(5);
        let server = Server::new(vec![slow]);
        let (sender, cancel) = watch::channel(false);
        let requests = server.requests.clone();
        let trigger = async move {
            for _ in 0..100 {
                if !requests.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            sender.send(true).unwrap();
        };
        let start = Instant::now();
        let (result, ()) = tokio::join!(discover(input(&server.base), cancel), trigger);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(result.error.unwrap().contains("取消"));
        assert_eq!(server.paths().len(), 1);
    }
}
