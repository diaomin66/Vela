use super::super::{
    inventory, paths,
    tests::{fixture, save},
    vault,
};
use super::{
    journal::{self, Record},
    plan,
};
use crate::threads::{scan, types::*};
use std::{fs, path::Path};

fn protected(root: &Path) -> (crate::core::AppPaths, ThreadSource, ThreadSummary, Vec<u8>) {
    let (paths, source, mut thread, bytes) = fixture(root);
    vault::protect(&paths, &thread, &bytes).unwrap();
    thread.snapshot = SnapshotState::Protected;
    thread.protected_bytes = Some(thread.bytes);
    save(&paths, source.clone(), thread.clone());
    let db = rusqlite::Connection::open(Path::new(&source.root).join("state_5.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TABLE thread_spawn_edges(parent_thread_id TEXT, child_thread_id TEXT);",
    )
    .unwrap();
    (paths, source, thread, bytes)
}

fn record(thread: ThreadSummary, state: &str) -> Record {
    Record {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        deleted_at: chrono::Utc::now().to_rfc3339(),
        state: state.into(),
        message: None,
        items: vec![thread],
    }
}

#[test]
fn selecting_one_row_expands_all_rollouts_of_the_same_logical_thread() {
    let root = tempfile::tempdir().unwrap();
    let (_paths, _source, first, _) = fixture(root.path());
    let mut second = first.clone();
    second.key = "second".into();
    let mut other_source = first.clone();
    other_source.key = "other-source".into();
    other_source.source_id = "another".into();
    let index = ThreadIndex {
        threads: vec![first.clone(), second, other_source],
        ..Default::default()
    };
    let selected = plan::groups(&index, &[first.key]).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].len(), 2);
    assert!(plan::groups(&index, &["stale".into()]).is_err());
}

#[test]
fn source_present_protected_thread_is_deletable_and_changed_bytes_are_blocked() {
    let root = tempfile::tempdir().unwrap();
    let (paths, source, thread, _) = protected(root.path());
    assert_eq!(thread.recoverability, "source-present");
    assert!(
        plan::preview_delete(&paths, vec![thread.key.clone()])
            .unwrap()
            .items[0]
            .can_delete
    );
    fs::write(&thread.path, b"changed\n").unwrap();
    assert!(plan::protect_sources(&paths, &source, &[thread]).is_err());
}

#[test]
fn source_rollout_set_changes_invalidate_deletion() {
    let root = tempfile::tempdir().unwrap();
    let (_paths, source, thread, bytes) = protected(root.path());
    let second = Path::new(&source.root)
        .join("archived_sessions")
        .join(format!(
            "rollout-2026-10-04T00-00-00-{}_{}.jsonl",
            thread.thread_id,
            uuid::Uuid::new_v4()
        ));
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    fs::write(second, bytes).unwrap();
    assert!(plan::verify_scope(&source, &[thread]).is_err());
}

#[test]
fn immutable_rollout_references_and_spawn_descendants_block_delete() {
    let root = tempfile::tempdir().unwrap();
    let (paths, source, mut parent, _) = protected(root.path());
    let rollout = uuid::Uuid::new_v4().to_string();
    parent.path = Path::new(&source.root)
        .join("sessions")
        .join(format!(
            "rollout-2026-10-04T00-00-00-{}_{}.jsonl",
            parent.thread_id, rollout
        ))
        .to_string_lossy()
        .into_owned();
    let mut child = parent.clone();
    child.thread_id = uuid::Uuid::new_v4().to_string();
    child.key = "child".into();
    child.history_base = Some(ThreadHistoryBase {
        thread_id: rollout,
        end_ordinal_exclusive: 1,
        end_byte_offset: None,
    });
    let index = ThreadIndex {
        threads: vec![parent.clone(), child],
        ..Default::default()
    };
    assert!(plan::dependency_reason(&index, &[parent.clone()]).is_some());
    let db = rusqlite::Connection::open(Path::new(&source.root).join("state_5.sqlite")).unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges VALUES(?1,?2)",
        [&parent.thread_id, &uuid::Uuid::new_v4().to_string()],
    )
    .unwrap();
    assert!(plan::verify_graph(&paths, &source, &parent.thread_id).is_err());
}

#[test]
fn newer_official_database_cannot_reuse_an_older_empty_spawn_graph() {
    let root = tempfile::tempdir().unwrap();
    let (paths, source, thread, _) = protected(root.path());
    assert!(plan::verify_graph(&paths, &source, &thread.thread_id).is_ok());
    fs::write(
        Path::new(&source.root).join("state_6.sqlite"),
        b"future schema",
    )
    .unwrap();
    assert!(plan::verify_graph(&paths, &source, &thread.thread_id)
        .unwrap_err()
        .contains("更新版本"));
}

#[test]
fn deletion_preview_binds_source_root_and_sqlite_mapping() {
    let root = tempfile::tempdir().unwrap();
    let (paths, _, thread, _) = protected(root.path());
    let mut index = inventory::read(&paths).unwrap();
    let groups = plan::groups(&index, &[thread.key]).unwrap();
    let original = plan::token(&index, &groups).unwrap();
    index.sources[0].sqlite_home = Some(
        root.path()
            .join("other-index")
            .to_string_lossy()
            .into_owned(),
    );
    let remapped = plan::token(&index, &groups).unwrap();
    assert_ne!(original, remapped);
    index.sources[0].root = root
        .path()
        .join("other-home")
        .to_string_lossy()
        .into_owned();
    assert_ne!(remapped, plan::token(&index, &groups).unwrap());
}

#[test]
fn interrupted_tombstones_survive_scan_manifest_rebuild_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let (paths, _source, thread, _) = protected(root.path());
    let record = record(thread.clone(), "prepared");
    let mut store = journal::read(&paths).unwrap();
    journal::update(&paths, &mut store, &record).unwrap();
    fs::remove_file(&thread.path).unwrap();
    journal::audit(&paths).unwrap();
    assert_eq!(
        journal::list_trash(&paths, 0, 50).unwrap().items[0].state,
        "interrupted"
    );
    assert!(scan::run(&paths).unwrap().threads.is_empty());
    assert!(inventory::rebuild(&paths).unwrap().threads.is_empty());
    inventory::recover(&paths).unwrap();
    assert!(inventory::read(&paths).unwrap().threads.is_empty());
    assert!(vault::snapshot(&paths, &thread).unwrap().is_some());
}

#[test]
fn damaged_tombstones_fail_closed_and_never_recreate_threads() {
    let root = tempfile::tempdir().unwrap();
    let (paths, _, _, _) = protected(root.path());
    fs::write(
        paths::directory(&paths).join("trash.bin"),
        b"corrupt encrypted journal",
    )
    .unwrap();
    assert!(journal::tombstones(&paths).is_err());
    assert!(scan::run(&paths).is_err());
    assert!(inventory::rebuild(&paths).is_err());
    assert!(inventory::recover(&paths).is_err());
    assert!(inventory::lookup(&paths, "stale-key").is_err());
    assert!(inventory::overview(&paths).is_err());
    assert!(inventory::page(&paths, &ThreadListQuery::default()).is_err());
}

#[test]
fn stale_sqlite_rows_do_not_bypass_tombstones_before_inventory_commit() {
    let root = tempfile::tempdir().unwrap();
    let (paths, _, thread, _) = protected(root.path());
    let record = record(thread.clone(), "prepared");
    let mut store = journal::read(&paths).unwrap();
    journal::update(&paths, &mut store, &record).unwrap();
    assert!(inventory::lookup(&paths, &thread.key).unwrap().is_none());
    assert_eq!(inventory::overview(&paths).unwrap().total, 0);
    let page = inventory::page(&paths, &ThreadListQuery::default()).unwrap();
    assert_eq!(page.total, 0);
    assert!(page.threads.is_empty());
}

#[test]
fn partial_delete_crash_keeps_remaining_rollout_and_undo_restores_both_exactly() {
    let root = tempfile::tempdir().unwrap();
    let (paths, source, first, bytes) = protected(root.path());
    let mut second = first.clone();
    second.relative_path = format!(
        "archived_sessions/rollout-2026-10-04T00-00-00-{}_{}.jsonl",
        first.thread_id,
        uuid::Uuid::new_v4()
    );
    second.path = Path::new(&source.root)
        .join(&second.relative_path)
        .to_string_lossy()
        .into_owned();
    second.key = paths::hash(format!("{}\0{}", source.id, second.relative_path).as_bytes());
    second.archived = true;
    fs::create_dir_all(Path::new(&second.path).parent().unwrap()).unwrap();
    fs::write(&second.path, &bytes).unwrap();
    vault::protect(&paths, &second, &bytes).unwrap();
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source],
            threads: vec![first.clone(), second.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    let mut transaction = record(first.clone(), "prepared");
    transaction.items.push(second.clone());
    let mut store = journal::read(&paths).unwrap();
    journal::update(&paths, &mut store, &transaction).unwrap();
    fs::remove_file(&first.path).unwrap(); // simulate RPC failure after only its first unlink
    journal::audit(&paths).unwrap();
    assert_eq!(fs::read(&second.path).unwrap(), bytes);
    assert_eq!(
        inventory::page(&paths, &ThreadListQuery::default())
            .unwrap()
            .total,
        0
    );
    let preview = super::preview_trash_restore(&paths, transaction.id).unwrap();
    assert!(preview.can_restore, "{:?}", preview.reason);
    let result =
        super::actions::restore_with_refresh(&paths, preview.id, preview.expected_hash, |_, _| {
            Err("simulated unavailable official client".into())
        })
        .unwrap();
    assert_eq!(result.deleted_count, 2);
    assert_eq!(result.failed_count, 0);
    assert!(result
        .items
        .iter()
        .all(|item| item.status == "restored" && item.message.contains("尚未刷新")));
    assert_eq!(fs::read(&first.path).unwrap(), bytes);
    assert_eq!(fs::read(&second.path).unwrap(), bytes);
    assert!(journal::tombstones(&paths).unwrap().is_empty());
    assert_eq!(scan::run(&paths).unwrap().threads.len(), 2);
}

#[test]
fn paginated_history_requires_a_verified_native_projection_restore_path() {
    let root = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, _) = protected(root.path());
    let bytes = format!("{{\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{{\"id\":\"{}\",\"history_mode\":\"paginated\"}}}}\n", thread.thread_id).into_bytes();
    fs::write(&thread.path, &bytes).unwrap();
    thread.bytes = bytes.len() as u64;
    thread.fingerprint = paths::hash(&bytes);
    vault::protect(&paths, &thread, &bytes).unwrap();
    save(&paths, source, thread.clone());
    let preview = plan::preview_delete(&paths, vec![thread.key]).unwrap();
    assert!(!preview.items[0].can_delete);
    assert!(preview.items[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("分页消息历史"));
}

#[test]
#[ignore = "requires AHAX_TEST_APP_SERVER pointing to explicitly selected official executable"]
fn official_delete_removes_all_rollouts_and_undo_restores_native_history() {
    use crate::threads::{client::Client, ThreadState};
    use serde_json::json;
    let executable = std::path::PathBuf::from(
        std::env::var_os("AHAX_TEST_APP_SERVER").expect("set explicit official test executable"),
    );
    let sandbox = tempfile::tempdir().unwrap();
    let (paths, source, mut thread, _) = fixture(sandbox.path());
    let root = Path::new(&source.root);
    fs::write(&paths.config, "[analytics]\nenabled=false\n").unwrap();
    let marker = "AhaX isolated deletion historical message";
    let meta = json!({"timestamp":"2026-10-04T09:00:00Z","type":"session_meta","payload":{"id":thread.thread_id,"timestamp":"2026-10-04T09:00:00Z","cwd":root,"originator":"codex_cli_rs","cli_version":"0.160.0","source":"cli","model_provider":"openai"}});
    let event = json!({"timestamp":"2026-10-04T09:00:01Z","type":"event_msg","payload":{"type":"user_message","message":marker,"images":[],"local_images":[],"text_elements":[]}});
    let bytes = format!("{meta}\n{event}\n").into_bytes();
    fs::write(&thread.path, &bytes).unwrap();
    thread.bytes = bytes.len() as u64;
    thread.fingerprint = paths::hash(&bytes);
    let archive = root.join("archived_sessions").join(format!(
        "rollout-2026-10-04T09-00-00-{}_{}.jsonl",
        thread.thread_id,
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(archive.parent().unwrap()).unwrap();
    fs::write(&archive, &bytes).unwrap();
    let mut rpc = Client::start(&executable, root, Some(root)).unwrap();
    let before = rpc
        .call(
            "thread/read",
            json!({"threadId":thread.thread_id,"includeTurns":true}),
        )
        .unwrap();
    assert!(before.to_string().contains(marker));
    drop(rpc);
    let state = ThreadState::new(paths.clone());
    let dashboard = state.start_scan().unwrap();
    let key = dashboard
        .threads
        .iter()
        .find(|item| {
            item.path
                .ends_with(&thread.relative_path.replace('/', "\\"))
                || item.relative_path == thread.relative_path
        })
        .unwrap()
        .key
        .clone();
    let preview = state.preview_delete(vec![key.clone()]).unwrap();
    assert_eq!(preview.rollout_count, 2);
    assert!(preview.items[0].can_delete, "{:?}", preview.items[0].reason);
    let deleted = state.delete(vec![key], preview.expected_hash).unwrap();
    assert_eq!(deleted.deleted_count, 1, "{:?}", deleted.items);
    assert!(!Path::new(&thread.path).exists());
    assert!(!archive.exists());
    assert_eq!(state.list(ThreadListQuery::default()).unwrap().total, 0);
    let trash = state.list_trash(0, 50).unwrap();
    assert_eq!(trash.total, 1);
    assert!(inventory::rebuild(&paths).unwrap().threads.is_empty());
    let restore = state
        .preview_trash_restore(trash.items[0].id.clone())
        .unwrap();
    assert!(restore.can_restore, "{:?}", restore.reason);
    let restored = state
        .restore_trash(restore.id, restore.expected_hash)
        .unwrap();
    assert_eq!(restored.failed_count, 0, "{:?}", restored.items);
    assert_eq!(fs::read(&thread.path).unwrap(), bytes);
    assert_eq!(fs::read(&archive).unwrap(), bytes);
    assert_eq!(state.list_trash(0, 50).unwrap().total, 0);
    let mut rpc = Client::start(&executable, root, Some(root)).unwrap();
    let after = rpc
        .call(
            "thread/read",
            json!({"threadId":thread.thread_id,"includeTurns":true}),
        )
        .unwrap();
    assert!(after.to_string().contains(marker));
    assert!(rpc.list_ids(false).unwrap().contains(&thread.thread_id));
}

#[test]
fn trash_preview_binds_target_changes_and_preserves_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let (paths, _, thread, bytes) = protected(root.path());
    let record = record(thread.clone(), "deleted");
    let mut store = journal::read(&paths).unwrap();
    journal::update(&paths, &mut store, &record).unwrap();
    fs::remove_file(&thread.path).unwrap();
    let preview = super::preview_trash_restore(&paths, record.id.clone()).unwrap();
    assert!(preview.can_restore);
    fs::write(&thread.path, &bytes).unwrap(); // equal bytes still change the consent-bound target state
    assert!(super::restore_trash(&paths, record.id.clone(), preview.expected_hash).is_err());
    fs::write(&thread.path, b"conflict\n").unwrap();
    assert!(
        !super::preview_trash_restore(&paths, record.id)
            .unwrap()
            .can_restore
    );
    assert_eq!(fs::read(&thread.path).unwrap(), b"conflict\n");
}

#[cfg(windows)]
#[test]
fn open_writer_is_refused_and_guard_denies_new_writes_but_allows_delete() {
    use std::fs::OpenOptions;
    let root = tempfile::tempdir().unwrap();
    let (paths, source, thread, _) = protected(root.path());
    let writer = OpenOptions::new().append(true).open(&thread.path).unwrap();
    assert!(plan::protect_sources(&paths, &source, &[thread.clone()]).is_err());
    drop(writer);
    let readers = plan::protect_sources(&paths, &source, &[thread.clone()]).unwrap();
    assert!(OpenOptions::new().append(true).open(&thread.path).is_err());
    fs::remove_file(&thread.path).unwrap();
    drop(readers);
    assert!(!Path::new(&thread.path).exists());
}
