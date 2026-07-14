use anyhow::Context;
use directories::ProjectDirs;
use std::path::PathBuf;

const APP_DIR_ENV: &str = "TCYB_CONFIG_DIR";

pub struct AppPaths {
    pub config_file: PathBuf,
    pub db_dir: PathBuf,
}

fn app_paths_from(
    base_override: Option<PathBuf>,
    proj: Option<ProjectDirs>,
) -> anyhow::Result<AppPaths> {
    if let Some(base) = base_override {
        return Ok(AppPaths {
            config_file: base.join("config.toml"),
            db_dir: base.join("db"),
        });
    }
    let proj = proj.context("OS のユーザーディレクトリを解決できませんでした")?;
    Ok(AppPaths {
        config_file: proj.config_dir().join("config.toml"),
        db_dir: proj.data_dir().to_path_buf(),
    })
}

pub fn app_paths() -> anyhow::Result<AppPaths> {
    let base = std::env::var_os(APP_DIR_ENV).map(PathBuf::from);
    app_paths_from(base, ProjectDirs::from("", "", "tcyb"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_dir_places_config_and_db_under_base() {
        let base = PathBuf::from("/tmp/tcyb-base");
        let p = app_paths_from(Some(base.clone()), None).unwrap();
        assert_eq!(p.config_file, base.join("config.toml"));
        assert_eq!(p.db_dir, base.join("db"));
    }

    #[test]
    fn missing_project_dirs_without_override_errors() {
        assert!(app_paths_from(None, None).is_err());
    }
}
