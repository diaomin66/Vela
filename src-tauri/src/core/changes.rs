//! Safe preview summaries and conflict fingerprints shared by apply and restore.
use super::domain::Change;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item};
use zeroize::Zeroizing;

pub(super) fn digest(bytes: Option<&[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(if bytes.is_some() {
        b"exists".as_slice()
    } else {
        b"missing".as_slice()
    });
    if let Some(value) = bytes {
        hasher.update(value);
    }
    format!("{:x}", hasher.finalize())
}
pub(super) fn token(current: Option<&[u8]>, proposed: Option<&[u8]>) -> String {
    digest(Some(
        format!("{}:{}", digest(current), digest(proposed)).as_bytes(),
    ))
}
pub(super) fn raw(bytes: &Option<Zeroizing<Vec<u8>>>) -> Option<&[u8]> {
    bytes.as_ref().map(|v| v.as_slice())
}
fn safe_endpoint(value: &str) -> String {
    if let Ok(mut url) = url::Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        url.to_string()
    } else {
        "未设置或格式无效".into()
    }
}
pub(super) fn describe(current: &str) -> (String, String, String) {
    if let Ok(doc) = current.parse::<DocumentMut>() {
        let selected = doc
            .get("profile")
            .and_then(Item::as_str)
            .and_then(|name| doc.get("profiles").and_then(|profiles| profiles.get(name)));
        let model = selected
            .and_then(|p| p.get("model"))
            .or_else(|| doc.get("model"))
            .and_then(Item::as_str)
            .unwrap_or("默认模型")
            .to_string();
        let provider = selected
            .and_then(|p| p.get("model_provider"))
            .or_else(|| doc.get("model_provider"))
            .and_then(Item::as_str)
            .unwrap_or("openai")
            .to_string();
        let endpoint = doc
            .get("model_providers")
            .and_then(|p| p.get(&provider))
            .and_then(|p| p.get("base_url"))
            .and_then(Item::as_str)
            .map(safe_endpoint)
            .unwrap_or_else(|| "默认地址".into());
        (provider, model, endpoint)
    } else {
        ("配置无法解析".into(), "无法读取".into(), "无法读取".into())
    }
}
pub(super) fn changes(before: &str, after: &str) -> Vec<Change> {
    let a = describe(before);
    let b = describe(after);
    vec![
        Change {
            label: "服务商".into(),
            before: a.0,
            after: b.0,
        },
        Change {
            label: "模型".into(),
            before: a.1,
            after: b.1,
        },
        Change {
            label: "API 地址".into(),
            before: a.2,
            after: b.2,
        },
    ]
}
