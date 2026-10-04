use super::paths::{self, directory, guard_path, hash, safe_hash, safe_id};
use crate::{core::AppPaths, security, threads::types::ThreadSummary};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

const CHUNK_BYTES: usize = 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(in crate::threads) struct SnapshotInfo {
    pub bytes: u64,
    pub hash: String,
    pub recoverable: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub version: u32,
    pub captured_at: String,
    pub source_root: String,
    pub source_bytes: u64,
    pub source_modified_at: Option<String>,
    pub bytes: u64,
    pub hash: String,
    pub chunks: Vec<Chunk>,
    pub thread: ThreadSummary,
    #[serde(skip)]
    pub warning: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Chunk {
    hash: String,
    bytes: u64,
}

fn object_path(paths: &AppPaths, source: &str, digest: &str) -> Result<PathBuf, String> {
    safe_id(source)?;
    safe_hash(digest)?;
    Ok(directory(paths)
        .join("objects")
        .join(source)
        .join(&digest[..2])
        .join(format!("{digest}.bin")))
}

fn manifest_directory(paths: &AppPaths, thread: &ThreadSummary) -> Result<PathBuf, String> {
    safe_id(&thread.source_id)?;
    Ok(directory(paths)
        .join("manifests")
        .join(&thread.source_id)
        .join(hash(thread.relative_path.as_bytes())))
}

fn manifest_name(thread: &ThreadSummary, digest: &str) -> Result<String, String> {
    safe_hash(digest)?;
    let metadata = serde_json::to_vec(&serde_json::json!({
        "threadId": thread.thread_id, "title": thread.title, "cwd": thread.cwd,
        "provider": thread.provider, "sourceKind": thread.source_kind,
        "createdAt": thread.created_at, "updatedAt": thread.updated_at, "integrity": thread.integrity,
    })).map_err(|_| "无法生成线程快照元数据标识。")?;
    Ok(format!("{digest}-{}.bin", hash(&metadata)))
}

pub(super) fn manifest_path(paths: &AppPaths, manifest: &Manifest) -> Result<PathBuf, String> {
    safe_hash(&manifest.hash)?;
    Ok(manifest_directory(paths, &manifest.thread)?
        .join(manifest_name(&manifest.thread, &manifest.hash)?))
}

pub(super) fn publish(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("线程保护文件路径无效。")?;
    paths::ensure_directory(parent)?;
    let temporary = parent.join(format!(".ahax-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "无法暂存线程保护数据。")?;
        file.write_all(content)
            .and_then(|_| file.sync_all())
            .map_err(|_| "线程保护数据未能完整写入。")?;
        drop(file);
        publish_file(&temporary, path)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

pub(super) fn publish_file(temporary: &Path, target: &Path) -> Result<(), String> {
    guard_path(target, true)?;
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
        let from = extended_path(temporary)?;
        let to = extended_path(target)?;
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
            let error = std::io::Error::last_os_error();
            return Err(format!(
                "线程保护文件未能发布（Windows 错误码 {}）；已有文件未被覆盖。",
                error.raw_os_error().unwrap_or(0)
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::hard_link(temporary, target)
            .map_err(|_| "线程保护文件未能发布；已有文件未被覆盖。".to_owned())
    }
}

#[cfg(windows)]
fn extended_path(path: &Path) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path).map_err(|_| "无法解析线程保护文件路径。")?;
    let native: Vec<u16> = absolute
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == b'/' as u16 {
                b'\\' as u16
            } else {
                unit
            }
        })
        .collect();
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let mut result = if native.starts_with(&prefix) {
        native
    } else if native.starts_with(&[b'\\' as u16, b'\\' as u16]) {
        let mut result: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        result.extend_from_slice(&native[2..]);
        result
    } else {
        let mut result = prefix;
        result.extend(native);
        result
    };
    result.push(0);
    Ok(result)
}

fn read_chunk(paths: &AppPaths, source: &str, chunk: &Chunk) -> Result<Zeroizing<Vec<u8>>, String> {
    if chunk.bytes == 0 || chunk.bytes > CHUNK_BYTES as u64 {
        return Err("线程快照分块长度无效。".into());
    }
    let path = object_path(paths, source, &chunk.hash)?;
    guard_path(&path, false)?;
    let metadata = fs::metadata(&path).map_err(|_| "线程快照分块缺失。")?;
    if !metadata.is_file() || metadata.len() > (CHUNK_BYTES + 64 * 1024) as u64 {
        return Err("线程快照分块格式无效。".into());
    }
    let encrypted = fs::read(path).map_err(|_| "无法读取线程快照分块。")?;
    let bytes = Zeroizing::new(security::unprotect(&encrypted)?);
    if bytes.len() as u64 != chunk.bytes || hash(&bytes) != chunk.hash {
        return Err("线程快照分块校验失败，未使用该副本。".into());
    }
    Ok(bytes)
}

pub(super) fn reader<'a>(paths: &'a AppPaths, manifest: &'a Manifest) -> impl Read + 'a {
    SnapshotReader {
        paths,
        manifest,
        next_chunk: 0,
        bytes: Zeroizing::new(Vec::new()),
        offset: 0,
    }
}

struct SnapshotReader<'a> {
    paths: &'a AppPaths,
    manifest: &'a Manifest,
    next_chunk: usize,
    bytes: Zeroizing<Vec<u8>>,
    offset: usize,
}

impl Read for SnapshotReader<'_> {
    fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
        if target.is_empty() {
            return Ok(0);
        }
        if self.offset == self.bytes.len() {
            let Some(chunk) = self.manifest.chunks.get(self.next_chunk) else {
                return Ok(0);
            };
            self.bytes = read_chunk(self.paths, &self.manifest.thread.source_id, chunk)
                .map_err(std::io::Error::other)?;
            self.next_chunk += 1;
            self.offset = 0;
        }
        let length = target.len().min(self.bytes.len() - self.offset);
        target[..length].copy_from_slice(&self.bytes[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}

fn store_chunk(paths: &AppPaths, source: &str, bytes: &[u8]) -> Result<Chunk, String> {
    let chunk = Chunk {
        hash: hash(bytes),
        bytes: bytes.len() as u64,
    };
    let target = object_path(paths, source, &chunk.hash)?;
    if target.exists() {
        read_chunk(paths, source, &chunk)?;
        return Ok(chunk);
    }
    let encrypted = security::protect(bytes)?;
    if let Err(error) = publish(&target, &encrypted) {
        if !target.exists() {
            return Err(error);
        }
    }
    read_chunk(paths, source, &chunk)?;
    Ok(chunk)
}

pub(in crate::threads) fn protect(
    paths: &AppPaths,
    thread: &ThreadSummary,
    bytes: &[u8],
) -> Result<SnapshotInfo, String> {
    if bytes.is_empty() {
        return Err("线程尚无完整记录可保护。".into());
    }
    let relative = paths::relative(&thread.relative_path)?;
    if !thread.relative_path.ends_with(".zst") && bytes.last() != Some(&b'\n') {
        return Err("线程仍在写入，请等待完整记录后再保护。".into());
    }
    let root = paths::source_root(Path::new(&thread.path), &relative)?;
    guard_path(&root.join(&relative), false)?;
    let digest = hash(bytes);
    if digest != thread.fingerprint {
        return Err("线程内容与扫描结果不一致，未保存该副本。".into());
    }
    let target_directory = manifest_directory(paths, thread)?;
    let target = target_directory.join(manifest_name(thread, &digest)?);
    if target.exists() {
        let manifest = load_manifest(&target)?;
        validate_identity(&manifest, thread)?;
        verify(paths, &manifest)?;
        return Ok(SnapshotInfo {
            bytes: manifest.bytes,
            hash: manifest.hash,
            recoverable: manifest.thread.integrity == "valid",
        });
    }
    let mut chunks = Vec::with_capacity(bytes.len().div_ceil(CHUNK_BYTES));
    for block in bytes.chunks(CHUNK_BYTES) {
        chunks.push(store_chunk(paths, &thread.source_id, block)?);
    }
    let metadata = fs::metadata(&thread.path).map_err(|_| "线程保护期间源文件不可用。")?;
    let source_modified_at = metadata
        .modified()
        .ok()
        .map(|time| chrono::DateTime::<Utc>::from(time).to_rfc3339());
    let manifest = Manifest {
        version: 1,
        captured_at: Utc::now().to_rfc3339(),
        source_root: root.to_string_lossy().into_owned(),
        source_bytes: metadata.len(),
        source_modified_at,
        bytes: bytes.len() as u64,
        hash: digest.clone(),
        chunks,
        thread: thread.clone(),
        warning: None,
    };
    verify(paths, &manifest)?;
    let raw = Zeroizing::new(serde_json::to_vec(&manifest).map_err(|_| "无法生成线程快照清单。")?);
    let encrypted = security::protect(&raw)?;
    if let Err(error) = publish(&target, &encrypted) {
        if !target.exists() {
            return Err(error);
        }
        let existing = load_manifest(&target)?;
        validate_identity(&existing, thread)?;
        verify(paths, &existing)?;
    }
    Ok(SnapshotInfo {
        bytes: bytes.len() as u64,
        hash: digest,
        recoverable: thread.integrity == "valid",
    })
}

pub(in crate::threads) fn protect_file(
    paths: &AppPaths,
    thread: &ThreadSummary,
    complete_bytes: u64,
) -> Result<SnapshotInfo, String> {
    if complete_bytes == 0 {
        return Err("线程尚无完整记录可保护。".into());
    }
    safe_hash(&thread.fingerprint)?;
    let relative = paths::relative(&thread.relative_path)?;
    let source = Path::new(&thread.path);
    let root = paths::source_root(source, &relative)?;
    guard_path(source, false)?;
    let mut file = File::open(source).map_err(|_| "无法读取待保护的线程记录。")?;
    let before = file.metadata().map_err(|_| "无法读取线程源文件状态。")?;
    if !before.is_file() || before.len() < complete_bytes {
        return Err("线程源文件在扫描后发生变化，请重新扫描。".into());
    }
    let target =
        manifest_directory(paths, thread)?.join(manifest_name(thread, &thread.fingerprint)?);
    if target.exists() {
        let manifest = load_manifest(&target)?;
        validate_identity(&manifest, thread)?;
        verify(paths, &manifest)?;
        if manifest.bytes != complete_bytes {
            return Err("已有线程快照长度与扫描结果不符。".into());
        }
        return Ok(SnapshotInfo {
            bytes: manifest.bytes,
            hash: manifest.hash,
            recoverable: manifest.thread.integrity == "valid",
        });
    }
    let mut remaining = complete_bytes;
    let mut buffer = Zeroizing::new(vec![0u8; CHUNK_BYTES]);
    let mut chunks = Vec::new();
    let mut digest = Sha256::new();
    let mut last_byte = None;
    while remaining > 0 {
        let length = remaining.min(CHUNK_BYTES as u64) as usize;
        file.read_exact(&mut buffer[..length])
            .map_err(|_| "线程读取中断，已完成的保护分块保留，未提交不完整快照。")?;
        let bytes = &buffer[..length];
        digest.update(bytes);
        chunks.push(store_chunk(paths, &thread.source_id, bytes)?);
        last_byte = bytes.last().copied();
        remaining -= length as u64;
    }
    let digest = format!("{:x}", digest.finalize());
    if digest != thread.fingerprint {
        return Err("线程内容在扫描后发生变化，未提交不一致的快照。".into());
    }
    if !thread.relative_path.ends_with(".zst") && last_byte != Some(b'\n') {
        return Err("线程保护边界不是完整记录，未提交快照。".into());
    }
    let after = fs::metadata(source).map_err(|_| "线程源文件在保护期间不可用。")?;
    if after.len() < before.len()
        || after.len() == before.len() && after.modified().ok() != before.modified().ok()
    {
        return Err("线程源文件在保护期间被替换或修改，已保留分块并等待重新扫描。".into());
    }
    guard_path(source, false)?;
    let manifest = Manifest {
        version: 1,
        captured_at: Utc::now().to_rfc3339(),
        source_root: root.to_string_lossy().into_owned(),
        source_bytes: before.len(),
        source_modified_at: before
            .modified()
            .ok()
            .map(|time| chrono::DateTime::<Utc>::from(time).to_rfc3339()),
        bytes: complete_bytes,
        hash: digest.clone(),
        chunks,
        thread: thread.clone(),
        warning: None,
    };
    verify(paths, &manifest)?;
    let raw = Zeroizing::new(serde_json::to_vec(&manifest).map_err(|_| "无法生成线程快照清单。")?);
    let encrypted = security::protect(&raw)?;
    if let Err(error) = publish(&target, &encrypted) {
        if !target.exists() {
            return Err(error);
        }
        let existing = load_manifest(&target)?;
        validate_identity(&existing, thread)?;
        verify(paths, &existing)?;
    }
    Ok(SnapshotInfo {
        bytes: complete_bytes,
        hash: digest,
        recoverable: thread.integrity == "valid",
    })
}

pub(super) fn load_manifest(path: &Path) -> Result<Manifest, String> {
    guard_path(path, false)?;
    let metadata = fs::metadata(path).map_err(|_| "无法读取线程快照清单。")?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err("线程快照清单大小无效。".into());
    }
    let bytes = fs::read(path).map_err(|_| "无法读取线程快照清单。")?;
    let raw = Zeroizing::new(security::unprotect(&bytes)?);
    let manifest: Manifest = serde_json::from_slice(&raw).map_err(|_| "线程快照清单无法解析。")?;
    if manifest.version != 1 {
        return Err("线程快照由更新版本创建，请更新应用后读取。".into());
    }
    safe_id(&manifest.thread.source_id)?;
    safe_hash(&manifest.hash)?;
    let relative = paths::relative(&manifest.thread.relative_path)?;
    if paths::normalized(&Path::new(&manifest.source_root).join(relative))?
        != paths::normalized(Path::new(&manifest.thread.path))?
    {
        return Err("线程快照来源校验失败。".into());
    }
    if manifest.chunks.is_empty()
        || manifest
            .chunks
            .iter()
            .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.bytes))
            != Some(manifest.bytes)
    {
        return Err("线程快照长度校验失败。".into());
    }
    Ok(manifest)
}

fn validate_identity(manifest: &Manifest, thread: &ThreadSummary) -> Result<(), String> {
    if manifest.thread.source_id != thread.source_id
        || manifest.thread.thread_id != thread.thread_id
        || manifest.thread.relative_path != thread.relative_path
        || manifest.thread.key != thread.key
    {
        return Err("线程快照与所选记录不匹配。".into());
    }
    Ok(())
}

pub(super) fn selected(
    paths: &AppPaths,
    thread: &ThreadSummary,
) -> Result<Option<Manifest>, String> {
    let folder = manifest_directory(paths, thread)?;
    if !folder.exists() {
        return Ok(None);
    }
    guard_path(&folder, false)?;
    let mut candidates = Vec::new();
    let mut damaged = false;
    for entry in fs::read_dir(&folder).map_err(|_| "无法读取线程快照目录。")? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                damaged = true;
                continue;
            }
        };
        if entry.path().extension().and_then(|value| value.to_str()) != Some("bin") {
            continue;
        }
        let candidate = match load_manifest(&entry.path()) {
            Ok(candidate) => candidate,
            Err(_) => {
                damaged = true;
                continue;
            }
        };
        if validate_identity(&candidate, thread).is_err() {
            damaged = true;
            continue;
        }
        candidates.push(candidate);
    }
    let newest = candidates
        .iter()
        .map(|candidate| candidate.captured_at.clone())
        .max();
    candidates.sort_by(|left, right| {
        (right.thread.integrity == "valid")
            .cmp(&(left.thread.integrity == "valid"))
            .then_with(|| {
                (right.hash == thread.fingerprint).cmp(&(left.hash == thread.fingerprint))
            })
            .then_with(|| right.captured_at.cmp(&left.captured_at))
    });
    let mut rejected = false;
    for mut candidate in candidates {
        if verify(paths, &candidate).is_err() {
            damaged = true;
            rejected = true;
            continue;
        }
        if rejected
            || newest
                .as_ref()
                .is_some_and(|time| time > &candidate.captured_at)
        {
            candidate.warning = Some(
                "较新的副本未通过校验或记录不完整，当前使用较早的完整副本。原有副本已保留。".into(),
            );
        } else if damaged {
            candidate.warning =
                Some("部分历史副本无法校验；当前副本已验证，损坏证据已保留。".into());
        }
        return Ok(Some(candidate));
    }
    if damaged {
        Err("此线程的保护副本无法通过校验，已保留损坏证据；其他线程不受影响。".into())
    } else {
        Ok(None)
    }
}

pub(super) fn stream(
    paths: &AppPaths,
    manifest: &Manifest,
    output: &mut impl Write,
) -> Result<(), String> {
    let mut digest = Sha256::new();
    let mut length = 0u64;
    for chunk in &manifest.chunks {
        let bytes = read_chunk(paths, &manifest.thread.source_id, chunk)?;
        output.write_all(&bytes).map_err(|_| "线程副本写入失败。")?;
        digest.update(&bytes);
        length = length
            .checked_add(bytes.len() as u64)
            .ok_or("线程副本长度溢出。")?;
    }
    if length != manifest.bytes || format!("{:x}", digest.finalize()) != manifest.hash {
        return Err("线程快照完整性校验失败，未使用该副本。".into());
    }
    Ok(())
}

pub(super) fn verify(paths: &AppPaths, manifest: &Manifest) -> Result<(), String> {
    stream(paths, manifest, &mut std::io::sink())
}

pub(in crate::threads) fn snapshot(
    paths: &AppPaths,
    thread: &ThreadSummary,
) -> Result<Option<SnapshotInfo>, String> {
    selected(paths, thread)?
        .map(|manifest| {
            Ok(SnapshotInfo {
                bytes: manifest.bytes,
                hash: manifest.hash,
                recoverable: manifest.thread.integrity == "valid",
            })
        })
        .transpose()
}

pub(in crate::threads) fn snapshot_bytes(
    paths: &AppPaths,
    thread: &ThreadSummary,
) -> Result<Option<Vec<u8>>, String> {
    selected(paths, thread)?
        .map(|manifest| {
            if manifest.bytes > 64 * 1024 * 1024 {
                return Err("此线程较大，请使用原始记录查看；保护副本仍完整保留。".into());
            }
            let mut bytes = Vec::with_capacity(manifest.bytes as usize);
            stream(paths, &manifest, &mut bytes)?;
            Ok(bytes)
        })
        .transpose()
}

pub(super) fn file_hash(path: &Path) -> Result<Option<String>, String> {
    guard_path(path, true)?;
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取线程目标文件。".into()),
    };
    let before = file.metadata().map_err(|_| "无法读取线程目标状态。")?;
    if !before.is_file() {
        return Err("线程目标不是普通文件。".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = Zeroizing::new(vec![0u8; CHUNK_BYTES]);
    let mut count = 0u64;
    loop {
        let amount = file
            .read(&mut buffer)
            .map_err(|_| "无法校验线程目标文件。")?;
        if amount == 0 {
            break;
        }
        digest.update(&buffer[..amount]);
        count += amount as u64;
    }
    let after = fs::metadata(path).map_err(|_| "线程目标在检查期间发生变化。")?;
    if before.len() != count
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("线程目标正在被其他程序修改，请稍后重新预览。".into());
    }
    Ok(Some(format!("{:x}", digest.finalize())))
}
