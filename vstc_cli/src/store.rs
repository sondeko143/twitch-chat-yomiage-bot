//! Where `profiles.toml` lives, and how it is read and written.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

use crate::profile::ProfileStore;

/// Overrides the directory holding `profiles.toml`.
/// Mirrors tcyb's `TCYB_CONFIG_DIR` so both crates behave the same way.
const CONFIG_DIR_ENV: &str = "VSTC_CONFIG_DIR";
/// Name of the single file holding every profile (ADR-0015).
const FILE_NAME: &str = "profiles.toml";

/// Pure path composition, split out so both branches are testable without
/// touching the real user directories.
fn profiles_path_from(
    base_override: Option<PathBuf>,
    proj: Option<ProjectDirs>,
) -> Result<PathBuf> {
    if let Some(base) = base_override {
        return Ok(base.join(FILE_NAME));
    }
    let proj = proj.context("OS のユーザーディレクトリを解決できませんでした")?;
    Ok(proj.config_dir().join(FILE_NAME))
}

/// Absolute path of `profiles.toml` on this machine.
///
/// ## Errors
///
/// Fails when `VSTC_CONFIG_DIR` is unset and the OS user directories cannot be
/// resolved.
pub fn profiles_path() -> Result<PathBuf> {
    let base = std::env::var_os(CONFIG_DIR_ENV).map(PathBuf::from);
    profiles_path_from(base, ProjectDirs::from("", "", "vstc"))
}

/// Read the store. A missing file means "no profiles yet", not an error, so
/// every command works on a machine that has never run `profile set`.
///
/// ## Errors
///
/// Fails when the file exists but cannot be read or is not valid TOML.
pub fn load(path: &Path) -> Result<ProfileStore> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ProfileStore::default()),
        Err(e) => return Err(e).with_context(|| format!("{} を読めませんでした", path.display())),
    };
    toml::from_str(&text)
        .with_context(|| format!("{} の TOML を解析できませんでした", path.display()))
}

/// Write the store atomically: serialize into a sibling temp file, then rename
/// it over the target. An interrupted write therefore cannot truncate the file
/// and lose every saved profile (ADR-0015).
///
/// ## Errors
///
/// Fails when the directory cannot be created, or the file cannot be written or
/// renamed into place.
pub fn save(path: &Path, store: &ProfileStore) -> Result<()> {
    let dir = path
        .parent()
        .context("プロファイルの保存先ディレクトリを決定できませんでした")?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("{} を作成できませんでした", dir.display()))?;
    let text = toml::to_string(store).context("プロファイルを TOML へ変換できませんでした")?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)
        .with_context(|| format!("{} へ書き込めませんでした", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("{} へ反映できませんでした", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    #[test]
    fn override_dir_places_the_file_under_base() {
        let base = PathBuf::from("base-dir");
        let got =
            profiles_path_from(Some(base.clone()), None).expect("override needs no ProjectDirs");
        assert_eq!(got, base.join("profiles.toml"));
    }

    #[test]
    fn missing_project_dirs_without_override_errors() {
        assert!(profiles_path_from(None, None).is_err());
    }

    #[test]
    fn load_returns_empty_store_when_file_is_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let got = load(&dir.path().join("profiles.toml")).expect("absent file is not an error");
        assert!(got.profiles.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(19829),
                config_path: Some("c.yml".to_string()),
            },
        );
        save(&path, &store).expect("save");
        assert_eq!(load(&path).expect("load"), store);
    }

    #[test]
    fn save_creates_the_config_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir
            .path()
            .join("nested")
            .join("deeper")
            .join("profiles.toml");
        save(&path, &ProfileStore::default()).expect("save should create parents");
        assert!(path.exists());
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        save(&path, &ProfileStore::default()).expect("save");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
    }

    #[test]
    fn save_over_existing_file_replaces_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        let mut first = ProfileStore::default();
        first.merge(
            "old",
            &Profile {
                host: Some("h".to_string()),
                ..Profile::default()
            },
        );
        save(&path, &first).expect("first save");
        let mut second = ProfileStore::default();
        second.merge(
            "new",
            &Profile {
                host: Some("h2".to_string()),
                ..Profile::default()
            },
        );
        save(&path, &second).expect("second save");
        let got = load(&path).expect("load");
        assert_eq!(got, second, "rename must replace the existing file");
    }

    #[test]
    fn load_reports_the_path_when_the_file_is_not_valid_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        std::fs::write(&path, "this is not = = toml").expect("write");
        let err = load(&path).expect_err("broken toml should error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("profiles.toml"),
            "error should name the file: {msg}"
        );
    }
}
