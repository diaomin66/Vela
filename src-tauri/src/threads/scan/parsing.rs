use super::super::types::{ThreadHistoryBase, ThreadMessagePreview};
use serde_json::Value;
use std::{collections::HashMap, io::Read, path::Path};
use uuid::Uuid;

const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PREVIEW_MESSAGES: usize = 100;
const MAX_PREVIEW_CHARS: usize = 4_000;
const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DETAIL_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Parsed {
    pub thread_id: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub provider: Option<String>,
    pub source_kind: Option<String>,
    pub history_base: Option<ThreadHistoryBase>,
    pub created_at: Option<String>,
    pub lines: u64,
    pub integrity: String,
    pub preview: Vec<ThreadMessagePreview>,
}

pub(super) fn parse(path: &Path, raw: &[u8], include_preview: bool) -> Parsed {
    let mut parsed = Parsed::default();
    let mut decompressed = Vec::new();
    let bytes = if path.extension().is_some_and(|extension| extension == "zst") {
        let decoded = zstd::stream::read::Decoder::new(raw).and_then(|reader| {
            reader
                .take(super::files::MAX_BYTES + 1)
                .read_to_end(&mut decompressed)
        });
        if decoded.is_err() {
            parsed.integrity = "unreadable".into();
            return parsed;
        }
        if decompressed.len() as u64 > super::files::MAX_BYTES {
            parsed.integrity = "too-large".into();
            return parsed;
        }
        decompressed.as_slice()
    } else {
        raw
    };
    let mut malformed = false;
    let mut unsupported = false;
    let mut own_metadata = false;
    let prefix_length = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    for line in bytes[..prefix_length].split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        parsed.lines += 1;
        if line.len() > MAX_LINE_BYTES {
            malformed |= serde_json::from_slice::<serde::de::IgnoredAny>(line).is_err();
            continue;
        }
        let value: Value = match serde_json::from_slice(line) {
            Ok(value) => value,
            Err(_) => {
                malformed = true;
                continue;
            }
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            unsupported = true;
            continue;
        };
        let Some(payload) = value.get("payload") else {
            unsupported = true;
            continue;
        };
        if kind == "session_meta" && !own_metadata {
            own_metadata = true;
            parsed.thread_id = string(payload, "id")
                .or_else(|| string(payload, "thread_id"))
                .or_else(|| string(payload, "session_id"))
                .filter(|id| Uuid::parse_str(id).is_ok());
            parsed.cwd = string(payload, "cwd");
            parsed.provider = string(payload, "model_provider");
            parsed.created_at =
                string(payload, "timestamp").or_else(|| string(&value, "timestamp"));
            parsed.source_kind = payload.get("source").and_then(source_kind);
            match history_base(payload.get("history_base")) {
                Ok(base) => parsed.history_base = base,
                Err(()) => unsupported = true,
            }
            if payload
                .get("history_mode")
                .and_then(Value::as_str)
                .is_some_and(|mode| !matches!(mode, "legacy" | "paginated"))
            {
                unsupported = true;
            }
        }
        if kind == "turn_context" {
            parsed.cwd = string(payload, "cwd").or(parsed.cwd);
        }
        if let Some(message) = include_preview.then(|| message(&value)).flatten() {
            if parsed.preview.len() < MAX_PREVIEW_MESSAGES
                && !parsed
                    .preview
                    .last()
                    .is_some_and(|old| old.role == message.role && old.text == message.text)
            {
                parsed.preview.push(message);
            }
        }
    }
    let identity_mismatch = filename_id(path)
        .zip(parsed.thread_id.as_ref())
        .is_some_and(|(filename, actual)| &filename != actual);
    parsed.integrity =
        if !own_metadata || parsed.thread_id.is_none() || unsupported || identity_mismatch {
            "unrecognized"
        } else if malformed {
            "corrupt"
        } else if !bytes.ends_with(b"\n") {
            "partial"
        } else {
            "valid"
        }
        .into();
    parsed
}

pub(super) fn preview_file(root: &Path, path: &Path) -> Vec<ThreadMessagePreview> {
    if super::files::validate_path(root, path).is_err() {
        return Vec::new();
    }
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let reader: Box<dyn Read> = if path.extension().is_some_and(|extension| extension == "zst") {
        match zstd::stream::read::Decoder::new(file) {
            Ok(reader) => Box::new(reader),
            Err(_) => return Vec::new(),
        }
    } else {
        Box::new(file)
    };
    let mut bytes = Vec::new();
    if reader
        .take(MAX_DETAIL_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Vec::new();
    }
    parse(Path::new("preview.jsonl"), &bytes, true).preview
}

pub(super) fn read_names(root: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let path = root.join("session_index.jsonl");
    if std::fs::metadata(&path)
        .map(|metadata| metadata.len() > MAX_INDEX_BYTES)
        .unwrap_or(true)
    {
        return names;
    }
    let Ok(stable) = super::files::stable_read(root, &path) else {
        return names;
    };
    for line in stable.bytes.split(|byte| *byte == b'\n').rev() {
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let Some(id) = string(&value, "id").filter(|id| Uuid::parse_str(id).is_ok()) else {
            continue;
        };
        let Some(name) = string(&value, "thread_name").filter(|name| !name.trim().is_empty())
        else {
            continue;
        };
        names
            .entry(id)
            .or_insert_with(|| super::redaction::text(&shorten(name.trim(), 256)));
    }
    names
}

pub(in crate::threads) fn filename_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.strip_prefix("rollout-")?;
    let name = name
        .strip_suffix(".zst")
        .unwrap_or(name)
        .strip_suffix(".jsonl")?;
    for (offset, character) in name.char_indices() {
        if character != '-' {
            continue;
        }
        let value = name.get(offset + 1..)?.split('_').next()?;
        if let Ok(id) = Uuid::parse_str(value) {
            return Some(id.to_string());
        }
    }
    None
}

pub(in crate::threads) fn filename_rollout_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.strip_prefix("rollout-")?;
    let name = name
        .strip_suffix(".zst")
        .unwrap_or(name)
        .strip_suffix(".jsonl")?;
    match name.rsplit_once('_') {
        Some((_, id)) => Uuid::parse_str(id).ok().map(|id| id.to_string()),
        None => filename_id(path),
    }
}

pub(super) fn history_base(value: Option<&Value>) -> Result<Option<ThreadHistoryBase>, ()> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let thread_id = value
        .get("thread_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(())?
        .to_string();
    let end_ordinal_exclusive = value
        .get("end_ordinal_exclusive")
        .and_then(Value::as_u64)
        .ok_or(())?;
    let end_byte_offset = match value.get("end_byte_offset") {
        Some(Value::Null) | None => None,
        Some(value) => Some(value.as_u64().ok_or(())?),
    };
    Ok(Some(ThreadHistoryBase {
        thread_id,
        end_ordinal_exclusive,
        end_byte_offset,
    }))
}

fn string(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_owned)
}

fn source_kind(value: &Value) -> Option<String> {
    if let Some(kind) = value.as_str() {
        return Some(kind.to_owned());
    }
    value.as_object()?.keys().next().cloned()
}

fn message(value: &Value) -> Option<ThreadMessagePreview> {
    let payload = value.get("payload")?;
    let (role, text) = match value.get("type")?.as_str()? {
        "response_item" if payload.get("type")?.as_str()? == "message" => {
            let role = payload.get("role")?.as_str()?;
            if !matches!(role, "user" | "assistant") {
                return None;
            }
            let text = match payload.get("content")? {
                Value::String(text) => text.to_owned(),
                Value::Array(content) => content
                    .iter()
                    .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                        Some("input_text" | "output_text" | "text") => {
                            part.get("text").and_then(Value::as_str)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => return None,
            };
            (role, text)
        }
        "event_msg" => {
            let role = match payload.get("type")?.as_str()? {
                "user_message" => "user",
                "agent_message" => "assistant",
                _ => return None,
            };
            (role, payload.get("message")?.as_str()?.to_owned())
        }
        _ => return None,
    };
    if text.trim().is_empty() {
        return None;
    }
    Some(ThreadMessagePreview {
        role: Some(role.to_owned()),
        text: super::redaction::text(&shorten(text.trim(), MAX_PREVIEW_CHARS)),
        timestamp: string(value, "timestamp"),
    })
}

fn shorten(text: &str, maximum: usize) -> String {
    let mut output = text.chars().take(maximum).collect::<String>();
    if text.chars().nth(maximum).is_some() {
        output.push('…');
    }
    output
}
