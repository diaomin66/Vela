use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

const MIGRATION_RECORD: &str = "brand-migration.json";
pub(crate) const CREDENTIAL_SERVICE: &str = "Vela";
pub(crate) const BACKUP_HEADER: &[u8] = b"VLB1";

pub(crate) fn is_default_provider(value: &str) -> bool {
    value == "Vela"
}

pub(crate) fn direct_provider(id: &str) -> String {
    format!("vela_{}", id.replace('-', ""))
}

pub(crate) fn is_direct_provider(value: &str) -> bool {
    value.starts_with("vela_")
}

pub(crate) fn helper_matches(command: &str, helper: &Path) -> bool {
    let command = Path::new(command);
    if !command.is_absolute() || !helper.is_absolute() {
        return false;
    }
    let normalized = |path: &Path| {
        std::path::absolute(path).ok().map(|absolute| {
            #[cfg(windows)]
            {
                absolute.to_string_lossy().replace('/', "\\").to_lowercase()
            }
            #[cfg(not(windows))]
            {
                absolute.to_string_lossy().to_string()
            }
        })
    };
    if normalized(command).is_some_and(|command| normalized(helper).as_ref() == Some(&command)) {
        return true;
    }
    command
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("vela.exe"))
        && helper
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("ahax.exe"))
        && command
            .parent()
            .zip(helper.parent())
            .is_some_and(|(left, right)| {
                normalized(left).is_some() && normalized(left) == normalized(right)
            })
}

#[derive(Serialize, Deserialize)]
struct MigrationRecord {
    version: u32,
    source: PathBuf,
}

pub(crate) fn data_directory(local: &Path) -> Result<PathBuf, String> {
    if let Some(value) = ["AHAX_DATA_DIR", "VELA_DATA_DIR", "CODEXTOOL_DATA_DIR"]
        .into_iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
    {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("AHAX_DATA_DIR 必须为绝对路径。".into());
        }
        return Ok(path);
    }
    migrate_default(local, require_legacy_closed)
}

pub(crate) fn catalog_roots(paths: &super::AppPaths) -> Vec<PathBuf> {
    let record = paths.data.join(MIGRATION_RECORD);
    if crate::locations::guard_brand_path(&record, true).is_err() {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(record) else {
        return Vec::new();
    };
    if bytes.len() > 16 * 1024 {
        return Vec::new();
    }
    let Ok(record) = serde_json::from_slice::<MigrationRecord>(&bytes) else {
        return Vec::new();
    };
    if record.version != 1 || !record.source.is_absolute() {
        return Vec::new();
    }
    let Some(local) = paths.data.parent().and_then(Path::parent) else {
        return Vec::new();
    };
    if record.source != local.join("Vela").join("data") {
        return Vec::new();
    }
    vec![record.source]
}

pub(crate) fn legacy_webview_directory(paths: &super::AppPaths) -> Result<Option<PathBuf>, String> {
    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some_and(|value| !value.is_empty()) {
        return Ok(None);
    }
    let base = directories::BaseDirs::new().ok_or("无法确定当前用户目录。")?;
    let default_data = base.data_local_dir().join("ahaX/data");
    let normalized = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    if normalized(&paths.data) != normalized(&default_data) {
        return Ok(None);
    }
    let legacy = base.data_local_dir().join("app.vela.desktop");
    if crate::locations::guard_brand_path(&legacy, true).is_ok() && legacy.is_dir() {
        require_legacy_closed()?;
        Ok(Some(legacy))
    } else {
        Ok(None)
    }
}

fn lock(path: &Path) -> Result<File, String> {
    crate::locations::guard_brand_path(path, true)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| "无法锁定旧版本数据，原文件保持不变。")?;
    file.try_lock_exclusive()
        .map_err(|_| "旧版本仍在使用数据，请退出旧版本后重新打开 ahaX。")?;
    Ok(file)
}

fn migrate_default(
    local: &Path,
    ensure_closed: impl Fn() -> Result<(), String>,
) -> Result<PathBuf, String> {
    let source = local.join("Vela").join("data");
    let target = local.join("ahaX").join("data");
    crate::locations::guard_brand_path(&source, true)?;
    crate::locations::guard_brand_path(&target, true)?;
    if !source
        .try_exists()
        .map_err(|_| "无法检查旧版本数据目录。")?
    {
        return Ok(target);
    }
    if target
        .try_exists()
        .map_err(|_| "无法检查 ahaX 数据目录。")?
    {
        let marker = target.join(MIGRATION_RECORD);
        let record: MigrationRecord = fs::read(marker)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or("新旧数据目录同时存在且未确认迁移完成；原数据均保留，请检查数据位置。")?;
        if record.version != 1 || record.source != source {
            return Err("数据迁移记录与当前目录不符，原数据均保留。".into());
        }
        return Ok(target);
    }
    ensure_closed()?;
    if !source.is_dir() {
        return Err("旧版本数据位置不是目录，未创建空数据。".into());
    }
    let parent = target.parent().ok_or("无法确定 ahaX 数据位置。")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建 ahaX 数据目录。")?;
    let _migration_lock = lock(&parent.join("upgrade.lock"))?;
    let _source_lock = lock(&source.join("changes.lock"))?;
    if target.exists() {
        return Err("另一实例刚完成迁移，请重新打开 ahaX。".into());
    }
    if source.join("location-migration.json").exists() {
        return Err("旧版本有未完成的数据位置迁移，请先在旧版本完成或取消后再打开 ahaX。".into());
    }
    if let Ok(bytes) = fs::read(source.join("locations.json")) {
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| "旧版数据位置设置无法解析，原文件保持不变。")?;
        if value
            .get("pending")
            .is_some_and(|pending| !pending.is_null())
        {
            return Err("旧版本有待重启的位置设置，请先完成或取消后再打开 ahaX。".into());
        }
    }
    let staging = parent.join(format!(".upgrade-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|_| "无法创建升级暂存目录。")?;
    crate::locations::copy_brand_data(&source, &staging)?;
    let identity_file = staging.join("gateway-credential-id");
    if !identity_file.exists() {
        let id = crate::security::credential_id_for_directory(&source);
        super::atomic_write(&identity_file, id.as_bytes())?;
    }
    let identity = fs::read_to_string(&identity_file).map_err(|_| "旧版网关凭据标识无法读取。")?;
    super::validate_id(identity.trim())?;
    let record = MigrationRecord { version: 1, source };
    super::atomic_write(
        &staging.join(MIGRATION_RECORD),
        &serde_json::to_vec_pretty(&record).map_err(|_| "无法记录品牌迁移。")?,
    )?;
    ensure_closed()?;
    fs::rename(&staging, &target)
        .map_err(|_| "升级副本已保留，数据目录尚未切换，请重新打开 ahaX。")?;
    Ok(target)
}

#[cfg(windows)]
fn require_legacy_closed() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq vela.exe", "/FO", "CSV", "/NH"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|_| "无法确认旧版本已退出，未迁移数据。")?;
    if !output.status.success() {
        return Err("无法确认旧版本已退出，未迁移数据。".into());
    }
    if String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("\"vela.exe\",")
    }) {
        return Err(
            "检测到旧版本仍在运行，请从托盘退出旧版本后重新打开 ahaX；历史数据保持不变。".into(),
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn require_legacy_closed() -> Result<(), String> {
    Err("自动升级旧数据仅支持 Windows，原数据保持不变。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_upgrade_accepts_only_the_exact_installation_directory() {
        let temp = tempfile::tempdir().unwrap();
        let helper = temp.path().join("install/ahax.exe");
        assert!(helper_matches(&helper.to_string_lossy(), &helper));
        assert!(helper_matches(
            &temp.path().join("install/vela.exe").to_string_lossy(),
            &helper
        ));
        assert!(!helper_matches(
            &temp.path().join("other/vela.exe").to_string_lossy(),
            &helper
        ));
        assert!(!helper_matches("vela.exe", &helper));
        assert!(!helper_matches(
            &temp.path().join("install/other.exe").to_string_lossy(),
            &helper
        ));
    }

    #[test]
    fn verified_upgrade_preserves_source_custom_locations_and_gateway_identity() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Vela").join("data");
        fs::create_dir_all(source.join("backups")).unwrap();
        fs::write(source.join("connections.json"), b"{\"profiles\":[]}").unwrap();
        fs::write(source.join("backups/original.bin"), b"VLB1encrypted").unwrap();
        let locations = br#"{"active":{"backupsDirectory":"D:\\Custom\\backups"},"pending":null}"#;
        fs::write(source.join("locations.json"), locations).unwrap();
        let target = migrate_default(temp.path(), || Ok(())).unwrap();
        assert_eq!(target, temp.path().join("ahaX/data"));
        assert_eq!(
            fs::read(target.join("backups/original.bin")).unwrap(),
            b"VLB1encrypted"
        );
        assert_eq!(fs::read(target.join("locations.json")).unwrap(), locations);
        assert!(source.join("connections.json").exists());
        assert_eq!(
            fs::read_to_string(target.join("gateway-credential-id")).unwrap(),
            crate::security::credential_id_for_directory(&source)
        );
        assert_eq!(
            migrate_default(temp.path(), || panic!(
                "completed migration must not recopy"
            ))
            .unwrap(),
            target
        );
    }

    #[test]
    fn active_old_process_and_conflicting_target_never_activate_empty_storage() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("Vela/data")).unwrap();
        assert!(migrate_default(temp.path(), || Err("running".into())).is_err());
        assert!(!temp.path().join("ahaX/data").exists());
        fs::create_dir_all(temp.path().join("ahaX/data")).unwrap();
        assert!(migrate_default(temp.path(), || Ok(())).is_err());
    }

    #[test]
    fn pending_location_migration_keeps_original_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Vela").join("data");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("locations.json"),
            br#"{"pending":{"codexHome":"D:\\Another"}}"#,
        )
        .unwrap();
        assert!(migrate_default(temp.path(), || Ok(())).is_err());
        assert!(!temp.path().join("ahaX/data").exists());
    }

    #[test]
    fn sqlite_wal_records_and_encrypted_snapshots_survive_upgrade() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Vela").join("data");
        fs::create_dir_all(source.join("threads")).unwrap();
        let database =
            rusqlite::Connection::open(source.join("threads/inventory.sqlite3")).unwrap();
        database.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE protected (id TEXT PRIMARY KEY); INSERT INTO protected VALUES ('persisted-thread');").unwrap();
        assert!(source.join("threads/inventory.sqlite3-wal").exists());
        let target = migrate_default(temp.path(), || Ok(())).unwrap();
        assert!(!target.join("threads/inventory.sqlite3-wal").exists());
        let migrated =
            rusqlite::Connection::open(target.join("threads/inventory.sqlite3")).unwrap();
        let value: String = migrated
            .query_row("SELECT id FROM protected", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "persisted-thread");
        assert!(source.join("threads/inventory.sqlite3").exists());
    }

    #[test]
    fn checkpointed_sqlite_index_can_be_copied_after_old_application_exits() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Vela").join("data");
        fs::create_dir_all(source.join("threads")).unwrap();
        let database =
            rusqlite::Connection::open(source.join("threads/inventory.sqlite3")).unwrap();
        database.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE protected (id INTEGER); INSERT INTO protected VALUES (42);").unwrap();
        drop(database);
        let target = migrate_default(temp.path(), || Ok(())).unwrap();
        let migrated =
            rusqlite::Connection::open(target.join("threads/inventory.sqlite3")).unwrap();
        let value: i64 = migrated
            .query_row("SELECT id FROM protected", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, 42);
    }
}
