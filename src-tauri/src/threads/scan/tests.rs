use super::*;
use serde_json::json;
use std::io::Write;

const FIRST_ID: &str = "0199a5d0-9ac1-79d0-8be1-223344556677";
const SECOND_ID: &str = "0199a5d0-9ac1-79d0-8be1-223344556688";

fn fixture() -> (tempfile::TempDir, AppPaths, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("home");
    fs::create_dir_all(&root).unwrap();
    let paths = AppPaths {
        data: directory.path().join("data"),
        config: root.join("config.toml"),
        helper: directory.path().join("helper.exe"),
        locations: None,
    };
    (directory, paths, root)
}

fn rollout(root: &Path, id: &str, archived: bool, suffix: &str) -> PathBuf {
    let base = if archived {
        root.join("archived_sessions")
    } else {
        root.join("sessions/2026/10/04")
    };
    fs::create_dir_all(&base).unwrap();
    base.join(format!("rollout-2026-10-04T12-00-00-{id}{suffix}.jsonl"))
}

fn document(id: &str) -> Vec<u8> {
    [
        json!({"timestamp":"2026-10-04T12:00:00Z","type":"session_meta","payload":{"id":id,"timestamp":"2026-10-04T12:00:00Z","cwd":"D:/project/旧项目","source":"cli","model_provider":"earlier-provider"}}),
        json!({"timestamp":"2026-10-04T12:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"查找并修复列表布局"}]}}),
        json!({"timestamp":"2026-10-04T12:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"已整理布局。"}]}}),
    ].into_iter().map(|value| format!("{value}\n")).collect::<String>().into_bytes()
}

fn ordinal_document(id: &str) -> Vec<u8> {
    String::from_utf8(document(id))
        .unwrap()
        .lines()
        .enumerate()
        .map(|(ordinal, line)| {
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["ordinal"] = json!(ordinal);
            if ordinal == 0 {
                value["payload"]["history_mode"] = json!("paginated");
            }
            format!("{value}\n")
        })
        .collect::<String>()
        .into_bytes()
}

#[test]
fn changing_current_home_retains_each_historical_sources_custom_database() {
    let (directory, paths, root) = fixture();
    let sqlite_home = directory.path().join("old-custom-database");
    fs::create_dir_all(&sqlite_home).unwrap();
    let source = rollout(&root, FIRST_ID, false, "");
    let contents = document(FIRST_ID);
    fs::write(&source, &contents).unwrap();
    let database = rusqlite::Connection::open(sqlite_home.join("state_5.sqlite")).unwrap();
    database
        .execute_batch("CREATE TABLE threads(id TEXT, rollout_path TEXT, title TEXT);")
        .unwrap();
    database
        .execute(
            "INSERT INTO threads VALUES(?1,?2,'Retained index title')",
            [FIRST_ID, &source.to_string_lossy()],
        )
        .unwrap();
    drop(database);
    // This override deliberately does not exist in the source's config.toml.
    let original = crate::locations::ResolvedLocations::defaults(&paths.data, &root, &sqlite_home);
    let original_paths = paths.with_locations(original);
    let before = run(&original_paths).unwrap();
    assert_eq!(before.threads[0].state_index.as_deref(), Some("indexed"));
    assert_eq!(
        PathBuf::from(before.sources[0].sqlite_home.as_ref().unwrap()),
        sqlite_home
    );
    storage::write(&original_paths, &before).unwrap();
    let new_home = directory.path().join("new-home");
    fs::create_dir_all(&new_home).unwrap();
    let next =
        crate::locations::ResolvedLocations::defaults(&original_paths.data, &new_home, &new_home);
    let next_paths = original_paths.with_locations(next);
    let after = run(&next_paths).unwrap();
    let retained = after
        .sources
        .iter()
        .find(|candidate| candidate.id == before.sources[0].id)
        .unwrap();
    assert_eq!(
        PathBuf::from(retained.sqlite_home.as_ref().unwrap()),
        sqlite_home
    );
    assert_eq!(
        super::super::reconcile::sqlite_root(&next_paths, retained).unwrap(),
        sqlite_home
    );
    assert_eq!(after.threads[0].state_index.as_deref(), Some("indexed"));
    assert_eq!(
        after.threads[0].title.as_deref(),
        Some("Retained index title")
    );
    storage::write(&next_paths, &after).unwrap();
    let rebuilt = storage::rebuild(&next_paths).unwrap();
    let retained = rebuilt
        .sources
        .iter()
        .find(|candidate| candidate.id == before.sources[0].id)
        .unwrap();
    assert_eq!(
        PathBuf::from(retained.sqlite_home.as_ref().unwrap()),
        sqlite_home
    );
    assert_eq!(fs::read(&source).unwrap(), contents);
    assert!(!root.join("state_5.sqlite").exists());
}

#[test]
fn historical_sources_resolve_their_own_config_and_fail_closed_when_invalid() {
    let (directory, paths, _) = fixture();
    let old_home = directory.path().join("imported-home");
    let source = rollout(&old_home, FIRST_ID, false, "");
    fs::write(&source, document(FIRST_ID)).unwrap();
    fs::write(
        old_home.join("config.toml"),
        "sqlite_home = 'separate-state'\n",
    )
    .unwrap();
    let sqlite_home = old_home.join("separate-state");
    fs::create_dir_all(&sqlite_home).unwrap();
    let database = rusqlite::Connection::open(sqlite_home.join("state_5.sqlite")).unwrap();
    database
        .execute_batch("CREATE TABLE threads(id TEXT, rollout_path TEXT, title TEXT);")
        .unwrap();
    database
        .execute(
            "INSERT INTO threads VALUES(?1,?2,'Historical index')",
            [FIRST_ID, &source.to_string_lossy()],
        )
        .unwrap();
    drop(database);
    let index = run_with_sources(&paths, ThreadIndex::default(), vec![old_home.clone()]).unwrap();
    assert_eq!(index.threads[0].state_index.as_deref(), Some("indexed"));
    // Windows TEMP can use an 8.3 alias while discovery canonicalizes the
    // source home. Assert the database's filesystem identity, not its spelling.
    assert_eq!(
        fs::canonicalize(index.sources[0].sqlite_home.as_ref().unwrap()).unwrap(),
        fs::canonicalize(&sqlite_home).unwrap()
    );
    fs::write(old_home.join("config.toml"), "sqlite_home = 17\n").unwrap();
    let invalid = run_with_sources(&paths, ThreadIndex::default(), vec![old_home.clone()]).unwrap();
    assert!(invalid.sources[0].error.is_some());
    assert!(invalid.sources[0].sqlite_home.is_none());
    assert_eq!(
        invalid.threads[0].state_index.as_deref(),
        Some("unavailable")
    );
    assert!(super::super::reconcile::sqlite_root(&paths, &invalid.sources[0]).is_err());
    assert!(!old_home.join("state_5.sqlite").exists());
}

fn based_document(id: &str, base: &str, parent: &[u8]) -> Vec<u8> {
    let value: serde_json::Value =
        serde_json::from_str(std::str::from_utf8(parent).unwrap().lines().last().unwrap()).unwrap();
    let ordinal = value["ordinal"].as_u64().unwrap() + 1;
    format!("{}\n", json!({"ordinal":ordinal,"type":"session_meta","payload":{"id":id,"history_mode":"paginated","history_base":{"thread_id":base,"end_ordinal_exclusive":ordinal,"end_byte_offset":parent.len()}}})).into_bytes()
}

#[cfg(windows)]
#[test]
fn history_dependencies_follow_immutable_rollout_ids_not_the_active_thread_id() {
    const THIRD_ID: &str = "0199a5d0-9ac1-79d0-8be1-223344556699";
    let (_directory, paths, root) = fixture();
    let original = ordinal_document(FIRST_ID);
    let reverted = based_document(FIRST_ID, FIRST_ID, &original);
    fs::write(rollout(&root, FIRST_ID, false, ""), &original).unwrap();
    fs::write(
        rollout(&root, FIRST_ID, false, &format!("_{SECOND_ID}")),
        &reverted,
    )
    .unwrap();
    fs::write(
        rollout(&root, THIRD_ID, false, ""),
        based_document(THIRD_ID, SECOND_ID, &reverted),
    )
    .unwrap();
    let index = run_with_sources(&paths, ThreadIndex::default(), vec![root]).unwrap();
    assert_eq!(index.threads.len(), 3);
    assert!(index
        .threads
        .iter()
        .all(|thread| thread.integrity == "valid" && thread.snapshot == SnapshotState::Protected));
    let child = index
        .threads
        .iter()
        .find(|thread| thread.thread_id == THIRD_ID)
        .unwrap();
    assert_eq!(child.history_base.as_ref().unwrap().thread_id, SECOND_ID);
    assert_eq!(
        child.history_base.as_ref().unwrap().end_byte_offset,
        Some(reverted.len() as u64)
    );
}

#[cfg(windows)]
#[test]
fn missing_history_parent_blocks_complete_protection_even_when_a_parent_snapshot_exists() {
    let (_directory, paths, root) = fixture();
    let parent = rollout(&root, FIRST_ID, false, "");
    let original = ordinal_document(FIRST_ID);
    fs::write(&parent, &original).unwrap();
    fs::write(
        rollout(&root, SECOND_ID, false, ""),
        based_document(SECOND_ID, FIRST_ID, &original),
    )
    .unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    fs::remove_file(&parent).unwrap();
    let second = run_with_sources(&paths, first, vec![root.clone()]).unwrap();
    let missing_parent = second
        .threads
        .iter()
        .find(|thread| thread.thread_id == FIRST_ID)
        .unwrap();
    assert_eq!(missing_parent.recoverability, "recoverable");
    let child = second
        .threads
        .iter()
        .find(|thread| thread.thread_id == SECOND_ID)
        .unwrap();
    assert_eq!(child.integrity, "dependency-missing");
    assert_eq!(child.recoverability, "dependencies-missing");
    assert_eq!(child.snapshot, SnapshotState::Failed);
    assert!(storage::snapshot(&paths, child).unwrap().unwrap().bytes > 0);
    fs::write(&parent, original).unwrap();
    let third = run_with_sources(&paths, second, vec![root]).unwrap();
    assert!(third
        .threads
        .iter()
        .all(|thread| thread.integrity == "valid"));
}

#[cfg(windows)]
#[test]
fn missing_history_parent_without_any_snapshot_preserves_the_child_evidence() {
    let (_directory, paths, root) = fixture();
    fs::write(
        rollout(&root, SECOND_ID, false, ""),
        based_document(SECOND_ID, FIRST_ID, &ordinal_document(FIRST_ID)),
    )
    .unwrap();
    let scanned = run_with_sources(&paths, ThreadIndex::default(), vec![root]).unwrap();
    assert_eq!(scanned.threads[0].integrity, "dependency-missing");
    assert_eq!(scanned.threads[0].snapshot, SnapshotState::Failed);
    assert!(storage::snapshot(&paths, &scanned.threads[0])
        .unwrap()
        .is_some());
}

#[cfg(windows)]
#[test]
fn cyclic_history_dependencies_are_retained_but_never_reported_complete() {
    let (_directory, paths, root) = fixture();
    fs::write(
        rollout(&root, FIRST_ID, false, ""),
        based_document(FIRST_ID, SECOND_ID, &ordinal_document(SECOND_ID)),
    )
    .unwrap();
    fs::write(
        rollout(&root, SECOND_ID, false, ""),
        based_document(SECOND_ID, FIRST_ID, &ordinal_document(FIRST_ID)),
    )
    .unwrap();
    let scanned = run_with_sources(&paths, ThreadIndex::default(), vec![root]).unwrap();
    assert!(scanned
        .threads
        .iter()
        .all(|thread| thread.integrity == "dependency-cycle"
            && thread.snapshot == SnapshotState::Failed));
    assert!(scanned
        .threads
        .iter()
        .all(|thread| thread.recoverability == "dependencies-missing"));
}

#[test]
fn invalid_history_base_is_unrecognized_in_both_readers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rollout.jsonl");
    let contents = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"id":FIRST_ID,"history_base":{"thread_id":SECOND_ID,"end_ordinal_exclusive":-1}}})
    );
    fs::write(&path, &contents).unwrap();
    assert_eq!(
        parsing::parse(&path, contents.as_bytes(), false).integrity,
        "unrecognized"
    );
    assert_eq!(
        streaming::scan(directory.path(), &path)
            .ok()
            .unwrap()
            .parsed
            .integrity,
        "unrecognized"
    );
}

#[cfg(windows)]
#[test]
fn complete_but_truncated_parent_records_do_not_claim_a_complete_child_history() {
    let (_directory, paths, root) = fixture();
    let original = ordinal_document(FIRST_ID);
    let parent = rollout(&root, FIRST_ID, false, "");
    fs::write(&parent, &original).unwrap();
    fs::write(
        rollout(&root, SECOND_ID, false, ""),
        based_document(SECOND_ID, FIRST_ID, &original),
    )
    .unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    assert!(first
        .threads
        .iter()
        .all(|thread| thread.snapshot == SnapshotState::Protected));
    let prefix = original.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    fs::write(&parent, &original[..prefix]).unwrap();
    let second = run_with_sources(&paths, first, vec![root.clone()]).unwrap();
    let parent_state = second
        .threads
        .iter()
        .find(|thread| thread.thread_id == FIRST_ID)
        .unwrap();
    assert_eq!(parent_state.integrity, "valid");
    let child = second
        .threads
        .iter()
        .find(|thread| thread.thread_id == SECOND_ID)
        .unwrap();
    assert_eq!(child.integrity, "dependency-missing");
    assert_eq!(child.snapshot, SnapshotState::Failed);
    assert_eq!(child.recoverability, "dependencies-missing");
    fs::write(&parent, original).unwrap();
    let third = run_with_sources(&paths, second, vec![root]).unwrap();
    assert!(third
        .threads
        .iter()
        .all(|thread| thread.integrity == "valid"));
}

#[test]
fn parser_uses_first_metadata_preserves_provider_and_bounds_preview() {
    let mut bytes = document(FIRST_ID);
    bytes.extend_from_slice(format!("{}\n", json!({"type":"session_meta","payload":{"id":SECOND_ID,"cwd":"other","model_provider":"other","history_mode":"future"}})).as_bytes());
    let parsed = parsing::parse(Path::new("rollout.jsonl"), &bytes, true);
    assert_eq!(parsed.thread_id.as_deref(), Some(FIRST_ID));
    assert_eq!(parsed.provider.as_deref(), Some("earlier-provider"));
    assert_eq!(parsed.cwd.as_deref(), Some("D:/project/旧项目"));
    assert_eq!(parsed.integrity, "valid");
    assert_eq!(parsed.preview.len(), 2);
    assert_eq!(parsed.preview[0].role.as_deref(), Some("user"));
}

#[test]
fn malformed_tail_and_unknown_mode_are_preserved_without_claiming_integrity() {
    let mut bytes = document(FIRST_ID);
    bytes.extend_from_slice(b"{unfinished");
    assert_eq!(
        parsing::parse(Path::new("rollout.jsonl"), &bytes, false).integrity,
        "partial"
    );
    let unknown = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"id":FIRST_ID,"history_mode":"future-format"}})
    );
    assert_eq!(
        parsing::parse(Path::new("rollout.jsonl"), unknown.as_bytes(), false).integrity,
        "unrecognized"
    );
    let no_meta =
        br#"{"type":"event_msg","payload":{"type":"user_message","message":"not a rollout"}}
"#;
    assert_eq!(
        parsing::parse(Path::new("rollout.jsonl"), no_meta, false).integrity,
        "unrecognized"
    );
    let mut corrupt = document(FIRST_ID);
    corrupt.extend_from_slice(b"{broken}\n");
    assert_eq!(
        parsing::parse(Path::new("rollout.jsonl"), &corrupt, false).integrity,
        "corrupt"
    );
    assert!(
        parsing::parse(Path::new("rollout.jsonl"), &document(FIRST_ID), false)
            .preview
            .is_empty()
    );
    assert!(
        parsing::parse(Path::new("rollout.jsonl"), &document(FIRST_ID), false)
            .title
            .is_none()
    );
}

#[test]
fn legacy_metadata_aliases_remain_discoverable() {
    for name in ["thread_id", "session_id"] {
        let value = format!(
            "{}\n",
            json!({"type":"session_meta","payload":{name:FIRST_ID}})
        );
        let parsed = parsing::parse(Path::new("rollout.jsonl"), value.as_bytes(), false);
        assert_eq!(parsed.thread_id.as_deref(), Some(FIRST_ID));
        assert_eq!(parsed.integrity, "valid");
    }
}

#[test]
fn title_sidecar_uses_latest_valid_name_and_is_not_required_for_discovery() {
    let (_directory, paths, root) = fixture();
    let path = rollout(&root, FIRST_ID, false, "");
    fs::write(&path, document(FIRST_ID)).unwrap();
    let index = ThreadIndex {
        settings: ThreadSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let scanned = run_with_sources(&paths, index, vec![root.clone()]).unwrap();
    assert_eq!(scanned.threads.len(), 1);
    assert!(!scanned.threads[0].index_present);
    assert_eq!(scanned.threads[0].integrity, "valid");
    assert_eq!(scanned.threads[0].recoverability, "source-present");
    fs::write(
        root.join("session_index.jsonl"),
        format!(
            "{}\n{}\n{{broken\n",
            json!({"id":FIRST_ID,"thread_name":"previous"}),
            json!({"id":FIRST_ID,"thread_name":"最新名称"})
        ),
    )
    .unwrap();
    let names = parsing::read_names(&root);
    assert_eq!(names.get(FIRST_ID).map(String::as_str), Some("最新名称"));
}

#[test]
fn compression_and_revert_filename_remain_distinct_rollout_evidence() {
    let (_directory, paths, root) = fixture();
    let first = rollout(&root, FIRST_ID, false, "");
    let second = rollout(&root, FIRST_ID, false, &format!("_{SECOND_ID}"));
    fs::write(&first, document(FIRST_ID)).unwrap();
    let compressed = second.with_extension("jsonl.zst");
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 1).unwrap();
    encoder.write_all(&document(FIRST_ID)).unwrap();
    fs::write(&compressed, encoder.finish().unwrap()).unwrap();
    assert_eq!(parsing::filename_id(&compressed).as_deref(), Some(FIRST_ID));
    let index = ThreadIndex {
        settings: ThreadSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let scanned = run_with_sources(&paths, index, vec![root]).unwrap();
    assert_eq!(scanned.threads.len(), 2);
    assert_ne!(scanned.threads[0].key, scanned.threads[1].key);
    assert!(scanned
        .threads
        .iter()
        .all(|thread| thread.integrity == "valid"));
}

#[test]
fn sqlite_schema_read_is_optional_and_identifies_selected_rollout() {
    let (_directory, paths, root) = fixture();
    let first = rollout(&root, FIRST_ID, false, "");
    let second = rollout(&root, FIRST_ID, false, &format!("_{SECOND_ID}"));
    fs::write(&first, document(FIRST_ID)).unwrap();
    fs::write(&second, document(FIRST_ID)).unwrap();
    let database = root.join("state_5.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT);")
        .unwrap();
    connection
        .execute(
            "INSERT INTO threads VALUES(?1,?2,?3)",
            rusqlite::params![FIRST_ID, second.to_string_lossy(), "已选历史"],
        )
        .unwrap();
    drop(connection);
    let before = fs::read(&database).unwrap();
    let index = ThreadIndex {
        settings: ThreadSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let scanned = run_with_sources(&paths, index, vec![root]).unwrap();
    assert_eq!(
        scanned
            .threads
            .iter()
            .filter(|thread| thread.selected_rollout == Some(true))
            .count(),
        1
    );
    assert!(scanned
        .threads
        .iter()
        .all(|thread| thread.state_index.as_deref() == Some("indexed")));
    assert_eq!(fs::read(database).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn sqlite_selected_rollout_resolves_windows_short_path_aliases() {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let (_directory, paths, root) = fixture();
    let first = rollout(&root, FIRST_ID, false, "");
    let selected = rollout(&root, FIRST_ID, false, &format!("_{SECOND_ID}"));
    fs::write(&first, document(FIRST_ID)).unwrap();
    fs::write(&selected, document(FIRST_ID)).unwrap();
    let wide: Vec<_> = selected.as_os_str().encode_wide().chain(Some(0)).collect();
    let needed = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
    assert!(
        needed > 0,
        "GetShortPathNameW size query failed: {}",
        std::io::Error::last_os_error()
    );
    let mut buffer = vec![0u16; needed as usize];
    let length = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), needed) };
    assert!(
        length > 0 && length < needed,
        "GetShortPathNameW failed: {}",
        std::io::Error::last_os_error()
    );
    let short = PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    let canonical = fs::canonicalize(&selected).unwrap();
    if files::normalized_path(&short) == files::normalized_path(&canonical) {
        eprintln!("This volume does not expose a distinct 8.3 alias; the regular SQLite path test covers canonical names.");
        return;
    }
    eprintln!(
        "Validating distinct Windows 8.3 rollout alias: {}",
        short.display()
    );
    assert_eq!(fs::canonicalize(&short).unwrap(), canonical);

    let database = root.join("state_5.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT);")
        .unwrap();
    connection
        .execute(
            "INSERT INTO threads VALUES(?1,?2,?3)",
            rusqlite::params![FIRST_ID, short.to_string_lossy(), "短路径索引标题"],
        )
        .unwrap();
    drop(connection);
    let before = fs::read(&database).unwrap();
    let index = ThreadIndex {
        settings: ThreadSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let scanned = run_with_sources(&paths, index, vec![root]).unwrap();
    let selected_threads: Vec<_> = scanned
        .threads
        .iter()
        .filter(|thread| thread.selected_rollout == Some(true))
        .collect();
    assert_eq!(selected_threads.len(), 1);
    assert_eq!(
        fs::canonicalize(&selected_threads[0].path).unwrap(),
        canonical
    );
    assert_eq!(selected_threads[0].title.as_deref(), Some("短路径索引标题"));
    assert_eq!(fs::read(database).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn stable_prefix_is_protected_and_source_bytes_never_change() {
    let (_directory, paths, root) = fixture();
    let path = rollout(&root, FIRST_ID, false, "");
    let complete = document(FIRST_ID);
    let mut bytes = complete.clone();
    bytes.extend_from_slice(b"{unfinished");
    fs::write(&path, &bytes).unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    let thread = &first.threads[0];
    assert_eq!(thread.integrity, "partial");
    assert_eq!(thread.snapshot, SnapshotState::Pending);
    assert_eq!(
        storage::snapshot_bytes(&paths, thread).unwrap().unwrap(),
        complete
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::remove_file(&path).unwrap();
    let after = run_with_sources(&paths, first, vec![root]).unwrap();
    assert_eq!(after.threads.len(), 1);
    assert_eq!(after.threads[0].recoverability, "recoverable");
    assert_eq!(after.threads[0].integrity, "missing");
    let detail = detail(&paths, &after.threads[0]).unwrap();
    assert_eq!(detail.preview.len(), 2);
    assert!(!detail.raw_available);
}

#[cfg(windows)]
#[test]
fn archive_move_does_not_create_a_false_missing_thread() {
    let (_directory, paths, root) = fixture();
    let path = rollout(&root, FIRST_ID, false, "");
    fs::write(&path, document(FIRST_ID)).unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    let archived = rollout(&root, FIRST_ID, true, "");
    fs::rename(&path, &archived).unwrap();
    let after = run_with_sources(&paths, first, vec![root]).unwrap();
    assert_eq!(after.threads.len(), 1);
    assert!(after.threads[0].archived);
    assert_eq!(after.threads[0].recoverability, "source-present");
}

#[test]
fn corrupted_index_database_never_hides_rollout_files() {
    let (_directory, paths, root) = fixture();
    fs::write(rollout(&root, FIRST_ID, false, ""), document(FIRST_ID)).unwrap();
    fs::write(root.join("state_5.sqlite"), b"not a database").unwrap();
    let index = ThreadIndex {
        settings: ThreadSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let scanned = run_with_sources(&paths, index, vec![root.clone()]).unwrap();
    assert_eq!(scanned.threads.len(), 1);
    assert_eq!(
        scanned.threads[0].state_index.as_deref(),
        Some("unavailable")
    );
    assert_eq!(
        fs::read(root.join("state_5.sqlite")).unwrap(),
        b"not a database"
    );
}

#[test]
fn first_time_user_with_no_home_is_an_empty_source_without_a_protection_error() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("fresh-home");
    let paths = AppPaths {
        data: directory.path().join("data"),
        config: root.join("config.toml"),
        helper: directory.path().join("helper.exe"),
        locations: None,
    };
    let scanned = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    assert!(scanned.threads.is_empty());
    assert!(scanned.protection.error.is_none());
    assert!(scanned.sources[0].error.is_none());
    assert!(!scanned.sources[0].available);
    assert!(!root.exists());
}

#[cfg(windows)]
#[test]
fn configuration_checkpoint_protects_while_preserving_the_paused_setting() {
    let (_directory, paths, root) = fixture();
    fs::write(rollout(&root, FIRST_ID, false, ""), document(FIRST_ID)).unwrap();
    let mut initial = ThreadIndex::default();
    initial.settings.enabled = false;
    storage::write(&paths, &initial).unwrap();
    let checked = checkpoint(&paths).unwrap();
    assert!(!checked.settings.enabled);
    assert_eq!(checked.protection.state, "protected");
    assert_eq!(checked.threads[0].snapshot, SnapshotState::Protected);
    assert!(
        storage::snapshot(&paths, &checked.threads[0])
            .unwrap()
            .unwrap()
            .recoverable
    );
}

#[cfg(windows)]
#[test]
fn paused_protection_never_promotes_unknown_snapshot_bytes_to_verified_history() {
    let (_directory, paths, root) = fixture();
    let path = rollout(&root, FIRST_ID, false, "");
    let contents = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"id":FIRST_ID,"history_mode":"future-format"}})
    );
    fs::write(&path, contents).unwrap();
    let mut first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    assert_eq!(first.threads[0].integrity, "unrecognized");
    assert_eq!(first.threads[0].snapshot, SnapshotState::Failed);
    first.settings.enabled = false;
    let second = run_with_sources(&paths, first, vec![root]).unwrap();
    assert_eq!(second.threads[0].snapshot, SnapshotState::Failed);
}

#[cfg(windows)]
#[test]
fn junction_sources_are_refused_and_incomplete_scans_never_infer_deletion() {
    use std::os::windows::process::CommandExt;
    let (directory, paths, root) = fixture();
    let path = rollout(&root, FIRST_ID, false, "");
    fs::write(&path, document(FIRST_ID)).unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    let alias = directory.path().join("alias");
    let created = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&alias)
        .arg(&root)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(created.status.success());
    assert!(!files::root_available(&alias));
    let aliased = run_with_sources(&paths, ThreadIndex::default(), vec![alias.clone()]).unwrap();
    assert!(aliased.threads.is_empty());
    assert!(aliased.sources[0].error.is_some());
    fs::remove_dir(&alias).unwrap();

    let original = root.join("sessions");
    let moved = directory.path().join("detached-sessions");
    fs::rename(&original, &moved).unwrap();
    let created = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&original)
        .arg(&moved)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(created.status.success());
    let incomplete = run_with_sources(&paths, first, vec![root]).unwrap();
    assert_eq!(incomplete.threads.len(), 1);
    assert_eq!(incomplete.threads[0].integrity, "unreadable");
    assert_eq!(incomplete.threads[0].recoverability, "unavailable");
    assert!(incomplete.sources[0].error.is_some());
    fs::remove_dir(original).unwrap();
}

#[cfg(windows)]
#[test]
fn nested_junction_does_not_turn_an_unobserved_original_into_a_deleted_thread() {
    use std::os::windows::process::CommandExt;
    let (directory, paths, root) = fixture();
    fs::write(rollout(&root, FIRST_ID, false, ""), document(FIRST_ID)).unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    let original = root.join("sessions").join("2026").join("10");
    let detached = directory.path().join("detached-month");
    let empty = directory.path().join("empty-target");
    fs::create_dir(&empty).unwrap();
    fs::rename(&original, &detached).unwrap();
    let created = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&original)
        .arg(&empty)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "junction creation failed: {} {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let second = run_with_sources(&paths, first, vec![root]).unwrap();
    assert!(second.sources[0].error.is_some());
    assert_eq!(second.threads[0].integrity, "unreadable");
    assert_eq!(second.threads[0].recoverability, "unavailable");
    assert_eq!(second.threads[0].snapshot, SnapshotState::Pending);
    assert!(
        storage::snapshot(&paths, &second.threads[0])
            .unwrap()
            .unwrap()
            .recoverable
    );
    fs::remove_dir(original).unwrap();
}

#[cfg(windows)]
#[test]
fn unavailable_source_is_not_reported_as_deleted_and_incremental_scan_rechecks_changed_files() {
    let (directory, paths, root) = fixture();
    let path = rollout(&root, FIRST_ID, false, "");
    fs::write(&path, document(FIRST_ID)).unwrap();
    let first = run_with_sources(&paths, ThreadIndex::default(), vec![root.clone()]).unwrap();
    assert_eq!(first.threads[0].snapshot, SnapshotState::Protected);
    let second = run_scan(&paths, first, vec![root.clone()], false).unwrap();
    assert_eq!(second.threads[0].snapshot, SnapshotState::Protected);
    let mut changed = document(FIRST_ID);
    changed.extend_from_slice(
        format!(
            "{}\n",
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"New response"}})
        )
        .as_bytes(),
    );
    fs::write(&path, changed).unwrap();
    let third = run_scan(&paths, second, vec![root.clone()], false).unwrap();
    assert_eq!(third.threads[0].line_count, 4);
    let offline = directory.path().join("offline");
    fs::rename(&root, &offline).unwrap();
    let fourth = run_with_sources(&paths, third, vec![root]).unwrap();
    assert_eq!(fourth.threads[0].integrity, "source-unavailable");
    assert_eq!(fourth.threads[0].recoverability, "unavailable");
    assert_eq!(fourth.threads[0].snapshot, SnapshotState::Pending);
}
