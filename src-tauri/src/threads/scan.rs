mod dependencies;
mod files;
mod parsing;
mod redaction;
mod state;
mod streaming;

use super::{storage, types::*};
use crate::core::AppPaths;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(super) fn run(paths: &AppPaths) -> Result<ThreadIndex, String> {
    run_with_mode(paths, true, false)
}

pub(super) fn run_incremental(paths: &AppPaths) -> Result<ThreadIndex, String> {
    run_with_mode(paths, false, false)
}

pub(super) fn checkpoint(paths: &AppPaths) -> Result<ThreadIndex, String> {
    run_with_mode(paths, false, true)
}

fn run_with_mode(paths: &AppPaths, force: bool, checkpoint: bool) -> Result<ThreadIndex, String> {
    let mut index = storage::read(paths)?;
    let enabled = index.settings.enabled;
    if checkpoint {
        index.settings.enabled = true;
    }
    let mut roots = vec![paths
        .config
        .parent()
        .ok_or("无法定位线程目录。")?
        .to_path_buf()];
    roots.extend(
        index
            .sources
            .iter()
            .map(|source| PathBuf::from(&source.root)),
    );
    let sqlite_home = std::env::var_os("CODEX_SQLITE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let mut index = run_scan(paths, index, roots, sqlite_home.as_deref(), force)?;
    index.settings.enabled = enabled;
    Ok(index)
}

#[cfg(test)]
fn run_with_sources(
    paths: &AppPaths,
    index: ThreadIndex,
    roots: Vec<PathBuf>,
) -> Result<ThreadIndex, String> {
    run_scan(paths, index, roots, None, true)
}

fn run_scan(
    paths: &AppPaths,
    mut index: ThreadIndex,
    roots: Vec<PathBuf>,
    sqlite_home: Option<&Path>,
    force: bool,
) -> Result<ThreadIndex, String> {
    let at = Utc::now().to_rfc3339();
    let revision = Uuid::new_v4().to_string();
    let previous: HashMap<_, _> = index
        .threads
        .drain(..)
        .map(|thread| (thread.key.clone(), thread))
        .collect();
    let mut threads = Vec::new();
    let mut sources = Vec::new();
    let mut seen_roots = HashSet::new();
    let mut seen_keys = HashSet::new();
    let mut available_sources = HashSet::new();
    let mut complete_sources = HashSet::new();
    let mut errors = Vec::new();

    for root in roots {
        if !root.is_absolute() {
            continue;
        }
        let available = files::root_available(&root);
        let root = if available {
            fs::canonicalize(&root).unwrap_or(root)
        } else {
            root
        };
        let normalized = files::normalized_path(&root);
        if !seen_roots.insert(normalized.clone()) {
            continue;
        }
        let source_id = digest(normalized.as_bytes());
        let mut source = ThreadSource {
            id: source_id.clone(),
            kind: "local".into(),
            root: root.to_string_lossy().into_owned(),
            display_root: files::display_path(&root),
            available,
            writable: fs::metadata(&root)
                .map(|meta| !meta.permissions().readonly())
                .unwrap_or(false),
            last_scanned_at: Some(at.clone()),
            error: None,
        };
        if !available {
            let has_history = previous
                .values()
                .any(|thread| thread.source_id == source_id);
            if has_history || !files::root_missing_safe(&root) {
                source.error = Some("此来源暂时不可用，已有保护记录仍保留。".into());
            }
            sources.push(source);
            continue;
        }
        available_sources.insert(source_id.clone());
        let names = parsing::read_names(&root);
        let state_home = if paths.config.parent().is_some_and(|current| {
            files::normalized_path(
                &fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf()),
            ) == normalized
        }) {
            sqlite_home
        } else {
            None
        };
        let state = state::read(&root, state_home);
        let discovered = files::discover(&root, index.settings.include_archived);
        if let Some(error) = discovered.error {
            source.error = Some(error.clone());
            errors.push(error);
        } else {
            complete_sources.insert(source_id.clone());
        }
        for (path, archived) in discovered.paths {
            let relative = match path.strip_prefix(&root) {
                Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let key = digest(format!("{source_id}\0{relative}").as_bytes());
            seen_keys.insert(key.clone());
            let old = previous.get(&key);
            let mut thread = old.cloned().unwrap_or_else(|| ThreadSummary {
                key: key.clone(),
                source_id: source_id.clone(),
                thread_id: parsing::filename_id(&path).unwrap_or_else(|| key.clone()),
                path: path.to_string_lossy().into_owned(),
                relative_path: relative.clone(),
                archived,
                title: None,
                cwd: None,
                provider: None,
                source_kind: None,
                history_base: None,
                created_at: None,
                updated_at: None,
                bytes: 0,
                line_count: 0,
                index_present: false,
                state_index: None,
                selected_rollout: None,
                last_verified_at: None,
                protected_bytes: None,
                integrity: "unrecognized".into(),
                snapshot: SnapshotState::Pending,
                recoverability: "source-present".into(),
                fingerprint: String::new(),
                scan_revision: revision.clone(),
            });
            thread.scan_revision = revision.clone();
            thread.path = path.to_string_lossy().into_owned();
            thread.relative_path = relative;
            thread.source_id = source_id.clone();
            thread.archived = archived;
            thread.recoverability = "source-present".into();
            if !force && unchanged_verified(&thread, &path) {
                if let Some(name) = names.get(&thread.thread_id) {
                    thread.title = Some(name.clone());
                }
                thread.index_present = names.contains_key(&thread.thread_id);
                state.apply(&mut thread);
                threads.push(thread);
                continue;
            }
            match files::stable_read(&root, &path) {
                Ok(stable) => {
                    let bytes = stable.bytes;
                    thread.bytes = bytes.len() as u64;
                    thread.fingerprint = digest(&bytes);
                    thread.updated_at = stable.modified_at;
                    let parsed = parsing::parse(&path, &bytes, false);
                    if parsed.integrity == "too-large" {
                        if scan_streamed(
                            paths,
                            &root,
                            &path,
                            &mut thread,
                            index.settings.enabled,
                            &names,
                            &state,
                            &at,
                            &mut errors,
                        )
                        .is_err()
                        {
                            thread.integrity = "unreadable".into();
                            thread.snapshot = SnapshotState::Failed;
                        }
                        threads.push(thread);
                        continue;
                    }
                    thread.line_count = parsed.lines;
                    thread.integrity = parsed.integrity.clone();
                    thread.thread_id = parsed.thread_id.unwrap_or(thread.thread_id);
                    thread.cwd = parsed.cwd.or(thread.cwd);
                    thread.provider = parsed.provider.or(thread.provider);
                    thread.source_kind = parsed.source_kind.or(thread.source_kind);
                    thread.history_base = parsed.history_base;
                    thread.created_at = parsed.created_at.or(thread.created_at);
                    thread.title = names
                        .get(&thread.thread_id)
                        .cloned()
                        .or(parsed.title)
                        .or(thread.title);
                    thread.index_present = names.contains_key(&thread.thread_id);
                    state.apply(&mut thread);
                    if index.settings.enabled {
                        protect(paths, &mut thread, &bytes, &path, &mut errors);
                    } else {
                        thread.snapshot = snapshot_state(paths, &thread);
                    }
                    thread.last_verified_at = Some(at.clone());
                    if !same_source_version(&thread, &path) {
                        thread.integrity = "changed".into();
                        thread.snapshot = SnapshotState::Pending;
                    }
                }
                Err(files::ReadFailure::TooLarge(size)) => {
                    thread.bytes = size;
                    match scan_streamed(
                        paths,
                        &root,
                        &path,
                        &mut thread,
                        index.settings.enabled,
                        &names,
                        &state,
                        &at,
                        &mut errors,
                    ) {
                        Ok(()) => {}
                        Err(files::ReadFailure::Changed) => {
                            thread.integrity = "changed".into();
                            thread.snapshot = SnapshotState::Pending;
                        }
                        Err(_) => {
                            thread.integrity = "unreadable".into();
                            thread.snapshot = SnapshotState::Failed;
                        }
                    }
                }
                Err(files::ReadFailure::Changed) => {
                    thread.integrity = "changed".into();
                    thread.snapshot = SnapshotState::Pending;
                }
                Err(files::ReadFailure::Unreadable) => {
                    thread.integrity = "unreadable".into();
                    thread.snapshot = SnapshotState::Failed;
                }
            }
            threads.push(thread);
        }
        sources.push(source);
    }

    for (_, mut old) in previous {
        if seen_keys.contains(&old.key) {
            continue;
        }
        if threads
            .iter()
            .any(|current| same_rollout_moved(&old, current))
        {
            continue;
        }
        old.scan_revision = revision.clone();
        if !available_sources.contains(&old.source_id) {
            old.integrity = "source-unavailable".into();
            old.snapshot = SnapshotState::Pending;
            old.recoverability = "unavailable".into();
            threads.push(old);
            continue;
        }
        if old.archived && !index.settings.include_archived {
            threads.push(old);
            continue;
        }
        if !complete_sources.contains(&old.source_id) {
            old.integrity = "unreadable".into();
            old.snapshot = SnapshotState::Pending;
            old.recoverability = "unavailable".into();
            threads.push(old);
            continue;
        }
        if Path::new(&old.path).exists() {
            old.integrity = "unreadable".into();
            old.snapshot = SnapshotState::Pending;
            old.recoverability = "source-present".into();
        } else {
            old.integrity = "missing".into();
            match storage::snapshot(paths, &old) {
                Ok(Some(snapshot)) if snapshot.recoverable => {
                    old.snapshot = SnapshotState::Protected;
                    old.recoverability = "recoverable".into();
                    old.protected_bytes = Some(snapshot.bytes);
                }
                Ok(Some(_)) => {
                    old.snapshot = SnapshotState::Failed;
                    old.recoverability = "unavailable".into();
                }
                Ok(None) => {
                    old.snapshot = SnapshotState::Missing;
                    old.recoverability = "unavailable".into();
                }
                Err(_) => {
                    old.snapshot = SnapshotState::Failed;
                    old.recoverability = "unavailable".into();
                }
            }
        }
        threads.push(old);
    }

    dependencies::apply(&mut threads);
    threads.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then(left.key.cmp(&right.key))
    });
    let protected_count = threads
        .iter()
        .filter(|thread| thread.snapshot == SnapshotState::Protected)
        .count() as u64;
    let pending_count = threads
        .iter()
        .filter(|thread| thread.snapshot == SnapshotState::Pending)
        .count() as u64;
    let failed_count = threads
        .iter()
        .filter(|thread| {
            matches!(
                thread.snapshot,
                SnapshotState::Failed | SnapshotState::TooLarge | SnapshotState::Missing
            )
        })
        .count() as u64;
    let bytes_protected = threads
        .iter()
        .filter_map(|thread| thread.protected_bytes)
        .sum();
    let successful = index.settings.enabled
        && pending_count == 0
        && failed_count == 0
        && errors.is_empty()
        && sources.iter().all(|source| source.error.is_none());
    index.protection = ThreadProtectionStatus {
        state: if !index.settings.enabled {
            "paused"
        } else if successful {
            "protected"
        } else {
            "attention"
        }
        .into(),
        last_success_at: if successful {
            Some(at.clone())
        } else {
            index.protection.last_success_at
        },
        last_attempt_at: Some(at.clone()),
        protected_count,
        pending_count,
        failed_count,
        bytes_protected,
        current_path: None,
        error: (!errors.is_empty()).then(|| errors.join(" ")),
    };
    index.sources = sources;
    index.threads = threads;
    index.last_scan_at = Some(at);
    index.scan_revision = revision;
    Ok(index)
}

fn protect(
    paths: &AppPaths,
    thread: &mut ThreadSummary,
    bytes: &[u8],
    path: &Path,
    errors: &mut Vec<String>,
) {
    let is_compressed = path.extension().is_some_and(|extension| extension == "zst");
    let end = if is_compressed {
        if thread.integrity == "unreadable" || thread.integrity == "too-large" {
            thread.snapshot = SnapshotState::Failed;
            return;
        }
        bytes.len()
    } else {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|at| at + 1)
            .unwrap_or(0)
    };
    if end == 0 {
        thread.snapshot = SnapshotState::Pending;
        return;
    }
    let stable = &bytes[..end];
    let mut candidate = thread.clone();
    candidate.bytes = stable.len() as u64;
    candidate.fingerprint = digest(stable);
    if end != bytes.len() {
        candidate.integrity = parsing::parse(path, stable, false).integrity;
    }
    match storage::protect(paths, &candidate, stable) {
        Ok(snapshot) => {
            thread.snapshot = if candidate.integrity != "valid" {
                SnapshotState::Failed
            } else if end == bytes.len() {
                SnapshotState::Protected
            } else {
                SnapshotState::Pending
            };
            thread.protected_bytes = Some(snapshot.bytes);
        }
        Err(error) => {
            thread.snapshot = SnapshotState::Failed;
            if !errors.contains(&error) && errors.len() < 8 {
                errors.push(error);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_streamed(
    paths: &AppPaths,
    root: &Path,
    path: &Path,
    thread: &mut ThreadSummary,
    enabled: bool,
    names: &HashMap<String, String>,
    state: &state::StateIndex,
    at: &str,
    errors: &mut Vec<String>,
) -> Result<(), files::ReadFailure> {
    let streamed = streaming::scan(root, path)?;
    thread.bytes = streamed.bytes;
    thread.updated_at = streamed.modified_at;
    thread.fingerprint = streamed.fingerprint;
    thread.thread_id = streamed
        .parsed
        .thread_id
        .unwrap_or(thread.thread_id.clone());
    thread.line_count = streamed.parsed.lines;
    thread.integrity = streamed.parsed.integrity;
    thread.cwd = streamed.parsed.cwd.or(thread.cwd.take());
    thread.provider = streamed.parsed.provider.or(thread.provider.take());
    thread.created_at = streamed.parsed.created_at.or(thread.created_at.take());
    thread.source_kind = streamed.parsed.source_kind.or(thread.source_kind.take());
    thread.history_base = streamed.parsed.history_base;
    thread.title = names
        .get(&thread.thread_id)
        .cloned()
        .or(thread.title.take());
    thread.index_present = names.contains_key(&thread.thread_id);
    state.apply(thread);
    if enabled && streamed.prefix_bytes > 0 {
        let mut candidate = thread.clone();
        candidate.bytes = streamed.prefix_bytes;
        candidate.fingerprint = streamed.prefix_fingerprint;
        if candidate.integrity == "partial" && streamed.prefix_bytes < streamed.bytes {
            candidate.integrity = "valid".into();
        }
        match storage::protect_file(paths, &candidate, streamed.prefix_bytes) {
            Ok(snapshot) => {
                thread.protected_bytes = Some(snapshot.bytes);
                thread.snapshot = if !snapshot.recoverable {
                    SnapshotState::Failed
                } else if streamed.prefix_bytes == streamed.bytes {
                    SnapshotState::Protected
                } else {
                    SnapshotState::Pending
                };
            }
            Err(error) => {
                thread.snapshot = SnapshotState::Failed;
                if errors.len() < 8 && !errors.contains(&error) {
                    errors.push(error);
                }
            }
        }
    } else if enabled {
        thread.snapshot = SnapshotState::Pending;
    } else {
        thread.snapshot = snapshot_state(paths, thread);
    }
    thread.last_verified_at = Some(at.to_owned());
    if !same_source_version(thread, path) {
        thread.integrity = "changed".into();
        thread.snapshot = SnapshotState::Pending;
    }
    Ok(())
}

fn snapshot_state(paths: &AppPaths, thread: &ThreadSummary) -> SnapshotState {
    match storage::snapshot(paths, thread) {
        Ok(Some(snapshot))
            if snapshot.hash == thread.fingerprint
                && snapshot.recoverable
                && thread.integrity == "valid" =>
        {
            SnapshotState::Protected
        }
        Ok(Some(snapshot)) if !snapshot.recoverable => SnapshotState::Failed,
        Ok(_) => SnapshotState::Pending,
        Err(_) => SnapshotState::Failed,
    }
}

fn unchanged_verified(thread: &ThreadSummary, path: &Path) -> bool {
    thread.integrity == "valid"
        && thread.snapshot == SnapshotState::Protected
        && thread
            .last_verified_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .is_some_and(|at| {
                let age = Utc::now()
                    .signed_duration_since(at.with_timezone(&Utc))
                    .num_seconds();
                (0..86_400).contains(&age)
            })
        && same_source_version(thread, path)
}

fn same_source_version(thread: &ThreadSummary, path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() == thread.bytes)
        && files::modified_at(path) == thread.updated_at
}

fn same_rollout_moved(previous: &ThreadSummary, current: &ThreadSummary) -> bool {
    if previous.source_id != current.source_id
        || previous.thread_id != current.thread_id
        || current.integrity != "valid"
    {
        return false;
    }
    let previous_name = Path::new(&previous.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim_end_matches(".zst");
    let current_name = Path::new(&current.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim_end_matches(".zst");
    previous_name == current_name && !Path::new(&previous.path).exists()
}

pub(super) fn detail(paths: &AppPaths, thread: &ThreadSummary) -> Result<ThreadDetail, String> {
    let source_root = root_for_summary(thread)?;
    let source_path = Path::new(&thread.path);
    let raw_available = files::validate_path(&source_root, source_path).is_ok();
    let snapshot = storage::snapshot(paths, thread)?;
    let preview = if raw_available {
        parsing::preview_file(&source_root, source_path)
    } else if snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.bytes <= files::MAX_BYTES)
    {
        storage::snapshot_bytes(paths, thread)?
            .as_ref()
            .map(|bytes| parsing::parse(source_path, bytes, true).preview)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(ThreadDetail {
        summary: thread.clone(),
        preview,
        snapshot_bytes: snapshot.as_ref().map(|snapshot| snapshot.bytes),
        snapshot_hash: snapshot.map(|snapshot| snapshot.hash),
        raw_available,
    })
}

fn root_for_summary(thread: &ThreadSummary) -> Result<PathBuf, String> {
    let relative = Path::new(&thread.relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("线程路径信息无效，请重新扫描。".into());
    }
    if !matches!(
        relative
            .components()
            .next()
            .and_then(|part| part.as_os_str().to_str()),
        Some("sessions" | "archived_sessions")
    ) {
        return Err("线程路径不属于会话目录。".into());
    }
    let path = Path::new(&thread.path);
    if !path.is_absolute() || !path.ends_with(relative) {
        return Err("线程路径信息不一致，请重新扫描。".into());
    }
    let mut root = path.to_path_buf();
    for _ in relative.components() {
        root.pop();
    }
    if digest(files::normalized_path(&root).as_bytes()) != thread.source_id {
        return Err("线程来源信息不一致，请重新扫描。".into());
    }
    Ok(root)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
