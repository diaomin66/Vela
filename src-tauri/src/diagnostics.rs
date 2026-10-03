//! Local configuration inspection and deliberately bounded, user-initiated network probes.
//!
//! Remote response bodies, credentials and raw transport errors never enter the public DTOs.
//! A probe performs at most three inference requests and never retries or follows redirects.

use chrono::Utc;
use futures_util::StreamExt;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use toml_edit::DocumentMut;
use url::{Host, Url};
use uuid::Uuid;
use zeroize::Zeroize;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(35);
const DIAGNOSTIC_TOOL: &str = "connection_diagnostic_echo";
const TOOL_PROMPT: &str = "Call connection_diagnostic_echo once with value set to ok. After receiving its result, reply only OK.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticItem {
    pub id: String,
    pub category: String,
    pub title: String,
    pub status: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub repairable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub id: String,
    pub created_at: String,
    pub items: Vec<DiagnosticItem>,
    pub summary: String,
    pub can_repair: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub responses: bool,
    pub streaming: bool,
    pub tools: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub ok: bool,
    pub checked_at: String,
    pub items: Vec<DiagnosticItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub capabilities: Capabilities,
}

/// The command boundary obtains a credential only after the user starts this probe.
/// Intentionally not Debug/Serialize so accidental logging cannot print a key.
pub struct ValidationInput {
    pub endpoint: String,
    pub model: String,
    pub key: String,
}

impl Drop for ValidationInput {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// File I/O belongs to the native core. This inspection only examines supplied data.
#[derive(Default)]
pub struct LocalDiagnosticInput {
    pub config_path: String,
    pub config_contents: Option<String>,
    pub config_read_error: bool,
    pub expected_provider: Option<String>,
    pub expected_model: Option<String>,
    pub expected_endpoint: Option<String>,
    pub expected_helper_path: Option<String>,
    pub expected_profile_id: Option<String>,
    pub credential_available: Option<bool>,
    pub credential_helper_available: Option<bool>,
}

fn item(
    id: &str,
    category: &str,
    title: &str,
    status: &str,
    description: &str,
    action: Option<&str>,
) -> DiagnosticItem {
    DiagnosticItem {
        id: id.into(),
        category: category.into(),
        title: title.into(),
        status: status.into(),
        description: description.into(),
        action: action.map(str::to_owned),
        repairable: false,
    }
}

/// Preserve all user-supplied path segments; never silently insert or remove `/v1`.
pub fn normalize_endpoint(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) || value.contains('\\') {
        return Err("请输入完整的 API Base URL，且不要包含控制字符或反斜线。".into());
    }
    let mut url = Url::parse(value)
        .map_err(|_| "API 地址无法解析，请填写包含 https:// 的完整 Base URL。".to_owned())?;
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
        .unwrap_or("");
    if !url.username().is_empty() || url.password().is_some() || authority.contains('@') {
        return Err("API 地址不能包含用户名、密码或 Key；请将 Key 填入单独的密钥输入框。".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(
            "API Base URL 不能包含查询参数或 # 片段，请将 Key 填入单独的密钥输入框。".into(),
        );
    }
    let loopback = match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if url.host().is_none() || !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err("远程 API 地址必须使用 HTTPS；HTTP 仅支持本机 localhost 或回环地址。".into());
    }
    let path = url.path().trim_end_matches('/').to_owned();
    if path.ends_with("/responses") || path.ends_with("/chat/completions") {
        return Err("请填写服务商的 Base URL（例如 https://api.example.com/v1），不要附带 /responses 或 /chat/completions。".into());
    }
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn responses_url(endpoint: &str) -> Result<Url, String> {
    let normalized = normalize_endpoint(endpoint)?;
    Url::parse(&format!("{normalized}/responses"))
        .map_err(|_| "无法构造 Responses 请求地址。".into())
}

pub fn inspect_configuration(input: LocalDiagnosticInput) -> DiagnosticReport {
    let mut items = Vec::new();
    // The path is intentionally not copied to the shareable report (it may contain a username).
    let path_known = !input.config_path.is_empty();
    if !path_known {
        items.push(item(
            "config-location",
            "configuration",
            "配置目录未确定",
            "error",
            "无法确定当前用户的 Codex 配置目录。",
            Some("检查 CODEX_HOME 是否指向有效目录，然后重新扫描。"),
        ));
    }
    if input.config_read_error {
        items.push(item(
            "config-read",
            "configuration",
            "配置文件无法读取",
            "error",
            "配置文件读取失败，可能没有权限、被占用或文件编码无效。",
            Some("检查目录权限与文件状态；保留原文件后再尝试恢复。"),
        ));
    } else if let Some(contents) = input.config_contents.as_deref() {
        match contents.parse::<DocumentMut>() {
            Ok(document) => {
                items.push(item(
                    "config-syntax",
                    "configuration",
                    "配置语法完整",
                    "passed",
                    "配置文件可以正常解析为 TOML，未发现语法损坏。",
                    None,
                ));
                inspect_document(&document, &input, &mut items);
            }
            Err(error) => {
                let line = error.span().map(|span| {
                    contents.as_bytes()[..span.start.min(contents.len())]
                        .iter()
                        .filter(|byte| **byte == b'\n')
                        .count()
                        + 1
                });
                let description = line.map(|line| format!("第 {line} 行附近存在 TOML 语法问题。原始内容可能包含敏感信息，因此不会加入诊断报告。"))
                    .unwrap_or_else(|| "配置文件无法解析为 TOML。原始内容可能包含敏感信息，因此不会加入诊断报告。".into());
                items.push(item("config-syntax", "configuration", "配置语法损坏", "error", &description, Some("在恢复页面选择已知可用的备份；没有备份时，先保留原文件，再手动处理语法问题。")));
            }
        }
    } else {
        items.push(item(
            "config-missing",
            "configuration",
            "尚未创建用户配置",
            "info",
            "当前目录中没有配置文件。首次使用时属于正常情况，应用连接后会创建所需配置。",
            Some("添加连接并预览配置变更。"),
        ));
    }
    if let Some(available) = input.credential_available {
        items.push(if available {
            item(
                "credential",
                "authentication",
                "连接凭据已保存",
                "passed",
                "系统凭据库中存在此连接的密钥。可用性需要用户主动发起网络验证。",
                None,
            )
        } else {
            item(
                "credential",
                "authentication",
                "连接密钥缺失",
                "error",
                "系统凭据库中没有找到此连接的密钥，无法进行认证。",
                Some("编辑连接并重新保存 Key。"),
            )
        });
    }
    if let Some(available) = input.credential_helper_available {
        items.push(if available {
            item(
                "credential-helper",
                "authentication",
                "凭据读取程序可用",
                "passed",
                "已找到独立凭据读取程序；应用连接后 Codex 可以按需获取密钥。",
                None,
            )
        } else {
            item(
                "credential-helper",
                "authentication",
                "凭据读取程序缺失",
                "error",
                "未找到所需的独立凭据读取程序，应用连接后无法正常取得密钥。",
                Some("重新安装包含完整组件的工具安装包，再应用连接。"),
            )
        });
    }
    items.push(item("inspection-scope", "compatibility", "本次为本地检查", "info", "未发起网络请求。项目目录、启动参数或客户端版本仍可能影响实际生效结果；重新打开 Codex 后完成一次真实任务可进一步验证。", None));
    // These are repair candidates only. The core must resolve a concrete reapply/restore
    // preview before enabling canRepair; missing credentials never become a repair action.
    for diagnostic in &mut items {
        diagnostic.repairable = matches!(diagnostic.status.as_str(), "error" | "warning")
            && matches!(
                diagnostic.id.as_str(),
                "config-syntax"
                    | "profile-reference"
                    | "provider-type"
                    | "provider-reference"
                    | "provider-url"
                    | "wire-api"
                    | "active-provider"
                    | "active-model"
                    | "active-endpoint"
                    | "model-missing"
                    | "model-type"
                    | "official-auth"
                    | "helper-config"
                    | "conflicting-auth"
                    | "plaintext-key"
            );
    }
    let errors = items.iter().filter(|item| item.status == "error").count();
    let warnings = items.iter().filter(|item| item.status == "warning").count();
    let summary = if errors > 0 {
        format!("发现 {errors} 项需要处理的问题；请查看对应建议。")
    } else if warnings > 0 {
        format!("本地配置可以读取，有 {warnings} 项建议确认。")
    } else {
        "本地检查完成，未发现确定的配置错误。".into()
    };
    DiagnosticReport {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        items,
        summary,
        can_repair: false,
    }
}

fn inspect_document(
    document: &DocumentMut,
    input: &LocalDiagnosticInput,
    items: &mut Vec<DiagnosticItem>,
) {
    if document
        .get("profile")
        .is_some_and(|value| value.as_str().is_none())
    {
        items.push(item(
            "profile-type",
            "configuration",
            "默认配置方案格式错误",
            "error",
            "profile 字段必须是配置方案名称字符串。",
            Some("恢复可用备份，或手动将 profile 字段改为有效的方案名称。"),
        ));
    }
    let profile_name = document.get("profile").and_then(|value| value.as_str());
    let profile = profile_name.and_then(|name| {
        document
            .get("profiles")
            .and_then(|profiles| profiles.get(name))
    });
    if profile_name.is_some() && profile.is_none() {
        items.push(item(
            "profile-reference",
            "configuration",
            "默认配置方案不存在",
            "error",
            "profile 指向了未定义的 profiles 条目。",
            Some("在预览中重新应用所需连接，或恢复已知可用的配置。"),
        ));
    }
    let configured_provider = profile
        .and_then(|value| value.get("model_provider"))
        .or_else(|| document.get("model_provider"));
    if configured_provider.is_some_and(|value| value.as_str().is_none()) {
        items.push(item(
            "provider-type",
            "configuration",
            "服务商字段类型错误",
            "error",
            "model_provider 必须是服务商名称字符串。",
            Some("预览并重新应用保存的连接，或恢复可用备份。"),
        ));
        return;
    }
    let provider_id = configured_provider
        .and_then(|value| value.as_str())
        .unwrap_or("openai");
    let built_in_provider = matches!(
        provider_id,
        "openai" | "ollama" | "lmstudio" | "amazon-bedrock"
    );
    if input
        .expected_provider
        .as_deref()
        .is_some_and(|expected| expected != provider_id)
    {
        items.push(item(
            "active-provider",
            "configuration",
            "生效服务商与保存记录不同",
            "warning",
            "配置文件当前选择的服务商与工具记录不同，可能已被其他程序或默认配置方案修改。",
            Some("确认要使用的连接，预览差异后重新应用；同时检查默认 profile。"),
        ));
    }
    let model_item = profile
        .and_then(|value| value.get("model"))
        .or_else(|| document.get("model"));
    if model_item.is_some_and(|value| value.as_str().is_none()) {
        items.push(item(
            "model-type",
            "configuration",
            "模型字段类型错误",
            "error",
            "model 字段必须是模型名称字符串。",
            Some("预览并重新应用保存的连接，或恢复可用备份。"),
        ));
    }
    let model = model_item.and_then(|value| value.as_str());
    if !built_in_provider && model.is_none_or(|value| value.trim().is_empty()) {
        items.push(item(
            "model-missing",
            "configuration",
            "第三方模型未指定",
            "warning",
            "当前服务商没有明确的模型名称，客户端默认模型可能不在服务商的可用范围内。",
            Some("编辑连接并填写服务商支持的确切模型名称。"),
        ));
    } else if input
        .expected_model
        .as_deref()
        .is_some_and(|expected| Some(expected) != model)
    {
        items.push(item(
            "active-model",
            "configuration",
            "模型与保存记录不同",
            "warning",
            "配置中的模型与工具记录不同。",
            Some("确认模型后重新应用连接；检查默认 profile 是否覆盖模型。"),
        ));
    }
    if built_in_provider {
        items.push(item(
            "provider-reference",
            "configuration",
            "当前使用内置服务商",
            "info",
            "当前服务商由 Codex 内置提供，不要求自定义服务商表。此处不读取或更改该服务商的登录凭据，也未验证其特有的认证设置。",
            None,
        ));
        return;
    }
    let provider = document
        .get("model_providers")
        .and_then(|providers| providers.get(provider_id));
    let Some(provider) = provider else {
        items.push(item(
            "provider-reference",
            "configuration",
            "服务商配置缺失",
            "error",
            "model_provider 引用的服务商没有对应的 model_providers 配置。",
            Some("预览并重新应用保存的连接，或恢复已知可用的配置。"),
        ));
        return;
    };
    items.push(item(
        "provider-reference",
        "configuration",
        "服务商引用正确",
        "passed",
        "已找到当前选择的自定义服务商配置。",
        None,
    ));
    match provider.get("base_url").and_then(|value| value.as_str()) {
        Some(value) => match normalize_endpoint(value) {
            Ok(_) => items.push(item(
                "provider-url",
                "configuration",
                "API 地址格式正确",
                "passed",
                "地址可以解析，且没有在地址中嵌入凭据或查询参数。尚未检查服务是否可达。",
                None,
            )),
            Err(description) => items.push(item(
                "provider-url",
                "configuration",
                "API 地址需要调整",
                "error",
                &description,
                Some("编辑连接中的 API Base URL，再预览应用。"),
            )),
        },
        None => items.push(item(
            "provider-url",
            "configuration",
            "服务商地址缺失",
            "error",
            "自定义服务商没有有效的 base_url 字符串。",
            Some("编辑连接中的 API Base URL，再预览应用。"),
        )),
    }
    if let Some(expected) = input.expected_endpoint.as_deref() {
        let configured = provider
            .get("base_url")
            .and_then(|value| value.as_str())
            .and_then(|value| normalize_endpoint(value).ok());
        if configured != normalize_endpoint(expected).ok() {
            items.push(item(
                "active-endpoint",
                "configuration",
                "API 地址与保存记录不同",
                "warning",
                "当前配置的 API 地址与工具保存的连接不同，可能已被其他程序修改。",
                Some("确认要使用的地址，预览差异后重新应用连接。"),
            ));
        }
    }
    if let (Some(expected_helper), Some(expected_id)) = (
        input.expected_helper_path.as_deref(),
        input.expected_profile_id.as_deref(),
    ) {
        let auth = provider.get("auth");
        let actual_command = auth
            .and_then(|value| value.get("command"))
            .and_then(|value| value.as_str());
        let command_matches = actual_command.is_some_and(|command| {
            if cfg!(windows) {
                command.eq_ignore_ascii_case(expected_helper)
            } else {
                command == expected_helper
            }
        });
        let args_match = auth
            .and_then(|value| value.get("args"))
            .and_then(|value| value.as_array())
            .is_some_and(|args| {
                args.len() == 2
                    && args.get(0).and_then(|value| value.as_str()) == Some("--credential")
                    && args.get(1).and_then(|value| value.as_str()) == Some(expected_id)
            });
        if !command_matches || !args_match {
            items.push(item(
                "helper-config",
                "authentication",
                "凭据读取配置需要修复",
                "error",
                "当前服务商的凭据读取命令或连接标识与已安装组件不一致。",
                Some("预览并重新应用保存的连接，更新凭据读取程序与参数。"),
            ));
        } else {
            items.push(item(
                "helper-config",
                "authentication",
                "凭据读取配置匹配",
                "passed",
                "当前服务商指向正确的独立凭据读取程序和连接标识。",
                None,
            ));
        }
        let conflicting_header = ["http_headers", "env_http_headers"].iter().any(|field| {
            provider
                .get(field)
                .and_then(|value| value.as_table_like())
                .is_some_and(|headers| {
                    headers
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                })
        });
        if provider.get("env_key").is_some() || conflicting_header {
            items.push(item("conflicting-auth", "authentication", "存在额外认证配置", "warning", "当前服务商同时包含环境变量密钥或 Authorization 请求头设置，可能与凭据读取程序产生冲突。", Some("预览并重新应用连接，移除冲突的认证字段。")));
        }
    }
    if provider.get("wire_api").and_then(|value| value.as_str()) != Some("responses") {
        items.push(item(
            "wire-api",
            "compatibility",
            "协议配置需要确认",
            "warning",
            "自定义服务商未明确设置 wire_api = responses。此工具只验证原生 Responses 协议。",
            Some("预览并重新应用保存的连接，将协议明确设为 Responses。"),
        ));
    } else {
        items.push(item(
            "wire-api",
            "compatibility",
            "已选择 Responses 协议",
            "passed",
            "配置指定原生 Responses 协议；服务端支持程度仍需主动验证。",
            None,
        ));
    }
    if provider
        .get("requires_openai_auth")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        items.push(item(
            "official-auth",
            "authentication",
            "第三方连接要求官方认证",
            "warning",
            "当前自定义服务商开启了 requires_openai_auth，可能与第三方 Key 的认证方式冲突。",
            Some("确认服务商要求，或预览并重新应用此工具保存的连接。"),
        ));
    }
    if ["experimental_bearer_token", "api_key"]
        .iter()
        .any(|key| provider.get(key).is_some())
    {
        items.push(item(
            "plaintext-key",
            "authentication",
            "配置中可能存在明文凭据",
            "warning",
            "服务商配置包含可用于存放明文密钥的字段。报告不会复制该字段内容。",
            Some(
                "将连接重新保存到系统凭据库，再检查并移除旧的明文凭据；历史备份也可能包含旧内容。",
            ),
        ));
    }
}

#[derive(Debug)]
enum ProbeFailure {
    Cancelled,
    Timeout,
    Dns,
    Tls,
    Proxy,
    Connect,
    Transport,
    Http(u16),
    InvalidJson,
    Oversized,
    InvalidResponse,
    Incomplete,
    RemoteFailure,
    InvalidStream,
}

impl ProbeFailure {
    fn diagnostic(&self, stage: &str) -> DiagnosticItem {
        let (id, title, description, action) = match self {
            Self::Cancelled => ("cancelled", "验证已取消", "已停止后续请求；服务商可能仍对已经处理的请求计费。", "准备好后可重新主动发起验证。"),
            Self::Timeout => ("timeout", "连接或响应超时", "服务商未在 35 秒内完成本次请求，没有自动重试。", "检查服务商状态、代理设置和网络连接，稍后手动重试。"),
            Self::Dns => ("dns", "域名解析失败", "系统无法解析 API 服务商的域名。", "检查 Base URL 拼写、DNS 和代理设置。"),
            Self::Tls => ("tls", "安全连接未建立", "TLS 握手或证书校验失败，未绕过证书检查。", "检查系统时间、受信任证书与代理，或联系服务商处理证书。"),
            Self::Proxy => ("proxy", "代理连接失败", "请求无法通过当前代理到达服务商。", "检查系统代理及 HTTPS_PROXY / ALL_PROXY 等代理环境变量。"),
            Self::Connect => ("connect", "无法连接服务商", "连接建立失败，可能是端口不可达、代理异常或服务暂时不可用。", "确认地址与端口，检查网络和代理设置。"),
            Self::Transport => ("transport", "响应传输中断", "网络响应未能完整接收，没有自动重试。", "检查网络与服务商状态后手动重试。"),
            Self::Http(401) => ("unauthorized", "Key 未通过认证", "服务商返回 401，当前密钥无效、已失效或认证方式不匹配。", "在服务商控制台确认 Key，再编辑连接重新保存。"),
            Self::Http(403) => ("forbidden", "访问被服务商拒绝", "服务商返回 403，当前密钥可能没有模型、地区或网络访问权限。", "检查服务商授权、IP 限制与目标模型权限。"),
            Self::Http(404) => ("not-found", "接口或模型不存在", "服务商返回 404，Base URL 路径或模型名称可能不正确，也可能没有提供 Responses 接口。", "核对服务商提供的 Base URL 与模型名称，确认支持 /responses。"),
            Self::Http(429) => ("rate-limit", "额度不足或请求受限", "服务商返回 429，可能是余额不足、额度耗尽或速率限制。没有自动重试。", "检查服务商额度与限流策略，稍后主动重试。"),
            Self::Http(400 | 422) => ("request-rejected", "请求参数不被支持", "服务商拒绝此阶段的原生 Responses 请求，模型名称或所需协议能力可能不受支持。", "核对模型名称，并向服务商确认流式 Responses 与函数调用支持。"),
            Self::Http(405) => ("method", "接口不接受 Responses 请求", "此地址不接受 POST 请求，可能填写了控制台页面或错误的 API 路径。", "填写 API Base URL，而不是服务商网站或控制台地址。"),
            Self::Http(407) => ("proxy-auth", "代理要求认证", "代理服务器返回 407，尚未通过代理认证，请求未能正常到达服务商。", "检查系统或环境变量中的代理地址与代理认证信息。"),
            Self::Http(300..=399) => ("redirect", "服务商要求跳转地址", "请求遇到 HTTP 重定向。为避免把 Key 发送到另一地址，验证没有跟随跳转。", "从服务商确认最终 API Base URL 后再填写。"),
            Self::Http(500..=599) => ("server", "服务商暂时异常", "服务商或其上游返回服务器错误，没有自动重试。", "检查服务商状态，稍后手动重试。"),
            Self::Http(_) => ("http", "服务商返回异常状态", "请求未得到成功响应。报告不会复制可能含有凭据的服务端错误正文。", "核对服务商配置，必要时联系服务商支持。"),
            Self::InvalidJson => ("json", "响应不是有效的协议数据", "服务商返回的数据无法解析为 JSON，可能是网页、代理错误页或不兼容的接口。", "核对 API Base URL，并确认服务商支持原生 Responses。"),
            Self::Oversized => ("size-limit", "诊断响应超过安全上限", "本次小型测试的返回数据超过 1 MiB，已停止接收。", "检查服务商是否返回了错误页面或异常的大响应。"),
            Self::InvalidResponse => ("response-shape", "Responses 数据结构不完整", "返回值缺少完整的 Responses 对象或预期输出，不能确认基础兼容。", "向服务商确认返回的是 Responses 协议，而不是 Chat Completions。"),
            Self::Incomplete => ("incomplete", "小型测试未完成", "服务商标记响应未完成；有限的测试输出预算可能不足，暂不能确认兼容性。没有扩大预算或自动重试。", "结合服务商文档检查目标模型需求；此结果不代表模型一定不可用。"),
            Self::RemoteFailure => ("remote-failure", "服务商中止生成", "Responses 流返回失败事件。为保护凭据，未复制服务端原始错误。", "检查模型权限、额度和服务商状态后主动重试。"),
            Self::InvalidStream => ("stream", "流式事件不完整", "未收到相互对应的 response.created、增量输出与 response.completed 事件，或响应被提前截断。", "向服务商确认原生 Responses SSE 流式协议支持情况。"),
        };
        item(
            &format!("{stage}-{id}"),
            "connection",
            title,
            if matches!(self, Self::Cancelled) {
                "info"
            } else {
                "error"
            },
            description,
            Some(action),
        )
    }
}

fn transport_failure(error: reqwest::Error) -> ProbeFailure {
    if error.is_timeout() {
        return ProbeFailure::Timeout;
    }
    // Classification only: never expose this untrusted chain in a report or log.
    let mut text = error.to_string().to_lowercase();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        text.push_str(&cause.to_string().to_lowercase());
        source = cause.source();
    }
    if text.contains("dns") || text.contains("resolve") || text.contains("name or service") {
        ProbeFailure::Dns
    } else if text.contains("certificate") || text.contains("tls") || text.contains("ssl") {
        ProbeFailure::Tls
    } else if text.contains("proxy") || text.contains("tunnel") {
        ProbeFailure::Proxy
    } else if error.is_connect() {
        ProbeFailure::Connect
    } else {
        ProbeFailure::Transport
    }
}

#[derive(Default)]
struct StreamEvidence {
    created_id: Option<String>,
    text_delta: bool,
    function_delta: bool,
    function_done: bool,
    response: Option<Value>,
}

impl StreamEvidence {
    fn lifecycle_complete(&self) -> bool {
        self.response
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .zip(self.created_id.as_deref())
            .is_some_and(|(finished, started)| finished == started)
    }

    fn on_event(&mut self, event_name: &str, data: &str) -> Result<(), ProbeFailure> {
        if data == "[DONE]" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(data).map_err(|_| ProbeFailure::InvalidJson)?;
        let event = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_name);
        match event {
            "response.created" => {
                self.created_id = value
                    .pointer("/response/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            "response.output_text.delta" => {
                self.text_delta |= value
                    .get("delta")
                    .and_then(Value::as_str)
                    .is_some_and(|delta| !delta.is_empty());
            }
            "response.function_call_arguments.delta" => {
                self.function_delta |= value
                    .get("delta")
                    .and_then(Value::as_str)
                    .is_some_and(|delta| !delta.is_empty());
            }
            "response.function_call_arguments.done" => self.function_done = true,
            "response.output_item.done" => {
                self.function_done |=
                    value.pointer("/item/type").and_then(Value::as_str) == Some("function_call");
            }
            "response.completed" => {
                let response = value.get("response").ok_or(ProbeFailure::InvalidResponse)?;
                check_response(response)?;
                self.response = Some(response.clone());
            }
            "response.incomplete" => return Err(ProbeFailure::Incomplete),
            "response.failed" | "error" => return Err(ProbeFailure::RemoteFailure),
            _ => {}
        }
        Ok(())
    }
}

/// SSE lines are decoded only once a newline arrives, so split UTF-8 and CRLF chunks work.
#[derive(Default)]
struct SseParser {
    pending: Vec<u8>,
    event_name: String,
    data: String,
    evidence: StreamEvidence,
}

impl SseParser {
    fn push(&mut self, chunk: &[u8]) -> Result<(), ProbeFailure> {
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            let line = std::str::from_utf8(&raw[..raw.len() - 1])
                .map_err(|_| ProbeFailure::InvalidStream)?
                .trim_end_matches('\r');
            if line.is_empty() {
                if !self.data.is_empty() {
                    self.evidence
                        .on_event(&self.event_name, self.data.trim_end_matches('\n'))?;
                }
                self.event_name.clear();
                self.data.clear();
            } else if !line.starts_with(':') {
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => self.event_name = value.to_owned(),
                    "data" => {
                        self.data.push_str(value);
                        self.data.push('\n');
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn check_response(value: &Value) -> Result<(), ProbeFailure> {
    match value.get("status").and_then(Value::as_str) {
        Some("incomplete") => return Err(ProbeFailure::Incomplete),
        Some("failed" | "cancelled") => return Err(ProbeFailure::RemoteFailure),
        Some("completed") => {}
        _ => return Err(ProbeFailure::InvalidResponse),
    }
    if value.get("object").and_then(Value::as_str) != Some("response")
        || value
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !value.get("output").is_some_and(Value::is_array)
    {
        return Err(ProbeFailure::InvalidResponse);
    }
    Ok(())
}

fn has_text_output(value: &Value) -> bool {
    value
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|output| {
            output.iter().any(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("message")
                    && entry
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part.get("type").and_then(Value::as_str) == Some("output_text")
                                    && part
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .is_some_and(|text| !text.trim().is_empty())
                            })
                        })
            })
        })
}

struct ProbeResponse {
    response: Value,
    streamed: bool,
    text_streamed: bool,
    function_streamed: bool,
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow() || cancellation.changed().await.is_err() {
            return;
        }
    }
}

async fn execute_request(
    client: &reqwest::Client,
    url: &Url,
    authorization: &HeaderValue,
    payload: Value,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<ProbeResponse, ProbeFailure> {
    let request = async {
        let response = client
            .post(url.clone())
            .header(AUTHORIZATION, authorization.clone())
            .header(ACCEPT, "text/event-stream")
            .json(&payload)
            .send()
            .await
            .map_err(transport_failure)?;
        if !response.status().is_success() {
            // Do not read or expose an error body: providers sometimes echo Authorization.
            return Err(ProbeFailure::Http(response.status().as_u16()));
        }
        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|header| header.to_str().ok())
            .is_some_and(|content_type| {
                content_type
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
            });
        let mut stream = response.bytes_stream();
        let mut parser = SseParser::default();
        let mut bytes = Vec::new();
        let mut received = 0_usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport_failure)?;
            received = received.saturating_add(chunk.len());
            if received > MAX_RESPONSE_BYTES {
                return Err(ProbeFailure::Oversized);
            }
            if is_sse {
                parser.push(&chunk)?;
                if parser.evidence.response.is_some() {
                    break;
                }
            } else {
                bytes.extend_from_slice(&chunk);
            }
        }
        if is_sse {
            let lifecycle_complete = parser.evidence.lifecycle_complete();
            let text_streamed = lifecycle_complete && parser.evidence.text_delta;
            let function_streamed = lifecycle_complete
                && parser.evidence.function_delta
                && parser.evidence.function_done;
            let response = parser
                .evidence
                .response
                .ok_or(ProbeFailure::InvalidStream)?;
            Ok(ProbeResponse {
                response,
                streamed: lifecycle_complete,
                text_streamed,
                function_streamed,
            })
        } else {
            let response: Value =
                serde_json::from_slice(&bytes).map_err(|_| ProbeFailure::InvalidJson)?;
            check_response(&response)?;
            Ok(ProbeResponse {
                response,
                streamed: false,
                text_streamed: false,
                function_streamed: false,
            })
        }
    };
    tokio::select! {
        biased;
        _ = wait_for_cancellation(cancellation) => Err(ProbeFailure::Cancelled),
        result = tokio::time::timeout(REQUEST_TIMEOUT, request) => result.unwrap_or(Err(ProbeFailure::Timeout)),
    }
}

fn tool_definition() -> Value {
    json!({"type":"function","name":DIAGNOSTIC_TOOL,"description":"A harmless connection test that echoes a small fixed value. It does not access files, run commands or use the network.","parameters":{"type":"object","properties":{"value":{"type":"string","enum":["ok"]}},"required":["value"],"additionalProperties":false},"strict":true})
}

fn tool_followup(response: &Value) -> Result<Value, ProbeFailure> {
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::InvalidResponse)?;
    let calls: Vec<&Value> = output
        .iter()
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("function_call"))
        .collect();
    if calls.len() != 1 {
        return Err(ProbeFailure::InvalidResponse);
    }
    let call = calls[0];
    if call.get("name").and_then(Value::as_str) != Some(DIAGNOSTIC_TOOL) {
        return Err(ProbeFailure::InvalidResponse);
    }
    let call_id = call
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(ProbeFailure::InvalidResponse)?;
    let arguments: Value = serde_json::from_str(
        call.get("arguments")
            .and_then(Value::as_str)
            .ok_or(ProbeFailure::InvalidResponse)?,
    )
    .map_err(|_| ProbeFailure::InvalidResponse)?;
    if arguments.get("value").and_then(Value::as_str) != Some("ok") {
        return Err(ProbeFailure::InvalidResponse);
    }
    // Preserve all output items, including reasoning items needed for a stateless continuation.
    let mut input = vec![json!({"role":"user","content":TOOL_PROMPT})];
    input.extend(output.iter().cloned());
    input.push(json!({"type":"function_call_output","call_id":call_id,"output":"ok"}));
    Ok(Value::Array(input))
}

pub async fn validate_connection(
    input: ValidationInput,
    mut cancellation: watch::Receiver<bool>,
) -> ValidationResult {
    let started = Instant::now();
    let mut result = ValidationResult {
        ok: false,
        checked_at: Utc::now().to_rfc3339(),
        items: Vec::new(),
        latency_ms: None,
        capabilities: Capabilities::default(),
    };
    let url = match responses_url(&input.endpoint) {
        Ok(url) => url,
        Err(description) => {
            result.items.push(item(
                "endpoint",
                "configuration",
                "API 地址需要调整",
                "error",
                &description,
                Some("编辑连接中的 API Base URL。"),
            ));
            return result;
        }
    };
    if input.model.trim().is_empty() || input.model.chars().any(char::is_control) {
        result.items.push(item(
            "model",
            "configuration",
            "模型名称无效",
            "error",
            "请填写服务商支持的确切模型名称，名称不能包含控制字符。",
            Some("编辑连接中的模型名称。"),
        ));
        return result;
    }
    if input.key.trim().is_empty() {
        result.items.push(item(
            "key",
            "authentication",
            "连接密钥缺失",
            "error",
            "此连接尚未保存 Key。",
            Some("编辑连接并保存 Key。"),
        ));
        return result;
    }
    let mut header_text = format!("Bearer {}", input.key);
    let authorization = HeaderValue::from_str(&header_text);
    header_text.zeroize();
    let mut authorization = match authorization {
        Ok(header) => header,
        Err(_) => {
            result.items.push(item(
                "key-format",
                "authentication",
                "Key 格式无效",
                "error",
                "密钥包含不适合 HTTP 认证头的字符。",
                Some("重新复制 Key，去除换行或多余字符。"),
            ));
            return result;
        }
    };
    authorization.set_sensitive(true);
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(10))
        .timeout(REQUEST_TIMEOUT)
        .user_agent("Vela/0.1 connection-check");
    let local = match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    // A configured system proxy must not turn a local-only probe into a remote request.
    if local {
        builder = builder.no_proxy();
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => {
            result.items.push(item(
                "network-client",
                "connection",
                "无法初始化网络连接",
                "error",
                "网络客户端创建失败，可能与代理或 TLS 配置有关。",
                Some("检查系统网络与代理设置后重试。"),
            ));
            return result;
        }
    };
    result.items.push(item(
        "endpoint",
        "configuration",
        "地址与请求格式已检查",
        "passed",
        "已保留填写的 API 路径，并使用 /responses 验证。不会跟随重定向或自动重试。",
        None,
    ));
    let basic = json!({"model":input.model,"input":"Reply only OK.","stream":true,"store":false,"max_output_tokens":64});
    let basic_response =
        match execute_request(&client, &url, &authorization, basic, &mut cancellation).await {
            Ok(response) if has_text_output(&response.response) => response,
            Ok(_) => {
                result
                    .items
                    .push(ProbeFailure::InvalidResponse.diagnostic("responses"));
                return finish(result, started);
            }
            Err(error) => {
                result.items.push(error.diagnostic("responses"));
                return finish(result, started);
            }
        };
    result.capabilities.responses = true;
    result.items.push(item(
        "responses",
        "compatibility",
        "基础 Responses 请求通过",
        "passed",
        "目标模型已返回完整的 Responses 文本结果。未依赖模型列表接口。",
        None,
    ));
    if !basic_response.text_streamed {
        result
            .items
            .push(ProbeFailure::InvalidStream.diagnostic("streaming"));
        return finish(result, started);
    }
    result.capabilities.streaming = true;
    result.items.push(item(
        "streaming",
        "compatibility",
        "流式输出通过",
        "passed",
        "已收到对应的创建、文本增量与完成事件。",
        None,
    ));
    let tools_payload = json!({"model":input.model,"input":[{"role":"user","content":TOOL_PROMPT}],"stream":true,"store":false,"max_output_tokens":128,"include":["reasoning.encrypted_content"],"tools":[tool_definition()],"tool_choice":{"type":"function","name":DIAGNOSTIC_TOOL},"parallel_tool_calls":false});
    let tool_response = match execute_request(
        &client,
        &url,
        &authorization,
        tools_payload,
        &mut cancellation,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            result.items.push(error.diagnostic("tool-call"));
            return finish(result, started);
        }
    };
    if !tool_response.streamed || !tool_response.function_streamed {
        result
            .items
            .push(ProbeFailure::InvalidStream.diagnostic("tool-call"));
        return finish(result, started);
    }
    let followup = match tool_followup(&tool_response.response) {
        Ok(input) => input,
        Err(error) => {
            result.items.push(error.diagnostic("tool-call"));
            return finish(result, started);
        }
    };
    let followup_payload = json!({"model":input.model,"input":followup,"stream":true,"store":false,"max_output_tokens":64,"tools":[tool_definition()],"tool_choice":"none"});
    match execute_request(
        &client,
        &url,
        &authorization,
        followup_payload,
        &mut cancellation,
    )
    .await
    {
        Ok(response) if response.text_streamed && has_text_output(&response.response) => {
            result.capabilities.tools = true;
            result.ok = true;
            result.items.push(item("tools", "compatibility", "函数调用与结果回传通过", "passed", "模型完成了无副作用的回显函数调用，并接收结果后继续输出。没有执行文件、终端或外部操作。", None));
            result.items.push(item("compatibility-scope", "compatibility", "已验证基础任务能力", "info", "此检查覆盖文本、SSE 与单次函数调用往返，不代表所有 Codex 功能或任意长任务都兼容。请在 Codex 中完成一次实际任务。", None));
        }
        Ok(_) => result
            .items
            .push(ProbeFailure::InvalidResponse.diagnostic("tool-result")),
        Err(error) => result.items.push(error.diagnostic("tool-result")),
    }
    finish(result, started)
}

fn finish(mut result: ValidationResult, started: Instant) -> ValidationResult {
    result.latency_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::thread;

    fn response(output: Value) -> Value {
        json!({"id":"resp_test","object":"response","status":"completed","output":output})
    }

    fn message() -> Value {
        json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"OK"}]})
    }

    fn event(kind: &str, fields: Value) -> String {
        let mut value = fields;
        value["type"] = json!(kind);
        format!("event: {kind}\r\ndata: {value}\r\n\r\n")
    }

    fn text_stream() -> String {
        event("response.created", json!({"response":{"id":"resp_test"}}))
            + &event("response.output_text.delta", json!({"delta":"OK"}))
            + &event(
                "response.completed",
                json!({"response":response(json!([message()]))}),
            )
    }

    #[test]
    fn endpoint_preserves_custom_path_without_inventing_version() {
        assert_eq!(
            normalize_endpoint(" https://example.com/team/api/ ").unwrap(),
            "https://example.com/team/api"
        );
        assert_eq!(
            responses_url("https://example.com/team/api/")
                .unwrap()
                .as_str(),
            "https://example.com/team/api/responses"
        );
        assert_eq!(
            responses_url("https://example.com").unwrap().as_str(),
            "https://example.com/responses"
        );
        assert_eq!(
            normalize_endpoint("https://example.com/a%20b/v1").unwrap(),
            "https://example.com/a%20b/v1"
        );
    }

    #[test]
    fn endpoint_rejects_credential_leaks_and_remote_plaintext() {
        for endpoint in [
            "http://example.com/v1",
            "https://user:secret@example.com/v1",
            "https://@example.com/v1",
            "https://example.com/v1?api_key=secret",
            "https://example.com/v1#secret",
            "https://example.com/v1/responses",
            "https://example.com/v1/chat/completions",
            "file:///secret",
            "https://example.com/v1\nheader",
            "https:\\example.com",
        ] {
            assert!(
                normalize_endpoint(endpoint).is_err(),
                "unexpectedly accepted {endpoint}"
            );
        }
        for endpoint in [
            "http://127.0.0.1:9000/v1",
            "http://localhost:9000/api",
            "http://[::1]:9000/api",
        ] {
            assert!(normalize_endpoint(endpoint).is_ok());
        }
    }

    #[test]
    fn split_sse_frames_and_utf8_are_decoded() {
        let stream = text_stream().replace("OK", "连接正常");
        let mut parser = SseParser::default();
        for byte in stream.as_bytes() {
            parser.push(&[*byte]).unwrap();
        }
        assert!(parser.evidence.lifecycle_complete());
        assert!(parser.evidence.text_delta);
        assert!(has_text_output(parser.evidence.response.as_ref().unwrap()));
    }

    #[test]
    fn incomplete_and_failed_streams_are_not_success() {
        let mut parser = SseParser::default();
        assert!(matches!(
            parser.push(
                event(
                    "response.incomplete",
                    json!({"response":{"status":"incomplete"}})
                )
                .as_bytes()
            ),
            Err(ProbeFailure::Incomplete)
        ));
        let mut parser = SseParser::default();
        assert!(matches!(
            parser.push(event("error", json!({"message":"SECRET_KEY"})).as_bytes()),
            Err(ProbeFailure::RemoteFailure)
        ));
        let serialized =
            serde_json::to_string(&ProbeFailure::RemoteFailure.diagnostic("responses")).unwrap();
        assert!(!serialized.contains("SECRET_KEY"));
    }

    #[test]
    fn tool_followup_preserves_reasoning_and_call_id() {
        let output = json!([
            {"type":"reasoning","id":"rs_test","summary":[],"encrypted_content":"opaque"},
            {"type":"function_call","id":"fc_test","call_id":"call_test","name":DIAGNOSTIC_TOOL,"arguments":"{\"value\":\"ok\"}"}
        ]);
        let followup = tool_followup(&response(output)).unwrap();
        assert_eq!(followup[1]["type"], "reasoning");
        assert_eq!(followup[1]["encrypted_content"], "opaque");
        assert_eq!(followup[3]["call_id"], "call_test");
        assert_eq!(followup[3]["output"], "ok");
    }

    #[test]
    fn local_parser_does_not_leak_broken_config_contents() {
        let report = inspect_configuration(LocalDiagnosticInput {
            config_path: "C:\\Users\\private-user\\config.toml".into(),
            config_contents: Some("experimental_bearer_token = SECRET_KEY [".into()),
            ..Default::default()
        });
        let exported = serde_json::to_string(&report).unwrap();
        assert!(!exported.contains("SECRET_KEY"));
        assert!(!exported.contains("private-user"));
        assert!(report
            .items
            .iter()
            .any(|item| item.id == "config-syntax" && item.status == "error"));
        assert!(!report.can_repair);
    }

    #[test]
    fn local_inspection_distinguishes_missing_file_and_missing_provider() {
        let missing = inspect_configuration(LocalDiagnosticInput {
            config_path: "config.toml".into(),
            ..Default::default()
        });
        assert!(missing
            .items
            .iter()
            .any(|item| item.id == "config-missing" && item.status == "info"));
        let broken = inspect_configuration(LocalDiagnosticInput {
            config_path: "config.toml".into(),
            config_contents: Some("model_provider = \"missing\"\nmodel = \"test\"".into()),
            ..Default::default()
        });
        assert!(broken
            .items
            .iter()
            .any(|item| item.id == "provider-reference" && item.status == "error"));
    }

    #[test]
    fn built_in_providers_do_not_require_a_custom_provider_table() {
        for provider in ["openai", "ollama", "lmstudio", "amazon-bedrock"] {
            let report = inspect_configuration(LocalDiagnosticInput {
                config_path: "config.toml".into(),
                config_contents: Some(format!("model_provider = \"{provider}\"\n")),
                ..Default::default()
            });
            assert!(!report.items.iter().any(|item| item.status == "error"));
            assert!(!report.items.iter().any(|item| item.repairable));
        }
    }

    #[test]
    fn default_profile_overrides_are_reported() {
        let report = inspect_configuration(LocalDiagnosticInput { config_path: "config.toml".into(), config_contents: Some("model_provider = \"saved\"\nprofile = \"work\"\n[profiles.work]\nmodel_provider = \"other\"\nmodel = \"other-model\"".into()), expected_provider: Some("saved".into()), expected_model: Some("saved-model".into()), ..Default::default() });
        assert!(report
            .items
            .iter()
            .any(|item| item.id == "active-provider" && item.status == "warning"));
        assert!(report
            .items
            .iter()
            .any(|item| item.id == "active-model" && item.status == "warning"));
    }

    #[test]
    fn http_failures_have_actionable_safe_categories() {
        for (status, expected) in [
            (401, "unauthorized"),
            (403, "forbidden"),
            (404, "not-found"),
            (429, "rate-limit"),
            (302, "redirect"),
            (502, "server"),
        ] {
            let result = ProbeFailure::Http(status).diagnostic("test");
            assert_eq!(result.id, format!("test-{expected}"));
            assert_eq!(result.status, "error");
            assert!(result.action.is_some());
        }
    }

    #[tokio::test]
    async fn cancelled_probe_sends_no_requests() {
        let (_sender, receiver) = watch::channel(true);
        let result = validate_connection(
            ValidationInput {
                endpoint: "http://127.0.0.1:1/v1".into(),
                model: "test".into(),
                key: "fake-unit-test-key".into(),
            },
            receiver,
        )
        .await;
        assert!(!result.ok);
        assert!(result
            .items
            .iter()
            .any(|item| item.id.ends_with("cancelled")));
    }

    struct MockReply {
        status: u16,
        content_type: &'static str,
        body: String,
        headers: String,
        delay: Duration,
    }

    impl MockReply {
        fn sse(body: String) -> Self {
            Self {
                status: 200,
                content_type: "text/event-stream",
                body,
                headers: String::new(),
                delay: Duration::ZERO,
            }
        }
        fn json(status: u16, body: String) -> Self {
            Self {
                status,
                content_type: "application/json",
                body,
                headers: String::new(),
                delay: Duration::ZERO,
            }
        }
    }

    struct MockServer {
        base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl MockServer {
        fn start(replies: Vec<MockReply>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base_url = format!("http://{}/custom/v1", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let thread_requests = requests.clone();
            let thread_stop = stop.clone();
            let thread = thread::spawn(move || {
                let mut replies = replies.into_iter();
                while !thread_stop.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                            continue;
                        }
                        Err(_) => break,
                    };
                    // On Windows, accept inherits the listener's nonblocking mode.
                    // A read timeout alone does not change that mode: a partial
                    // POST could otherwise hit WouldBlock and be mistaken for EOF.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    let mut request_complete = false;
                    loop {
                        let received = match stream.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(received) => received,
                        };
                        bytes.extend_from_slice(&buffer[..received]);
                        if bytes.len() > 128 * 1024 {
                            break;
                        }
                        if let Some(header_end) =
                            bytes.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header = String::from_utf8_lossy(&bytes[..header_end]);
                            let length = header
                                .lines()
                                .filter_map(|line| line.split_once(':'))
                                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            if bytes.len() >= header_end + 4 + length {
                                request_complete = true;
                                break;
                            }
                        }
                    }
                    if !request_complete {
                        continue;
                    }
                    thread_requests
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&bytes).into_owned());
                    let Some(reply) = replies.next() else {
                        break;
                    };
                    let delayed_until = Instant::now() + reply.delay;
                    while Instant::now() < delayed_until && !thread_stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(5));
                    }
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let response = format!("HTTP/1.1 {} Test\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}", reply.status, reply.content_type, reply.body.len(), reply.headers, reply.body);
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    // Drain the peer's orderly close so Windows does not reset
                    // the connection before the client has consumed our reply.
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                    while matches!(stream.read(&mut buffer), Ok(received) if received > 0) {}
                }
            });
            Self {
                base_url,
                requests,
                stop,
                thread: Some(thread),
            }
        }

        fn count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    fn input_for(server: &MockServer) -> ValidationInput {
        ValidationInput {
            endpoint: server.base_url.clone(),
            model: "manual-model-name".into(),
            key: "fake-unit-test-key".into(),
        }
    }

    #[tokio::test]
    async fn mock_service_validates_three_request_function_roundtrip() {
        let tool_response = response(json!([
            {"type":"reasoning","id":"rs_test","summary":[],"encrypted_content":"opaque"},
            {"type":"function_call","id":"fc_test","call_id":"call_test","name":DIAGNOSTIC_TOOL,"arguments":"{\"value\":\"ok\"}"}
        ]));
        let tool_stream = event("response.created", json!({"response":{"id":"resp_test"}}))
            + &event(
                "response.function_call_arguments.delta",
                json!({"delta":"{\"value\":\"ok\"}","item_id":"fc_test"}),
            )
            + &event(
                "response.function_call_arguments.done",
                json!({"arguments":"{\"value\":\"ok\"}","item_id":"fc_test"}),
            )
            + &event("response.completed", json!({"response":tool_response}));
        let server = MockServer::start(vec![
            MockReply::sse(text_stream()),
            MockReply::sse(tool_stream),
            MockReply::sse(text_stream()),
        ]);
        let (_sender, receiver) = watch::channel(false);
        let result = validate_connection(input_for(&server), receiver).await;
        assert!(result.ok, "{:?}", result.items);
        assert!(
            result.capabilities.responses
                && result.capabilities.streaming
                && result.capabilities.tools
        );
        assert_eq!(server.count(), 3);
        let requests = server.requests.lock().unwrap();
        assert!(requests
            .iter()
            .all(|request| request.starts_with("POST /custom/v1/responses HTTP/1.1")));
        let final_body: Value =
            serde_json::from_str(requests[2].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(final_body["input"][1]["type"], "reasoning");
        assert_eq!(final_body["input"][3]["call_id"], "call_test");
        assert_eq!(final_body["store"], false);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("fake-unit-test-key"));
    }

    #[tokio::test]
    async fn authentication_failure_does_not_retry_or_echo_error_body() {
        let server = MockServer::start(vec![MockReply::json(
            401,
            "{\"error\":\"Bearer fake-unit-test-key\"}".into(),
        )]);
        let (_sender, receiver) = watch::channel(false);
        let result = validate_connection(input_for(&server), receiver).await;
        assert_eq!(server.count(), 1);
        assert!(!result.ok);
        assert!(result
            .items
            .iter()
            .any(|item| item.id == "responses-unauthorized"));
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("fake-unit-test-key"));
    }

    #[tokio::test]
    async fn redirects_never_receive_a_second_authorized_request() {
        let mut redirect = MockReply::json(307, "{}".into());
        redirect.headers = "Location: http://127.0.0.1:9/collect-key\r\n".into();
        let server = MockServer::start(vec![redirect]);
        let (_sender, receiver) = watch::channel(false);
        let result = validate_connection(input_for(&server), receiver).await;
        assert_eq!(server.count(), 1);
        assert!(
            result
                .items
                .iter()
                .any(|item| item.id == "responses-redirect"),
            "{:?}",
            result.items
        );
    }

    #[tokio::test]
    async fn plain_json_cannot_be_reported_as_streaming_compatible() {
        let server = MockServer::start(vec![MockReply::json(
            200,
            response(json!([message()])).to_string(),
        )]);
        let (_sender, receiver) = watch::channel(false);
        let result = validate_connection(input_for(&server), receiver).await;
        assert!(!result.ok);
        assert!(result.capabilities.responses, "{:?}", result.items);
        assert!(!result.capabilities.streaming);
        assert!(!result.capabilities.tools);
        assert_eq!(server.count(), 1);
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_inflight_response() {
        let mut reply = MockReply::sse(text_stream());
        reply.delay = Duration::from_secs(5);
        let server = MockServer::start(vec![reply]);
        let (sender, receiver) = watch::channel(false);
        let request_count = server.requests.clone();
        let cancel = async move {
            for _ in 0..100 {
                if !request_count.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            sender.send(true).unwrap();
        };
        let started = Instant::now();
        let (result, ()) = tokio::join!(validate_connection(input_for(&server), receiver), cancel);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(result
            .items
            .iter()
            .any(|item| item.id.ends_with("cancelled")));
        assert_eq!(server.count(), 1);
    }

    #[tokio::test]
    async fn truncated_stream_never_reports_success() {
        let stream = event("response.created", json!({"response":{"id":"resp_test"}}))
            + &event("response.output_text.delta", json!({"delta":"OK"}));
        let server = MockServer::start(vec![MockReply::sse(stream)]);
        let (_sender, receiver) = watch::channel(false);
        let result = validate_connection(input_for(&server), receiver).await;
        assert!(!result.ok);
        assert!(!result.capabilities.streaming);
        assert!(result
            .items
            .iter()
            .any(|item| item.id == "responses-stream"));
    }
}
