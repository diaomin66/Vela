//! Authenticated loopback routing for Codex's native Responses client.
//!
//! The catalog contains references to credentials, never credentials themselves.
//! A route is selected exactly once; inference requests are never retried or redirected.

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, Request, Response, StatusCode},
    routing::any,
    Router,
};
use futures_util::{stream, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};
use zeroize::Zeroizing;

pub const DEFAULT_PORT: u16 = 18761;
const MAX_BODY: usize = 32 * 1024 * 1024;
const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_CONTINUATIONS: usize = 4096;
const CONTINUATION_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const UPSTREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRoute {
    pub route_id: String,
    pub display_name: String,
    pub profile_id: String,
    pub upstream_model: String,
    pub base_url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCatalog {
    pub entries: Vec<GatewayRoute>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub running: bool,
    pub port: u16,
    pub endpoint: String,
    pub model_count: usize,
}

type SecretReader = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

#[derive(Clone)]
struct GatewayState {
    catalog: Arc<RwLock<GatewayCatalog>>,
    token: Arc<Zeroizing<String>>,
    client: reqwest::Client,
    secrets: SecretReader,
    continuations: Arc<Mutex<Continuations>>,
    stop: watch::Receiver<bool>,
    port: u16,
}

pub struct GatewayHandle {
    state: GatewayState,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl GatewayHandle {
    pub fn update_catalog(&self, catalog: GatewayCatalog) -> Result<(), String> {
        validate_catalog(&catalog)?;
        let mut current = self
            .state
            .catalog
            .write()
            .map_err(|_| "模型目录暂时不可用。")?;
        // An existing alias must never silently move to another service or model.
        for new in &catalog.entries {
            if let Some(old) = current
                .entries
                .iter()
                .find(|old| old.route_id == new.route_id)
            {
                if old.profile_id != new.profile_id
                    || !same_channel_endpoint(&old.base_url, &new.base_url)
                    || old.upstream_model != new.upstream_model
                {
                    return Err("已有模型标识不能改派到其他渠道；请生成新的模型标识。".into());
                }
            }
        }
        *current = catalog;
        Ok(())
    }

    pub fn status(&self) -> GatewayStatus {
        GatewayStatus {
            running: self.task.as_ref().is_some_and(|task| !task.is_finished())
                && !*self.state.stop.borrow(),
            port: self.state.port,
            endpoint: format!("http://127.0.0.1:{}/v1", self.state.port),
            model_count: self
                .state
                .catalog
                .read()
                .map(|c| c.entries.len())
                .unwrap_or(0),
        }
    }

    pub async fn shutdown(&mut self) {
        let _ = self.stop.send(true);
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(Duration::from_secs(3), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
    }
}

fn same_channel_endpoint(old: &str, new: &str) -> bool {
    if old == new {
        return true;
    }
    let (Ok(old), Ok(new)) = (url::Url::parse(old), url::Url::parse(new)) else {
        return false;
    };
    let without_version = |path: &str| {
        path.trim_end_matches('/')
            .strip_suffix("/v1")
            .unwrap_or(path.trim_end_matches('/'))
            .to_owned()
    };
    old.origin() == new.origin() && without_version(old.path()) == without_version(new.path())
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

pub async fn start(
    catalog: GatewayCatalog,
    token: String,
    port: u16,
) -> Result<GatewayHandle, String> {
    start_with_secrets(catalog, token, port, Arc::new(crate::security::get_secret)).await
}

async fn start_with_secrets(
    catalog: GatewayCatalog,
    token: String,
    port: u16,
    secrets: SecretReader,
) -> Result<GatewayHandle, String> {
    validate_catalog(&catalog)?;
    let token = Zeroizing::new(token);
    if token.len() < 32
        || token.len() > 512
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("本机路由凭据格式无效，请重新生成。".into());
    }
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .map_err(|_| format!("本机路由端口 {port} 无法使用，可能被其他程序占用。"))?;
    let port = listener
        .local_addr()
        .map_err(|_| "无法读取本机路由端口。")?
        .port();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(UPSTREAM_IDLE_TIMEOUT)
        .user_agent("Vela/0.2")
        .build()
        .map_err(|_| "无法初始化安全网络连接。")?;
    let (stop, receiver) = watch::channel(false);
    let state = GatewayState {
        catalog: Arc::new(RwLock::new(catalog)),
        token: Arc::new(token),
        client,
        secrets,
        continuations: Arc::new(Mutex::new(Continuations::default())),
        stop: receiver,
        port,
    };
    let router = Router::new()
        .fallback(any(dispatch))
        .with_state(state.clone());
    let mut shutdown = state.stop.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                while !*shutdown.borrow() {
                    if shutdown.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
    });
    Ok(GatewayHandle {
        state,
        stop,
        task: Some(task),
    })
}

fn validate_catalog(catalog: &GatewayCatalog) -> Result<(), String> {
    let mut aliases = std::collections::HashSet::new();
    for route in &catalog.entries {
        if route.route_id.is_empty()
            || route.route_id.len() > 1024
            || route.route_id.chars().any(char::is_control)
            || route.upstream_model.is_empty()
            || route.upstream_model.len() > 1024
            || route.upstream_model.chars().any(char::is_control)
            || route.display_name.is_empty()
            || route.display_name.len() > 2048
            || !aliases.insert(&route.route_id)
        {
            return Err("模型目录包含无效或重复的标识。".into());
        }
        crate::core::validate_id(&route.profile_id)?;
        let normalized = crate::diagnostics::normalize_endpoint(&route.base_url)?;
        if normalized != route.base_url {
            return Err("模型目录中的 API 地址需要先规范化。".into());
        }
    }
    Ok(())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn authorize(headers: &HeaderMap, state: &GatewayState) -> Result<(), Response<Body>> {
    if headers.contains_key(header::ORIGIN) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "browser_origin_blocked",
            "Vela 本机路由不接受浏览器跨来源请求。",
        ));
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if host != format!("127.0.0.1:{}", state.port) && host != format!("localhost:{}", state.port) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "本机路由地址无效。",
        ));
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_equal(bearer.as_bytes(), state.token.as_bytes()) {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "invalid_local_credential",
            "本机路由凭据无效，请在 Vela 中重新应用连接。",
        ));
    }
    Ok(())
}

async fn dispatch(State(state): State<GatewayState>, request: Request<Body>) -> Response<Body> {
    if let Err(response) = authorize(request.headers(), &state) {
        return response;
    }
    if *state.stop.borrow() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "gateway_stopping",
            "Vela 本机路由正在退出。",
        );
    }
    if request.uri().query().is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "query_not_supported",
            "本机路由入口不接受查询参数。",
        );
    }
    let path = request.uri().path();
    if request.method() == Method::GET && path == "/health" {
        return json_response(
            StatusCode::OK,
            json!({"running":true,"service":"Vela","port":state.port}),
        );
    }
    if request.method() == Method::GET && matches!(path, "/v1/models" | "/models") {
        return match state.catalog.read() {
            Ok(catalog) => json_response(
                StatusCode::OK,
                json!({"object":"list", "data": catalog.entries.iter().map(|route| {
                json!({"id":route.route_id,"object":"model","owned_by":"Vela","name":route.display_name,"display_name":route.display_name})
            }).collect::<Vec<_>>()}),
            ),
            Err(_) => error(
                StatusCode::SERVICE_UNAVAILABLE,
                "catalog_unavailable",
                "模型目录暂时不可用。",
            ),
        };
    }
    let compact = matches!(path, "/v1/responses/compact" | "/responses/compact");
    if request.method() != Method::POST
        || !(compact || matches!(path, "/v1/responses" | "/responses"))
    {
        return error(
            StatusCode::NOT_FOUND,
            "unsupported_endpoint",
            "Vela 仅提供 Models、Responses 和 Responses Compact 接口。",
        );
    }
    forward(state, request, compact).await
}

async fn forward(state: GatewayState, request: Request<Body>, compact: bool) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, MAX_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_body_invalid",
                "请求内容无法读取或超过 32 MiB。",
            )
        }
    };
    let mut payload: Value = match serde_json::from_slice(&bytes) {
        Ok(Value::Object(value)) => Value::Object(value),
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "请求必须是 JSON 对象。",
            )
        }
    };
    let route_id = match payload.get("model").and_then(Value::as_str) {
        Some(value) => value.to_owned(),
        None => {
            return error(
                StatusCode::BAD_REQUEST,
                "missing_model",
                "请选择 Vela 模型目录中的模型。",
            )
        }
    };
    let route = state.catalog.read().ok().and_then(|catalog| {
        catalog
            .entries
            .iter()
            .find(|r| r.route_id == route_id)
            .cloned()
    });
    let Some(route) = route else {
        return error(
            StatusCode::NOT_FOUND,
            "unknown_model_route",
            "此渠道模型已移除或尚未启用，请重新选择模型。",
        );
    };
    if let Some(previous) = payload.get("previous_response_id").filter(|v| !v.is_null()) {
        let Some(previous) = previous.as_str() else {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_previous_response_id",
                "previous_response_id 必须是字符串。",
            );
        };
        let matches = state
            .continuations
            .lock()
            .map(|mut ids| ids.matches(previous, &route))
            .unwrap_or(false);
        if !matches {
            return error(StatusCode::CONFLICT, "continuation_route_mismatch", "此续传记录不属于当前渠道模型或已过期。请在 Codex 中开启新对话，避免将上下文发送到错误渠道。");
        }
    }
    let streaming = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if compact && streaming {
        return error(
            StatusCode::BAD_REQUEST,
            "compact_stream_unsupported",
            "Compact 接口不支持流式响应。",
        );
    }
    payload["model"] = Value::String(route.upstream_model.clone());
    let key = match (state.secrets)(&route.profile_id) {
        Ok(key) if !key.is_empty() => Zeroizing::new(key),
        _ => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "channel_credential_missing",
                "渠道凭据无法读取，请在 Vela 中更新该渠道 Key。",
            )
        }
    };
    let authorization = match HeaderValue::from_str(&format!("Bearer {}", key.as_str())) {
        Ok(mut value) => {
            value.set_sensitive(true);
            value
        }
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "channel_credential_invalid",
                "渠道凭据格式无效，请更新 Key。",
            )
        }
    };
    let suffix = if compact {
        "/responses/compact"
    } else {
        "/responses"
    };
    let mut outgoing = state
        .client
        .post(format!("{}{suffix}", route.base_url))
        .header(header::AUTHORIZATION, authorization)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::ACCEPT,
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
        );
    // Never forward downstream credentials, cookies, Origin, proxy headers or hop-by-hop headers.
    for name in [
        "openai-beta",
        "session_id",
        "conversation_id",
        "originator",
        "x-codex-thread-id",
        "x-codex-turn-id",
        "x-codex-turn-metadata",
        "x-codex-parent-thread-id",
        "x-codex-session-id",
        "x-codex-originator",
        "x-codex-features",
        "x-codex-beta-features",
        "x-openai-subagent",
        "x-openai-internal-codex-residency",
    ] {
        if let Some(value) = parts.headers.get(name) {
            if value.as_bytes().len() <= 8192 {
                outgoing = outgoing.header(name, value);
            }
        }
    }
    // Sticky transport state belongs to the service that issued it, like response IDs.
    if let Some(value) = parts.headers.get("x-codex-turn-state") {
        if let Ok(text) = value.to_str() {
            if text.len() <= 8192
                && state
                    .continuations
                    .lock()
                    .is_ok_and(|mut ids| ids.matches(&format!("turn-state:{text}"), &route))
            {
                outgoing = outgoing.header("x-codex-turn-state", value);
            }
        }
    }
    let mut stop = state.stop.clone();
    let upstream = tokio::select! {
        result = outgoing.json(&payload).send() => match result {
            Ok(response) => response,
            Err(_) => return error(StatusCode::BAD_GATEWAY, "upstream_unreachable", "无法连接到当前渠道。请在 Vela 中检查网络与渠道状态。"),
        },
        _ = stop.changed() => return error(StatusCode::SERVICE_UNAVAILABLE, "gateway_stopping", "Vela 本机路由已停止。"),
    };
    let status = upstream.status();
    if !status.is_success() {
        // Upstream errors can contain an echoed Authorization header or customer data.
        return upstream_error(status);
    }
    let safe_headers = response_headers(upstream.headers(), &route, &key, &state);
    let is_sse = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        });
    if streaming {
        if !is_sse {
            return error(
                StatusCode::BAD_GATEWAY,
                "upstream_stream_incompatible",
                "当前渠道未返回 Responses 流式事件，请检查渠道兼容性。",
            );
        }
        let mut response = streaming_response(upstream, route, key, state);
        response.headers_mut().extend(safe_headers);
        return response;
    }
    let mut chunks = upstream.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let next = tokio::select! {
            result = chunks.next() => result,
            _ = stop.changed() => return error(StatusCode::SERVICE_UNAVAILABLE, "gateway_stopping", "Vela 本机路由已停止。"),
        };
        match next {
            Some(Ok(chunk)) if bytes.len().saturating_add(chunk.len()) <= MAX_BODY => {
                bytes.extend_from_slice(&chunk)
            }
            Some(_) => {
                return error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_response_invalid",
                    "渠道响应中断或超过 32 MiB。",
                )
            }
            None => break,
        }
    }
    let mut value: Value = match serde_json::from_slice(&bytes) {
        Ok(value @ Value::Object(_)) => value,
        _ => {
            return error(
                StatusCode::BAD_GATEWAY,
                "upstream_response_invalid",
                "渠道未返回有效的 Responses JSON。",
            )
        }
    };
    process_response_value(&mut value, &route, &key, &state);
    let mut response = json_response(status, value);
    response.headers_mut().extend(safe_headers);
    response
}

fn response_headers(
    headers: &HeaderMap,
    route: &GatewayRoute,
    key: &str,
    state: &GatewayState,
) -> HeaderMap {
    let mut allowed = HeaderMap::new();
    for name in ["x-request-id", "x-codex-turn-state"] {
        if let Some(value) = headers.get(name) {
            if let Ok(text) = value.to_str() {
                if text.len() <= 8192 && !text.contains(key) {
                    allowed.insert(name, value.clone());
                    if name == "x-codex-turn-state" {
                        if let Ok(mut ids) = state.continuations.lock() {
                            ids.remember(&format!("turn-state:{text}"), route);
                        }
                    }
                }
            }
        }
    }
    allowed
}

fn process_response_value(
    value: &mut Value,
    route: &GatewayRoute,
    key: &str,
    state: &GatewayState,
) {
    redact_protocol_errors(value, key);
    // Keep the public model alias stable across responses and subsequent Codex turns.
    if value.get("model").is_some() {
        value["model"] = Value::String(route.route_id.clone());
    }
    if let Some(response) = value.get_mut("response") {
        if response.get("model").is_some() {
            response["model"] = Value::String(route.route_id.clone());
        }
        remember_response(response, route, state);
    }
    remember_response(value, route, state);
}

fn redact_protocol_errors(value: &mut Value, key: &str) {
    // Local services commonly use placeholder keys such as "local" or "test".
    // Replacing them throughout successful text/tool arguments corrupts normal output.
    // Provider errors, where authentication details can be echoed, are the boundary.
    if value.get("type").and_then(Value::as_str) == Some("error") {
        redact_value(value, key);
        return;
    }
    if let Some(error) = value.get_mut("error") {
        redact_value(error, key);
    }
    if let Some(error) = value
        .get_mut("response")
        .and_then(|response| response.get_mut("error"))
    {
        redact_value(error, key);
    }
}

fn remember_response(value: &Value, route: &GatewayRoute, state: &GatewayState) {
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return;
    };
    if id.len() <= 512
        && (value.get("object").and_then(Value::as_str) == Some("response")
            || id.starts_with("resp_"))
    {
        if let Ok(mut ids) = state.continuations.lock() {
            ids.remember(id, route);
        }
    }
}

fn redact_value(value: &mut Value, key: &str) {
    match value {
        Value::String(text) => {
            if text.contains(key) {
                *text = text.replace(key, "[redacted]");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redact_value(item, key)),
        Value::Object(object) => {
            for item in object.values_mut() {
                redact_value(item, key);
            }
            let names: Vec<_> = object
                .keys()
                .filter(|name| name.contains(key))
                .cloned()
                .collect();
            for name in names {
                if let Some(value) = object.remove(&name) {
                    object.insert(name.replace(key, "[redacted]"), value);
                }
            }
        }
        _ => {}
    }
}

type UpstreamStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;
struct StreamState {
    upstream: UpstreamStream,
    buffer: Vec<u8>,
    route: GatewayRoute,
    key: Zeroizing<String>,
    state: GatewayState,
    ended: bool,
}

fn streaming_response(
    upstream: reqwest::Response,
    route: GatewayRoute,
    key: Zeroizing<String>,
    state: GatewayState,
) -> Response<Body> {
    let context = StreamState {
        upstream: Box::pin(upstream.bytes_stream()),
        buffer: Vec::new(),
        route,
        key,
        state,
        ended: false,
    };
    // Pull-driven stream: dropping the Codex connection immediately drops the upstream body.
    let body = stream::unfold(context, |mut context| async move {
        loop {
            if context.ended {
                return None;
            }
            if let Some((end, delimiter)) = frame_boundary(&context.buffer) {
                if end > MAX_FRAME {
                    context.ended = true;
                    return Some((Ok::<Bytes, Infallible>(stream_error()), context));
                }
                let frame: Vec<_> = context.buffer.drain(..end + delimiter).collect();
                match transform_frame(&frame, &context.route, &context.key, &context.state) {
                    Ok(bytes) => return Some((Ok(Bytes::from(bytes)), context)),
                    Err(()) => {
                        context.ended = true;
                        return Some((Ok(stream_error()), context));
                    }
                }
            }
            if context.buffer.len() > MAX_FRAME {
                context.ended = true;
                return Some((Ok(stream_error()), context));
            }
            let next = tokio::select! {
                chunk = context.upstream.next() => chunk,
                _ = context.state.stop.changed() => { context.ended = true; return Some((Ok(stream_error()), context)); },
            };
            match next {
                Some(Ok(chunk)) => context.buffer.extend_from_slice(&chunk),
                Some(Err(_)) => {
                    context.ended = true;
                    return Some((Ok(stream_error()), context));
                }
                None => {
                    context.ended = true;
                    if !context.buffer.iter().all(u8::is_ascii_whitespace) {
                        // A truncated frame is an incomplete response, never successful EOF.
                        return Some((Ok(stream_error()), context));
                    }
                    return None;
                }
            }
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(body))
        .expect("constant valid SSE response headers")
}

fn frame_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    let unix = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let windows = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (unix, windows) {
        (Some(a), Some(b)) => Some(if a.0 < b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

fn transform_frame(
    frame: &[u8],
    route: &GatewayRoute,
    key: &str,
    state: &GatewayState,
) -> Result<Vec<u8>, ()> {
    let text = std::str::from_utf8(frame).map_err(|_| ())?;
    let mut event = None;
    let mut data = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().replace(key, "[redacted]"));
        }
        if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    if data.is_empty() {
        return Ok(b": keep-alive\n\n".to_vec());
    }
    let data = data.join("\n");
    if data.trim() == "[DONE]" {
        return Ok(b"data: [DONE]\n\n".to_vec());
    }
    let mut value: Value = serde_json::from_str(&data).map_err(|_| ())?;
    if event.as_deref() == Some("error") {
        redact_value(&mut value, key);
    }
    process_response_value(&mut value, route, key, state);
    let event_prefix = event
        .map(|name| format!("event: {name}\n"))
        .unwrap_or_default();
    Ok(format!(
        "{event_prefix}data: {}\n\n",
        serde_json::to_string(&value).map_err(|_| ())?
    )
    .into_bytes())
}

fn stream_error() -> Bytes {
    Bytes::from_static(b"event: error\ndata: {\"type\":\"error\",\"code\":\"vela_upstream_stream_interrupted\",\"message\":\"The channel stream was interrupted. Check its status in Vela.\"}\n\n")
}

fn upstream_error(status: StatusCode) -> Response<Body> {
    let (code, message) = match status.as_u16() {
        401 => (
            "upstream_authentication_failed",
            "渠道 Key 认证失败，请在 Vela 中更新 Key。",
        ),
        403 => (
            "upstream_access_denied",
            "渠道拒绝访问此模型，请检查 Key 权限。",
        ),
        404 => (
            "upstream_endpoint_or_model_missing",
            "渠道接口或模型不存在，请重新拉取模型并检查 API 路径。",
        ),
        429 => (
            "upstream_rate_limited",
            "渠道限流或余额不足，请稍后重试或选择其他渠道。",
        ),
        300..=399 => (
            "upstream_redirect_blocked",
            "渠道返回了重定向。为保护 Key，Vela 不会跟随跳转，请修改渠道地址。",
        ),
        _ => (
            "upstream_request_failed",
            "渠道未完成请求，请在 Vela 中诊断该渠道。",
        ),
    };
    let public_status = if status.is_redirection() {
        StatusCode::BAD_GATEWAY
    } else {
        status
    };
    error(public_status, code, message)
}

fn error(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    json_response(
        status,
        json!({"error":{"type":"vela_gateway_error","code":code,"message":message}}),
    )
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(value.to_string()))
        .expect("constant valid JSON response headers")
}

#[derive(Default)]
struct Continuations {
    entries: HashMap<String, Continuation>,
    order: VecDeque<(String, Instant)>,
}
struct Continuation {
    route: Option<String>,
    seen: Instant,
}
impl Continuations {
    fn prune(&mut self) {
        let now = Instant::now();
        while self.order.front().is_some_and(|(_, timestamp)| {
            now.duration_since(*timestamp) > CONTINUATION_TTL
                || self.order.len() > MAX_CONTINUATIONS
        }) {
            if let Some((id, timestamp)) = self.order.pop_front() {
                if self
                    .entries
                    .get(&id)
                    .is_some_and(|entry| entry.seen == timestamp)
                {
                    self.entries.remove(&id);
                }
            }
        }
    }
    fn remember(&mut self, id: &str, route: &GatewayRoute) {
        let now = Instant::now();
        let binding = match self.entries.get(id) {
            Some(old) if old.route.as_deref() != Some(route.route_id.as_str()) => None,
            _ => Some(route.route_id.clone()),
        };
        self.entries.insert(
            id.into(),
            Continuation {
                route: binding,
                seen: now,
            },
        );
        self.order.push_back((id.into(), now));
        self.prune();
    }
    fn matches(&mut self, id: &str, route: &GatewayRoute) -> bool {
        self.prune();
        self.entries
            .get(id)
            .is_some_and(|entry| entry.route.as_deref() == Some(route.route_id.as_str()))
    }
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
