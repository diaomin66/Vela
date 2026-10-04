use super::{inventory, paths, restore, vault};
use crate::{core::AppPaths, threads::types::*};
use std::{fs, path::Path};

pub(super) fn fixture(root: &Path) -> (AppPaths, ThreadSource, ThreadSummary, Vec<u8>) {
    let source = root.join("home");
    fs::create_dir_all(&source).unwrap();
    // Production discovery resolves the existing source before deriving its
    // identity. Match it when TEMP contains an 8.3 Windows path alias.
    let source = fs::canonicalize(&source).unwrap();
    let paths = AppPaths {
        data: root.join("data"),
        config: source.join("config.toml"),
        helper: root.join("helper.exe"),
        locations: None,
    };
    let id = uuid::Uuid::new_v4().to_string();
    let relative = format!("sessions/2026/10/04/rollout-2026-10-04T00-00-00-{id}.jsonl");
    let path = source.join(&relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = format!("{{\"timestamp\":\"2026-10-04T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"D:/fixture\"}}}}\n").into_bytes();
    fs::write(&path, &bytes).unwrap();
    let source_id = paths::hash(paths::normalized(&source).unwrap().as_bytes());
    let source = ThreadSource {
        id: source_id.clone(),
        kind: "local".into(),
        root: source.to_string_lossy().into_owned(),
        sqlite_home: None,
        display_root: "fixture".into(),
        available: true,
        writable: true,
        last_scanned_at: None,
        error: None,
    };
    let thread = ThreadSummary {
        key: paths::hash(format!("{source_id}\0{relative}").as_bytes()),
        source_id,
        thread_id: id,
        path: path.to_string_lossy().into_owned(),
        relative_path: relative,
        archived: false,
        title: Some("Fixture title".into()),
        cwd: Some("D:/fixture".into()),
        provider: Some("fixture".into()),
        source_kind: None,
        history_base: None,
        created_at: None,
        updated_at: Some("2026-10-04T00:00:00Z".into()),
        bytes: bytes.len() as u64,
        line_count: 1,
        index_present: false,
        state_index: None,
        selected_rollout: None,
        integrity: "valid".into(),
        snapshot: SnapshotState::Pending,
        recoverability: "source-present".into(),
        fingerprint: paths::hash(&bytes),
        scan_revision: "fixture-revision".into(),
        last_verified_at: None,
        protected_bytes: None,
    };
    (paths, source, thread, bytes)
}

pub(super) fn save(paths: &AppPaths, source: ThreadSource, thread: ThreadSummary) {
    inventory::write(
        paths,
        &ThreadIndex {
            sources: vec![source],
            threads: vec![thread],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
}

#[test]
fn sqlite_replaces_inventory_without_deleting_snapshot_history() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    inventory::write(&paths, &ThreadIndex::default()).unwrap();
    let restored = inventory::read(&paths).unwrap();
    assert!(restored.threads.is_empty());
    assert_eq!(restored.sources.len(), 1);
    assert_eq!(
        vault::snapshot(&paths, &thread).unwrap().unwrap().hash,
        paths::hash(&bytes)
    );
}

#[test]
fn chunks_are_encrypted_deduplicated_and_verified_before_restore() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, _) = fixture(directory.path());
    let mut bytes = vec![b'a'; 2 * 1024 * 1024];
    bytes.push(b'\n');
    fs::write(&thread.path, &bytes).unwrap();
    thread.bytes = bytes.len() as u64;
    thread.fingerprint = paths::hash(&bytes);
    save(&paths, source, thread.clone());
    let first = vault::protect(&paths, &thread, &bytes).unwrap();
    let manifest = vault::selected(&paths, &thread).unwrap().unwrap();
    assert_eq!(first.bytes, bytes.len() as u64);
    assert_eq!(first.hash, thread.fingerprint);
    let object_root = paths::directory(&paths)
        .join("objects")
        .join(&thread.source_id);
    let objects: Vec<_> = fs::read_dir(&object_root)
        .unwrap()
        .flat_map(|entry| {
            fs::read_dir(entry.unwrap().path())
                .unwrap()
                .map(Result::unwrap)
        })
        .collect();
    assert_eq!(objects.len(), 2);
    assert!(objects.iter().all(|entry| !fs::read(entry.path())
        .unwrap()
        .windows(64)
        .any(|bytes| bytes.iter().all(|byte| *byte == b'a'))));
    assert_eq!(
        vault::snapshot_bytes(&paths, &thread).unwrap().unwrap(),
        bytes
    );
    assert_eq!(
        vault::protect(&paths, &thread, &bytes).unwrap().hash,
        first.hash
    );
    fs::write(objects[0].path(), b"damaged").unwrap();
    assert!(vault::verify(&paths, &manifest).is_err());
    assert!(restore::preview_restore(&paths, &thread).is_err());
}

#[test]
fn missing_file_restores_exact_bytes_without_overwriting_conflicts() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    fs::remove_file(&thread.path).unwrap();
    let preview = restore::preview_restore(&paths, &thread).unwrap();
    assert!(!preview.target_exists);
    restore::apply_restore(&paths, &thread, &preview.expected_hash).unwrap();
    assert_eq!(fs::read(&thread.path).unwrap(), bytes);
    fs::write(&thread.path, b"different content\n").unwrap();
    let conflict = restore::preview_restore(&paths, &thread).unwrap();
    assert!(conflict.conflict);
    assert!(restore::apply_restore(&paths, &thread, &conflict.expected_hash).is_err());
    assert_eq!(fs::read(&thread.path).unwrap(), b"different content\n");
}

#[test]
fn target_created_after_preview_is_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    fs::remove_file(&thread.path).unwrap();
    let preview = restore::preview_restore(&paths, &thread).unwrap();
    fs::write(&thread.path, b"created after preview\n").unwrap();
    assert!(
        restore::apply_restore(&paths, &thread, &preview.expected_hash)
            .unwrap_err()
            .contains("预览")
    );
    assert_eq!(fs::read(&thread.path).unwrap(), b"created after preview\n");
}

#[test]
fn unknown_and_unstable_snapshots_never_restore_automatically() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, bytes) = fixture(directory.path());
    thread.integrity = "unrecognized".into();
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    assert!(restore::preview_restore(&paths, &thread).is_err());
    let unfinished = b"unfinished";
    thread.fingerprint = paths::hash(unfinished);
    assert!(vault::protect(&paths, &thread, unfinished).is_err());
}

#[test]
fn latest_valid_snapshot_survives_a_damaged_later_source_and_index_loss() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    let bad = b"not json\n";
    thread.integrity = "partial".into();
    thread.fingerprint = paths::hash(bad);
    thread.bytes = bad.len() as u64;
    fs::write(&thread.path, bad).unwrap();
    vault::protect(&paths, &thread, bad).unwrap();
    assert_eq!(
        vault::snapshot(&paths, &thread).unwrap().unwrap().hash,
        paths::hash(&bytes)
    );
    let db = paths::directory(&paths).join("inventory.sqlite3");
    fs::write(&db, b"corrupt inventory").unwrap();
    assert!(inventory::read(&paths).is_err());
    let rebuilt = inventory::rebuild(&paths).unwrap();
    assert_eq!(rebuilt.threads.len(), 1);
    assert_eq!(rebuilt.threads[0].thread_id, thread.thread_id);
    assert_eq!(
        vault::snapshot_bytes(&paths, &thread).unwrap().unwrap(),
        bytes
    );
    assert!(fs::read_dir(paths::directory(&paths))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("inventory-preserved-")));
}

#[test]
fn identical_thread_ids_in_distinct_sources_never_share_objects_or_targets() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    let mut forged = thread.clone();
    forged.source_id = paths::hash(b"other-source");
    assert!(vault::snapshot(&paths, &forged).unwrap().is_none());
    forged = thread.clone();
    forged.relative_path = "sessions/../../../escaped.jsonl".into();
    assert!(vault::protect(&paths, &forged, &bytes).is_err());
}

#[test]
fn paginated_search_is_literal_and_dashboard_does_not_materialize_rows() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, _) = fixture(directory.path());
    let mut index = ThreadIndex {
        sources: vec![source],
        ..ThreadIndex::default()
    };
    for number in 0..125 {
        let mut row = thread.clone();
        row.key = format!("key-{number:03}");
        row.relative_path = format!("sessions/rollout-{number}.jsonl");
        row.title = Some(if number == 72 {
            "配额 100% 完成".into()
        } else {
            format!("线程 {number}")
        });
        row.archived = number % 2 == 0;
        row.snapshot = if number % 3 == 0 {
            SnapshotState::Protected
        } else {
            SnapshotState::Pending
        };
        index.threads.push(row);
    }
    inventory::write(&paths, &index).unwrap();
    let overview = inventory::overview(&paths).unwrap();
    assert_eq!(overview.total, 125);
    assert_eq!(overview.protected, 42);
    assert!(overview.threads.is_empty());
    assert!(inventory::read_state(&paths).unwrap().threads.is_empty());
    assert_eq!(
        inventory::lookup(&paths, "key-072")
            .unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("配额 100% 完成")
    );
    let page = inventory::page(
        &paths,
        &ThreadListQuery {
            offset: 100,
            limit: 20,
            ..ThreadListQuery::default()
        },
    )
    .unwrap();
    assert_eq!(page.total, 125);
    assert_eq!(page.threads.len(), 20);
    let exact = inventory::page(
        &paths,
        &ThreadListQuery {
            search: "100%".into(),
            ..ThreadListQuery::default()
        },
    )
    .unwrap();
    assert_eq!(exact.total, 1);
    let archived = inventory::page(
        &paths,
        &ThreadListQuery {
            scope: "archived".into(),
            ..ThreadListQuery::default()
        },
    )
    .unwrap();
    assert_eq!(archived.total, 63);
    assert!(inventory::page(
        &paths,
        &ThreadListQuery {
            limit: 101,
            ..ThreadListQuery::default()
        }
    )
    .is_err());
}

#[test]
fn startup_imports_protected_content_if_crash_preceded_catalog_commit() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
    vault::protect(&paths, &thread, &bytes).unwrap();
    fs::remove_file(&thread.path).unwrap();
    inventory::recover(&paths).unwrap();
    let recovered = inventory::read(&paths).unwrap();
    assert_eq!(recovered.threads.len(), 1);
    assert_eq!(recovered.threads[0].recoverability, "recoverable");
    assert_eq!(
        vault::snapshot_bytes(&paths, &recovered.threads[0])
            .unwrap()
            .unwrap(),
        bytes
    );
    inventory::recover(&paths).unwrap();
    assert_eq!(inventory::read(&paths).unwrap().threads.len(), 1);
}

#[test]
fn large_snapshot_streams_complete_prefix_and_rejects_a_changed_fingerprint() {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, _) = fixture(directory.path());
    let mut file = fs::File::create(&thread.path).unwrap();
    let mut block = vec![b'x'; 1024 * 1024];
    *block.last_mut().unwrap() = b'\n';
    let mut digest = Sha256::new();
    for _ in 0..65 {
        file.write_all(&block).unwrap();
        digest.update(&block);
    }
    let complete_bytes = 65 * 1024 * 1024;
    file.write_all(b"unfinished tail").unwrap();
    file.sync_all().unwrap();
    drop(file);
    thread.bytes = complete_bytes;
    thread.fingerprint = format!("{:x}", digest.finalize());
    save(&paths, source, thread.clone());
    let snapshot = vault::protect_file(&paths, &thread, complete_bytes).unwrap();
    assert_eq!(snapshot.bytes, complete_bytes);
    assert_eq!(snapshot.hash, thread.fingerprint);
    assert!(vault::snapshot_bytes(&paths, &thread).is_err());
    let mut bad = thread.clone();
    bad.fingerprint = paths::hash(b"incorrect");
    assert!(vault::protect_file(&paths, &bad, complete_bytes).is_err());
}

fn journal(paths: &AppPaths) -> (std::path::PathBuf, serde_json::Value) {
    let path = fs::read_dir(paths::directory(paths).join("restores"))
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("bin"))
        .unwrap()
        .path();
    let encrypted = fs::read(&path).unwrap();
    let raw = crate::security::unprotect(&encrypted).unwrap();
    (path, serde_json::from_slice(&raw).unwrap())
}

fn save_journal(path: &Path, value: &serde_json::Value) {
    let raw = serde_json::to_vec(value).unwrap();
    crate::core::atomic_write(path, &crate::security::protect(&raw).unwrap()).unwrap();
}

#[test]
fn interrupted_restore_audit_retains_stage_and_requires_explicit_retry() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    fs::remove_file(&thread.path).unwrap();
    let preview = restore::preview_restore(&paths, &thread).unwrap();
    restore::apply_restore(&paths, &thread, &preview.expected_hash).unwrap();
    let (journal_path, mut record) = journal(&paths);
    record["state"] = "prepared".into();
    let staging = std::path::PathBuf::from(record["staging"].as_str().unwrap());
    fs::write(&staging, &bytes).unwrap();
    fs::remove_file(&thread.path).unwrap();
    save_journal(&journal_path, &record);
    assert!(restore::audit(&paths).unwrap_err().contains("未完成"));
    assert!(!Path::new(&thread.path).exists());
    assert_eq!(fs::read(&staging).unwrap(), bytes);
    let raw = crate::security::unprotect(&fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&raw).unwrap()["state"],
        "interrupted"
    );
    let retry = restore::preview_restore(&paths, &thread).unwrap();
    restore::apply_restore(&paths, &thread, &retry.expected_hash).unwrap();
    assert_eq!(fs::read(&thread.path).unwrap(), bytes);
    assert_eq!(fs::read(&staging).unwrap(), bytes);
}

#[test]
fn published_restore_audit_checks_hash_and_corrupt_journal_is_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    fs::remove_file(&thread.path).unwrap();
    let preview = restore::preview_restore(&paths, &thread).unwrap();
    restore::apply_restore(&paths, &thread, &preview.expected_hash).unwrap();
    let (path, mut record) = journal(&paths);
    record["state"] = "published".into();
    save_journal(&path, &record);
    restore::audit(&paths).unwrap();
    let raw = crate::security::unprotect(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&raw).unwrap()["state"],
        "verified"
    );
    fs::write(&path, b"corrupt journal").unwrap();
    assert!(restore::audit(&paths).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"corrupt journal");
    assert_eq!(fs::read(&thread.path).unwrap(), bytes);
}

#[test]
fn format_recognition_updates_metadata_without_replacing_old_manifests() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    thread.integrity = "unrecognized".into();
    vault::protect(&paths, &thread, &bytes).unwrap();
    assert!(restore::preview_restore(&paths, &thread).is_err());
    thread.integrity = "valid".into();
    vault::protect(&paths, &thread, &bytes).unwrap();
    assert!(restore::preview_restore(&paths, &thread).is_ok());
    let folder = paths::directory(&paths)
        .join("manifests")
        .join(&thread.source_id)
        .join(paths::hash(thread.relative_path.as_bytes()));
    assert_eq!(fs::read_dir(folder).unwrap().count(), 2);
}

fn damage_first_chunk(paths: &AppPaths, thread: &ThreadSummary) -> std::path::PathBuf {
    let manifest = vault::selected(paths, thread).unwrap().unwrap();
    let value = serde_json::to_value(&manifest).unwrap();
    let hash = value["chunks"][0]["hash"].as_str().unwrap();
    let path = paths::directory(paths)
        .join("objects")
        .join(&thread.source_id)
        .join(&hash[..2])
        .join(format!("{hash}.bin"));
    fs::write(&path, b"damaged encrypted object").unwrap();
    path
}

#[test]
fn damaged_newer_snapshot_falls_back_with_an_explicit_preview_warning() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    let mut newer = bytes.clone();
    newer.extend_from_slice(b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"new turn\"}}\n");
    thread.fingerprint = paths::hash(&newer);
    thread.bytes = newer.len() as u64;
    fs::write(&thread.path, &newer).unwrap();
    vault::protect(&paths, &thread, &newer).unwrap();
    let damaged = damage_first_chunk(&paths, &thread);
    fs::remove_file(&thread.path).unwrap();
    let preview = restore::preview_restore(&paths, &thread).unwrap();
    assert_eq!(preview.snapshot_hash, paths::hash(&bytes));
    assert!(preview.warning.as_deref().unwrap().contains("较早"));
    restore::apply_restore(&paths, &thread, &preview.expected_hash).unwrap();
    assert_eq!(fs::read(&thread.path).unwrap(), bytes);
    assert_eq!(fs::read(damaged).unwrap(), b"damaged encrypted object");
}

#[test]
fn corrupt_database_and_one_bad_snapshot_do_not_hide_other_healthy_threads() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, first, first_bytes) = fixture(directory.path());
    let (_, _, second, second_bytes) = fixture(directory.path());
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source],
            threads: vec![first.clone(), second.clone()],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
    vault::protect(&paths, &first, &first_bytes).unwrap();
    vault::protect(&paths, &second, &second_bytes).unwrap();
    let damaged = damage_first_chunk(&paths, &first);
    fs::write(
        paths::directory(&paths).join("inventory.sqlite3"),
        b"broken database",
    )
    .unwrap();
    let recovered = inventory::rebuild(&paths).unwrap();
    assert_eq!(recovered.threads.len(), 2);
    assert_eq!(
        recovered
            .threads
            .iter()
            .find(|thread| thread.key == first.key)
            .unwrap()
            .snapshot,
        SnapshotState::Failed
    );
    assert_eq!(
        recovered
            .threads
            .iter()
            .find(|thread| thread.key == second.key)
            .unwrap()
            .snapshot,
        SnapshotState::Protected
    );
    assert!(recovered.protection.error.unwrap().contains("无法校验"));
    assert_eq!(
        vault::snapshot_bytes(&paths, &second).unwrap().unwrap(),
        second_bytes
    );
    assert_eq!(fs::read(damaged).unwrap(), b"damaged encrypted object");
}

#[test]
fn corrupt_restore_journal_is_reported_without_blocking_healthy_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, thread, bytes) = fixture(directory.path());
    save(&paths, source, thread.clone());
    vault::protect(&paths, &thread, &bytes).unwrap();
    let folder = paths::directory(&paths).join("restores");
    fs::create_dir_all(&folder).unwrap();
    let damaged = folder.join(format!("{}.bin", uuid::Uuid::new_v4()));
    fs::write(&damaged, b"bad journal").unwrap();
    inventory::recover(&paths).unwrap();
    assert_eq!(inventory::overview(&paths).unwrap().total, 1);
    assert!(inventory::read(&paths).unwrap().protection.error.is_some());
    assert_eq!(fs::read(damaged).unwrap(), b"bad journal");
}

#[test]
fn committed_partial_prefix_is_not_reimported_on_startup() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, bytes) = fixture(directory.path());
    let mut raw = bytes.clone();
    raw.extend_from_slice(b"{\"unfinished\":");
    fs::write(&thread.path, &raw).unwrap();
    let snapshot = vault::protect(&paths, &thread, &bytes).unwrap();
    thread.bytes = raw.len() as u64;
    thread.fingerprint = paths::hash(&raw);
    thread.integrity = "partial".into();
    thread.snapshot = SnapshotState::Pending;
    thread.protected_bytes = Some(snapshot.bytes);
    thread.last_verified_at = Some("2026-01-01T00:00:00Z".into());
    save(&paths, source, thread.clone());
    inventory::recover(&paths).unwrap();
    let actual = inventory::lookup(&paths, &thread.key).unwrap().unwrap();
    assert_eq!(actual.integrity, "partial");
    assert_eq!(actual.snapshot, SnapshotState::Pending);
    assert_eq!(actual.bytes, raw.len() as u64);
    assert_eq!(actual.fingerprint, paths::hash(&raw));
    assert_eq!(fs::read(&thread.path).unwrap(), raw);
}

#[test]
fn protected_thread_missing_native_index_is_in_attention_counts_and_page() {
    let directory = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, _) = fixture(directory.path());
    thread.snapshot = SnapshotState::Protected;
    thread.state_index = Some("missing".into());
    save(&paths, source, thread);
    assert_eq!(inventory::overview(&paths).unwrap().attention, 1);
    assert_eq!(
        inventory::page(
            &paths,
            &ThreadListQuery {
                status: "attention".into(),
                ..ThreadListQuery::default()
            }
        )
        .unwrap()
        .total,
        1
    );
}
