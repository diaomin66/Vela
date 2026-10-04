use super::{inventory, paths, vault};
use crate::{
    core::AppPaths,
    threads::types::{ThreadHistoryBase, ThreadSummary},
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::Path,
};

const MAX_LINE: u64 = 8 * 1024 * 1024;

#[derive(Serialize)]
pub(super) struct Proof {
    key: String,
    path: String,
    prefix_hash: String,
    end_ordinal_exclusive: u64,
    end_byte_offset: u64,
}

pub(super) struct Validated {
    pub proofs: Vec<Proof>,
    _readers: Vec<File>,
}

fn line(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_LINE + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "历史依赖读取失败，请结束正在写入的任务后重试。")?;
    if bytes.len() as u64 > MAX_LINE {
        return Err("历史依赖包含暂无法安全验证的超大记录，未自动恢复。".into());
    }
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        return Err("历史依赖只有不完整记录，不能恢复为完整线程。".into());
    }
    Ok(bytes)
}

fn history_base(value: &Value) -> Result<Option<ThreadHistoryBase>, String> {
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Err("线程副本缺少有效的起始元数据，未自动恢复。".into());
    }
    let Some(raw) = value
        .get("payload")
        .and_then(|payload| payload.get("history_base"))
        .filter(|base| !base.is_null())
    else {
        return Ok(None);
    };
    if value
        .get("payload")
        .and_then(|payload| payload.get("history_mode"))
        .and_then(Value::as_str)
        != Some("paginated")
    {
        return Err("历史依赖使用未知分页模式，未自动恢复。".into());
    }
    let thread_id = raw
        .get("thread_id")
        .and_then(Value::as_str)
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .ok_or("历史依赖标识无效，未自动恢复。")?
        .to_string();
    let end_ordinal_exclusive = raw
        .get("end_ordinal_exclusive")
        .and_then(Value::as_u64)
        .ok_or("历史依赖边界无效，未自动恢复。")?;
    let end_byte_offset = match raw.get("end_byte_offset") {
        None => None,
        Some(value) => Some(value.as_u64().ok_or("历史依赖字节边界无效，未自动恢复。")?),
    };
    Ok(Some(ThreadHistoryBase {
        thread_id,
        end_ordinal_exclusive,
        end_byte_offset,
    }))
}

fn head(reader: &mut impl BufRead) -> Result<(Vec<u8>, Value), String> {
    let mut leading = Vec::new();
    loop {
        let bytes = line(reader)?;
        if bytes.is_empty() {
            return Err("线程副本没有可验证的元数据。".into());
        }
        if leading.len().saturating_add(bytes.len()) > MAX_LINE as usize {
            return Err("线程副本起始部分超过安全读取限制。".into());
        }
        leading.extend_from_slice(&bytes);
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| "线程副本起始元数据无法解析。")?;
        return Ok((leading, value));
    }
}

fn snapshot_base(
    paths: &AppPaths,
    manifest: &vault::Manifest,
) -> Result<Option<ThreadHistoryBase>, String> {
    let stream = vault::reader(paths, manifest);
    let decoded: Box<dyn Read + '_> = if manifest.thread.relative_path.ends_with(".zst") {
        Box::new(zstd::stream::read::Decoder::new(stream).map_err(|_| "压缩线程副本无法解码。")?)
    } else {
        Box::new(stream)
    };
    let (_, value) = head(&mut BufReader::new(decoded))?;
    history_base(&value)
}

fn rollout_id(path: &str) -> Option<String> {
    let name = Path::new(path).file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".zst")
        .unwrap_or(name)
        .strip_suffix(".jsonl")?;
    if !stem.starts_with("rollout-") {
        return None;
    }
    let suffix = stem.get(stem.len().checked_sub(36)?..)?;
    uuid::Uuid::parse_str(suffix).ok().map(|id| id.to_string())
}

fn source_reader(thread: &ThreadSummary) -> Result<File, String> {
    let path = Path::new(&thread.path);
    paths::guard_path(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        options.share_mode(FILE_SHARE_READ);
    }
    options
        .open(path)
        .map_err(|_| "历史依赖正在使用或无法读取，请关闭正在使用该线程的客户端后重新预览。".into())
}

fn check_prefix(
    thread: &ThreadSummary,
    required: &ThreadHistoryBase,
) -> Result<(Proof, Option<ThreadHistoryBase>, File), String> {
    if required.end_ordinal_exclusive == 0 {
        return Err("历史依赖边界不能早于起始元数据，未自动恢复。".into());
    }
    let file = source_reader(thread)?;
    let stream = file.try_clone().map_err(|_| "无法读取历史依赖。")?;
    let decoded: Box<dyn Read> = if thread.relative_path.ends_with(".zst") {
        Box::new(zstd::stream::read::Decoder::new(stream).map_err(|_| "压缩历史依赖无法解码。")?)
    } else {
        Box::new(stream)
    };
    let mut reader = BufReader::new(decoded);
    let (first, value) = head(&mut reader)?;
    if value
        .get("payload")
        .and_then(|payload| payload.get("history_mode"))
        .and_then(Value::as_str)
        != Some("paginated")
    {
        return Err("历史依赖不是分页记录，未自动恢复。".into());
    }
    let base = history_base(&value)?;
    let mut expected = base.as_ref().map_or(0, |base| base.end_ordinal_exclusive);
    let mut offset = 0u64;
    let mut digest = Sha256::new();
    if required.end_ordinal_exclusive < expected {
        return Err("历史依赖边界早于可用记录，不能确认完整恢复。".into());
    }
    if required.end_ordinal_exclusive != expected {
        let mut current = first;
        let mut value = value;
        loop {
            let ordinal = value
                .get("ordinal")
                .and_then(Value::as_u64)
                .ok_or("历史依赖没有可验证的记录序号，请先修复依赖线程。")?;
            if ordinal != expected {
                return Err("历史依赖的记录序号存在缺口，不能确认完整恢复。".into());
            }
            expected = ordinal.checked_add(1).ok_or("历史依赖记录序号溢出。")?;
            offset = offset
                .checked_add(current.len() as u64)
                .ok_or("历史依赖字节边界溢出。")?;
            digest.update(&current);
            if expected == required.end_ordinal_exclusive {
                break;
            }
            if expected > required.end_ordinal_exclusive {
                return Err("历史依赖边界早于可用记录，不能确认完整恢复。".into());
            }
            loop {
                current = line(&mut reader)?;
                if current.is_empty() {
                    return Err("历史依赖缺少所需记录，请先找回依赖线程再恢复。".into());
                }
                if current.iter().all(u8::is_ascii_whitespace) {
                    offset = offset
                        .checked_add(current.len() as u64)
                        .ok_or("历史依赖字节边界溢出。")?;
                    digest.update(&current);
                    continue;
                }
                value = serde_json::from_slice(&current)
                    .map_err(|_| "历史依赖包含损坏记录，不能确认完整恢复。")?;
                break;
            }
        }
    }
    if required
        .end_byte_offset
        .is_some_and(|limit| limit != offset)
    {
        return Err("历史依赖的字节边界与记录序号不一致，请先找回完整依赖。".into());
    }
    let proof = Proof {
        key: thread.key.clone(),
        path: paths::normalized(Path::new(&thread.path))?,
        prefix_hash: format!("{:x}", digest.finalize()),
        end_ordinal_exclusive: required.end_ordinal_exclusive,
        end_byte_offset: offset,
    };
    Ok((proof, base, file))
}

pub(super) fn validate(paths: &AppPaths, manifest: &vault::Manifest) -> Result<Validated, String> {
    let mut base = snapshot_base(paths, manifest)?;
    let mut result = Validated {
        proofs: Vec::new(),
        _readers: Vec::new(),
    };
    if base.is_none() {
        return Ok(result);
    }
    let index = inventory::read(paths)?;
    let source_root = Path::new(&manifest.source_root);
    let mut visited = HashSet::new();
    if let Some(id) = rollout_id(&manifest.thread.relative_path) {
        visited.insert(id);
    }
    while let Some(required) = base {
        if !visited.insert(required.thread_id.clone()) {
            return Err("线程历史依赖形成循环，未自动恢复任何文件。".into());
        }
        let mut existing = Vec::new();
        for thread in index.threads.iter().filter(|thread| {
            thread.source_id == manifest.thread.source_id
                && rollout_id(&thread.relative_path).as_deref() == Some(required.thread_id.as_str())
        }) {
            let relative = paths::relative(&thread.relative_path)?;
            if paths::normalized(&source_root.join(relative))?
                != paths::normalized(Path::new(&thread.path))?
            {
                return Err("历史依赖不属于同一线程来源，未自动恢复。".into());
            }
            if Path::new(&thread.path).is_file() {
                existing.push(thread);
            }
        }
        if existing.is_empty() {
            return Err(format!(
                "缺少历史依赖 {}，请先找回并恢复依赖线程，再恢复当前线程。",
                required.thread_id
            ));
        }
        if existing.len() != 1 {
            return Err(format!(
                "历史依赖 {} 有多个文件候选，请先处理副本冲突再恢复。",
                required.thread_id
            ));
        }
        let (proof, parent, reader) = check_prefix(existing[0], &required)?;
        result.proofs.push(proof);
        result._readers.push(reader);
        base = parent;
    }
    Ok(result)
}

pub(in crate::threads) fn validate_history_prefix(
    thread: &ThreadSummary,
    required: &ThreadHistoryBase,
) -> Result<(), String> {
    check_prefix(thread, required).map(|_| ())
}

#[cfg(test)]
mod tests;
