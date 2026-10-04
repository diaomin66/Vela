use super::*;
use crate::threads::{
    storage::{restore, tests::fixture},
    types::ThreadIndex,
};
use serde_json::json;
use std::fs;

fn record(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn replace(thread: &mut ThreadSummary, bytes: &[u8]) {
    fs::write(&thread.path, bytes).unwrap();
    thread.bytes = bytes.len() as u64;
    thread.fingerprint = paths::hash(bytes);
}

fn parent_bytes(id: &str) -> Vec<u8> {
    let mut bytes = record(
        json!({"ordinal":0,"type":"session_meta","payload":{"id":id,"history_mode":"paginated"}}),
    );
    bytes.extend(record(json!({"ordinal":1,"type":"event_msg","payload":{"type":"user_message","message":"parent"}})));
    bytes
}

fn child_bytes(id: &str, parent: &str, ordinal: u64, offset: u64) -> Vec<u8> {
    record(
        json!({"ordinal":ordinal,"type":"session_meta","payload":{"id":id,"history_mode":"paginated","history_base":{"thread_id":parent,"end_ordinal_exclusive":ordinal,"end_byte_offset":offset}}}),
    )
}

fn pair(root: &Path) -> (AppPaths, ThreadSummary, ThreadSummary, Vec<u8>, Vec<u8>) {
    let (paths, source, mut parent, _) = fixture(root);
    let (_, _, mut child, _) = fixture(root);
    let parent_raw = parent_bytes(&parent.thread_id);
    let child_raw = child_bytes(
        &child.thread_id,
        &parent.thread_id,
        2,
        parent_raw.len() as u64,
    );
    replace(&mut parent, &parent_raw);
    replace(&mut child, &child_raw);
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source],
            threads: vec![parent.clone(), child.clone()],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
    vault::protect(&paths, &parent, &parent_raw).unwrap();
    vault::protect(&paths, &child, &child_raw).unwrap();
    (paths, parent, child, parent_raw, child_raw)
}

#[test]
fn parent_backup_alone_is_not_enough_then_ordered_recovery_succeeds() {
    let sandbox = tempfile::tempdir().unwrap();
    let (paths, parent, child, parent_raw, child_raw) = pair(sandbox.path());
    fs::remove_file(&parent.path).unwrap();
    fs::remove_file(&child.path).unwrap();
    // The encrypted bytes are authoritative even when cached metadata has no dependency.
    assert!(child.history_base.is_none());
    assert!(restore::preview_restore(&paths, &child)
        .unwrap_err()
        .contains("缺少历史依赖"));
    assert!(!Path::new(&child.path).exists());
    let preview = restore::preview_restore(&paths, &parent).unwrap();
    restore::apply_restore(&paths, &parent, &preview.expected_hash).unwrap();
    let preview = restore::preview_restore(&paths, &child).unwrap();
    restore::apply_restore(&paths, &child, &preview.expected_hash).unwrap();
    assert_eq!(fs::read(&parent.path).unwrap(), parent_raw);
    assert_eq!(fs::read(&child.path).unwrap(), child_raw);
}

#[test]
fn preview_token_covers_exact_parent_prefix_and_rejects_changed_content() {
    let sandbox = tempfile::tempdir().unwrap();
    let (paths, parent, child, parent_raw, _) = pair(sandbox.path());
    fs::remove_file(&child.path).unwrap();
    let preview = restore::preview_restore(&paths, &child).unwrap();
    let changed = String::from_utf8(parent_raw)
        .unwrap()
        .replace("parent", "mutate");
    fs::write(&parent.path, changed).unwrap();
    assert!(
        restore::apply_restore(&paths, &child, &preview.expected_hash)
            .unwrap_err()
            .contains("预览后发生变化")
    );
    assert!(!Path::new(&child.path).exists());
}

#[test]
fn immutable_rollout_suffix_and_compressed_byte_boundary_are_supported() {
    let sandbox = tempfile::tempdir().unwrap();
    let (paths, source, mut parent, _) = fixture(sandbox.path());
    let (_, _, mut child, _) = fixture(sandbox.path());
    let rollout_id = uuid::Uuid::new_v4().to_string();
    fs::remove_file(&parent.path).unwrap();
    fs::remove_file(&child.path).unwrap();
    parent.relative_path = parent
        .relative_path
        .replace(".jsonl", &format!("_{rollout_id}.jsonl.zst"));
    parent.path = Path::new(&source.root)
        .join(&parent.relative_path)
        .to_string_lossy()
        .into_owned();
    parent.key = paths::hash(parent.path.as_bytes());
    child.relative_path.push_str(".zst");
    child.path = Path::new(&source.root)
        .join(&child.relative_path)
        .to_string_lossy()
        .into_owned();
    child.key = paths::hash(child.path.as_bytes());
    let plain_parent = parent_bytes(&parent.thread_id);
    let plain_child = child_bytes(&child.thread_id, &rollout_id, 2, plain_parent.len() as u64);
    let packed_parent = zstd::stream::encode_all(plain_parent.as_slice(), 1).unwrap();
    let packed_child = zstd::stream::encode_all(plain_child.as_slice(), 1).unwrap();
    replace(&mut parent, &packed_parent);
    replace(&mut child, &packed_child);
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source],
            threads: vec![parent, child.clone()],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
    vault::protect(&paths, &child, &packed_child).unwrap();
    fs::remove_file(&child.path).unwrap();
    let preview = restore::preview_restore(&paths, &child).unwrap();
    restore::apply_restore(&paths, &child, &preview.expected_hash).unwrap();
    assert_eq!(fs::read(&child.path).unwrap(), packed_child);
}

#[test]
fn invalid_ordinal_and_byte_boundaries_are_rejected_but_inherited_empty_prefix_is_valid() {
    let sandbox = tempfile::tempdir().unwrap();
    let (_, _, mut parent, _) = fixture(sandbox.path());
    let raw = parent_bytes(&parent.thread_id);
    replace(&mut parent, &raw);
    let mut required = ThreadHistoryBase {
        thread_id: parent.thread_id.clone(),
        end_ordinal_exclusive: 3,
        end_byte_offset: None,
    };
    assert!(validate_history_prefix(&parent, &required)
        .unwrap_err()
        .contains("缺少所需记录"));
    required.end_ordinal_exclusive = 2;
    required.end_byte_offset = Some(raw.len() as u64 - 1);
    assert!(validate_history_prefix(&parent, &required)
        .unwrap_err()
        .contains("字节边界"));
    let inherited = child_bytes(&parent.thread_id, &uuid::Uuid::new_v4().to_string(), 7, 0);
    replace(&mut parent, &inherited);
    required.end_ordinal_exclusive = 7;
    required.end_byte_offset = Some(0);
    assert!(validate_history_prefix(&parent, &required).is_ok());
    required.end_ordinal_exclusive = 6;
    assert!(validate_history_prefix(&parent, &required).is_err());
    required.end_ordinal_exclusive = 0;
    assert!(validate_history_prefix(&parent, &required).is_err());
    required.end_ordinal_exclusive = 2;
    required.end_byte_offset = None;
    let legacy = String::from_utf8(raw)
        .unwrap()
        .replace("paginated", "legacy");
    replace(&mut parent, legacy.as_bytes());
    assert!(validate_history_prefix(&parent, &required)
        .unwrap_err()
        .contains("不是分页记录"));
}

#[test]
fn dependency_cycles_and_cross_source_substitutions_cannot_restore_child() {
    let sandbox = tempfile::tempdir().unwrap();
    let (paths, source, mut parent, _) = fixture(sandbox.path());
    let (_, _, mut child, _) = fixture(sandbox.path());
    let parent_raw = child_bytes(&parent.thread_id, &child.thread_id, 0, 0);
    let child_raw = child_bytes(
        &child.thread_id,
        &parent.thread_id,
        1,
        parent_raw.len() as u64,
    );
    replace(&mut parent, &parent_raw);
    replace(&mut child, &child_raw);
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source.clone()],
            threads: vec![parent.clone(), child.clone()],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
    vault::protect(&paths, &child, &child_raw).unwrap();
    fs::remove_file(&child.path).unwrap();
    assert!(restore::preview_restore(&paths, &child)
        .unwrap_err()
        .contains("形成循环"));
    parent.source_id = "different-source".into();
    inventory::write(
        &paths,
        &ThreadIndex {
            sources: vec![source],
            threads: vec![parent, child.clone()],
            ..ThreadIndex::default()
        },
    )
    .unwrap();
    assert!(restore::preview_restore(&paths, &child)
        .unwrap_err()
        .contains("缺少历史依赖"));
    assert!(!Path::new(&child.path).exists());
}
