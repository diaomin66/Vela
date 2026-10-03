//! Resolve per-user locations and serialize mutations through the application lock.
use super::domain::validate_id;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

const DEFAULT_INSTANCE_IDENTIFIER: &str = "app.vela.desktop";

#[derive(Clone)]
pub struct AppPaths {
    pub data: PathBuf,
    pub config: PathBuf,
    pub helper: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, String> {
        let base = directories::BaseDirs::new().ok_or("无法确定当前用户目录。")?;
        let codex_home = std::env::var_os("CODEX_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".codex"));
        if !codex_home.is_absolute() {
            return Err("CODEX_HOME 必须为绝对路径。".into());
        }
        let data = std::env::var_os("VELA_DATA_DIR")
            .or_else(|| std::env::var_os("CODEXTOOL_DATA_DIR"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| base.data_local_dir().join("Vela").join("data"));
        if !data.is_absolute() {
            return Err("VELA_DATA_DIR 必须为绝对路径。".into());
        }
        Ok(Self {
            data,
            config: codex_home.join("config.toml"),
            helper: std::env::current_exe().map_err(|_| "无法定位凭据读取程序。")?,
        })
    }

    /// Keep the installed application's identity for its normal data directory,
    /// while allowing explicitly isolated data directories to run independently.
    pub fn instance_identifier(&self) -> Result<String, String> {
        let base = directories::BaseDirs::new().ok_or("无法确定当前用户目录。")?;
        let default_data = base.data_local_dir().join("Vela").join("data");
        instance_identifier_for(&self.data, &default_data)
    }

    pub(super) fn profile_file(&self) -> PathBuf {
        self.data.join("connections.json")
    }
    pub(super) fn backup_file(&self, id: &str) -> Result<PathBuf, String> {
        validate_id(id)?;
        Ok(self.data.join("backups").join(format!("{id}.bin")))
    }
    pub(crate) fn lock(&self) -> Result<File, String> {
        fs::create_dir_all(&self.data).map_err(|_| "无法创建应用数据目录。")?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.data.join("changes.lock"))
            .map_err(|_| "无法锁定应用数据。")?;
        file.try_lock_exclusive()
            .map_err(|_| "另一项变更正在进行，请稍后重试。")?;
        Ok(file)
    }
}

fn instance_identifier_for(data: &Path, default_data: &Path) -> Result<String, String> {
    let normalized = normalize_instance_path(data)?;
    if normalized == normalize_instance_path(default_data)? {
        return Ok(DEFAULT_INSTANCE_IDENTIFIER.to_owned());
    }
    let digest = Sha256::digest(normalized.as_bytes());
    Ok(format!("{DEFAULT_INSTANCE_IDENTIFIER}.scope-{digest:x}"))
}

fn normalize_instance_path(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("应用数据目录必须为绝对路径。".into());
    }
    // This is lexical: instance identity must be stable before and after the
    // data directory is created, without reading configuration or credentials.
    let absolute = std::path::absolute(path).map_err(|_| "无法解析应用数据目录。")?;
    #[cfg(windows)]
    {
        let normalized = absolute.to_string_lossy().replace('/', "\\").to_lowercase();
        let normalized = if let Some(unc) = normalized.strip_prefix("\\\\?\\unc\\") {
            format!("\\\\{unc}")
        } else {
            normalized
                .strip_prefix("\\\\?\\")
                .unwrap_or(&normalized)
                .to_owned()
        };
        Ok(normalized.trim_end_matches('\\').to_owned())
    }
    #[cfg(not(windows))]
    {
        Ok(absolute.to_string_lossy().trim_end_matches('/').to_owned())
    }
}

#[cfg(test)]
mod instance_tests {
    use super::*;

    #[test]
    fn default_data_directory_retains_installed_instance_identifier() {
        let directory = tempfile::tempdir().unwrap();
        let default_data = directory.path().join("Vela").join("data");
        assert_eq!(
            instance_identifier_for(&default_data, &default_data).unwrap(),
            DEFAULT_INSTANCE_IDENTIFIER
        );
    }

    #[test]
    fn custom_data_directories_have_distinct_stable_instance_identifiers() {
        let directory = tempfile::tempdir().unwrap();
        let default_data = directory.path().join("default");
        let first = directory.path().join("one");
        let second = directory.path().join("two");
        let identifier = instance_identifier_for(&first, &default_data).unwrap();
        assert_ne!(identifier, DEFAULT_INSTANCE_IDENTIFIER);
        assert_ne!(
            identifier,
            instance_identifier_for(&second, &default_data).unwrap()
        );
        assert_eq!(
            identifier,
            instance_identifier_for(&first, &default_data).unwrap()
        );
        assert!(identifier.starts_with("app.vela.desktop.scope-"));
        assert!(!identifier.contains(&directory.path().to_string_lossy().to_string()));
    }

    #[test]
    fn creating_data_directory_does_not_change_its_instance_identifier() {
        let directory = tempfile::tempdir().unwrap();
        let default_data = directory.path().join("default");
        let data = directory.path().join("new").join("data");
        let before = instance_identifier_for(&data, &default_data).unwrap();
        assert!(!data.exists());
        fs::create_dir_all(&data).unwrap();
        assert_eq!(
            before,
            instance_identifier_for(&data, &default_data).unwrap()
        );
    }

    #[test]
    fn relative_data_directories_are_rejected_for_instance_identity() {
        let directory = tempfile::tempdir().unwrap();
        assert!(instance_identifier_for(Path::new("relative"), directory.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_variants_share_the_same_instance_identifier() {
        let default_data = Path::new(r"C:\Users\Example\AppData\Local\Vela\data");
        let default_variant = Path::new("c:/users/EXAMPLE/appdata/local/vela/./data/");
        assert_eq!(
            instance_identifier_for(default_variant, default_data).unwrap(),
            DEFAULT_INSTANCE_IDENTIFIER
        );
        let data = Path::new(r"D:\Vela Smoke\scope\data");
        let expected = instance_identifier_for(data, default_data).unwrap();
        for variant in [
            "d:/VELA SMOKE/SCOPE/data/",
            r"D:\Vela Smoke\scope\data\",
            r"D:\Vela Smoke\scope\unused\..\data",
            r"\\?\D:\Vela Smoke\scope\data",
        ] {
            assert_eq!(
                instance_identifier_for(Path::new(variant), default_data).unwrap(),
                expected,
                "{variant}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_unc_prefix_and_separators_share_the_same_instance_identifier() {
        let default_data = Path::new(r"C:\Users\Example\AppData\Local\Vela\data");
        let data = Path::new(r"\\server\share\Vela\data");
        let variant = Path::new(r"\\?\UNC\SERVER\SHARE\vela\data\");
        assert_eq!(
            instance_identifier_for(data, default_data).unwrap(),
            instance_identifier_for(variant, default_data).unwrap()
        );
    }
}
