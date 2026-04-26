use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct GlobalConfig {
    pub image: ImageConfig,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProjectConfig {
    pub repos: Vec<RepoConfig>,
    pub ssh_key: Option<String>,
    pub layers: Option<Vec<String>>,
    pub image: Option<ImageConfig>,
    /// Host directory bind-mounted at the same absolute path inside the container.
    /// Defaults to `~/projects/<name>/` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_dir: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct RepoConfig {
    pub url: String,
    pub dir: String,
    pub branch: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageConfig {
    pub name: String,
}

/// Legacy config format for migration support.
#[derive(Deserialize)]
struct LegacyProjectConfig {
    project: LegacyProjectInfo,
    image: Option<ImageConfig>,
}

#[derive(Deserialize)]
struct LegacyProjectInfo {
    repo_url: String,
    branch: Option<String>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            image: ImageConfig {
                name: "claudine:latest".to_string(),
            },
        }
    }
}

/// Extract a directory name from a git URL.
///
/// Strips `.git` suffix and takes the last path segment.
///
/// Examples:
/// - `git@github.com:acme/frontend.git` -> `frontend`
/// - `https://github.com/acme/backend.git` -> `backend`
/// - `https://github.com/acme/my.dotted.repo.git` -> `my.dotted.repo`
pub fn repo_dir_from_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');

    // Strip .git suffix
    let url = url.strip_suffix(".git").unwrap_or(url);

    // For SSH URLs like git@github.com:acme/frontend, split on ':' then '/'
    // For HTTPS URLs like https://github.com/acme/backend, split on '/'
    let last_segment = if let Some(after_colon) = url.rsplit_once(':') {
        // SSH-style URL — take the part after the colon, then the last path segment
        after_colon
            .1
            .rsplit_once('/')
            .map(|(_, name)| name)
            .unwrap_or(after_colon.1)
    } else {
        // HTTPS-style or plain path — take the last path segment
        url.rsplit_once('/')
            .map(|(_, name)| name)
            .unwrap_or(url)
    };

    let result = last_segment.to_string();

    // Guard against empty or dangerous directory names
    if result.is_empty() || result == "." || result == ".." {
        return "repo".to_string();
    }

    result
}

/// Attempt to migrate a legacy `[project]` format config to the new `[[repos]]` format.
fn migrate_project_config(raw: &str) -> Option<ProjectConfig> {
    let legacy: LegacyProjectConfig = toml::from_str(raw).ok()?;
    let dir = repo_dir_from_url(&legacy.project.repo_url);

    Some(ProjectConfig {
        repos: vec![RepoConfig {
            url: legacy.project.repo_url,
            dir,
            branch: legacy.project.branch,
        }],
        ssh_key: None,
        layers: None,
        image: legacy.image,
        host_dir: None,
    })
}

/// Create a directory with 700 permissions (owner only).
fn create_private_dir(path: &std::path::Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("Failed to set permissions on: {}", path.display()))?;
    Ok(())
}

/// Write a file with 600 permissions (owner read/write only).
fn write_private_file(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    fs::write(path, content)
        .with_context(|| format!("Failed to write: {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on: {}", path.display()))?;
    Ok(())
}

/// Return the base configuration directory.
///
/// Resolves via `dirs::config_dir()`, so on macOS this is
/// `~/Library/Application Support/claudine/` and on Linux it is
/// `~/.config/claudine/`.
pub fn config_dir() -> anyhow::Result<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine user config directory"))?;
    Ok(base.join("claudine"))
}

/// Return the directory that holds layer source checkouts.
///
/// Layers with a `source_repo` are cloned into `sources/<layer-name>/` here and
/// staged into Docker build contexts on demand.
pub fn sources_dir() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("sources"))
}

/// Load the global config from `~/.config/claudine/config.toml`.
/// Creates the config directory and a default config file if they do not exist.
pub fn load_global() -> anyhow::Result<GlobalConfig> {
    let dir = config_dir()?;
    let path = dir.join("config.toml");

    if !path.exists() {
        create_private_dir(&dir)?;

        let default = GlobalConfig::default();
        let content = toml::to_string_pretty(&default)
            .context("Failed to serialize default global config")?;
        write_private_file(&path, &content)?;

        return Ok(default);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let config: GlobalConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config: {}", path.display()))?;

    Ok(config)
}

/// Load a project config from `~/.config/claudine/projects/<name>/config.toml`.
///
/// Tries the new `[[repos]]` format first. If that fails, attempts to migrate
/// from the legacy `[project]` format. On successful migration, saves the config
/// in the new format.
pub fn load_project(name: &str) -> anyhow::Result<ProjectConfig> {
    let path = config_dir()?.join("projects").join(name).join("config.toml");

    if !path.exists() {
        anyhow::bail!(
            "Project '{}' not found. Run 'claudine init {}' first.",
            name,
            name
        );
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read project config: {}", path.display()))?;

    // Try the new format first
    if let Ok(config) = toml::from_str::<ProjectConfig>(&content) {
        return Ok(config);
    }

    // Fall back to legacy migration
    if let Some(migrated) = migrate_project_config(&content) {
        // Save the migrated config so future loads use the new format
        save_project(name, &migrated)?;
        return Ok(migrated);
    }

    anyhow::bail!("Failed to parse project config: {}", path.display());
}

/// Save a project config to `~/.config/claudine/projects/<name>/config.toml`.
/// Creates the project directory if it does not exist.
pub fn save_project(name: &str, config: &ProjectConfig) -> anyhow::Result<()> {
    let dir = config_dir()?.join("projects").join(name);
    create_private_dir(&dir)?;

    let path = dir.join("config.toml");
    let content =
        toml::to_string_pretty(config).context("Failed to serialize project config")?;
    write_private_file(&path, &content)?;

    Ok(())
}

/// List all project names by reading subdirectories of `~/.config/claudine/projects/`.
pub fn list_projects() -> anyhow::Result<Vec<String>> {
    let dir = config_dir()?.join("projects");

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();
    let entries = fs::read_dir(&dir)
        .with_context(|| format!("Failed to read projects directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                // Only include directories that contain a config.toml
                if entry.path().join("config.toml").exists() {
                    projects.push(name.to_string());
                }
            }
        }
    }

    projects.sort();
    Ok(projects)
}

/// Resolve the Docker image name for a project. The project-level image config
/// takes precedence over the global default.
pub fn resolve_image(project_config: &ProjectConfig, global_config: &GlobalConfig) -> String {
    match &project_config.image {
        Some(img) => img.name.clone(),
        None => global_config.image.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_global_config() {
        let config = GlobalConfig::default();
        assert_eq!(config.image.name, "claudine:latest");
    }

    #[test]
    fn resolve_image_uses_project_override() {
        let global = GlobalConfig::default();
        let project = ProjectConfig {
            repos: vec![RepoConfig {
                url: "https://example.com/repo.git".to_string(),
                dir: "repo".to_string(),
                branch: None,
            }],
            ssh_key: None,
            layers: None,
            image: Some(ImageConfig {
                name: "custom:latest".to_string(),
            }),
            host_dir: None,
        };
        assert_eq!(resolve_image(&project, &global), "custom:latest");
    }

    #[test]
    fn resolve_image_falls_back_to_global() {
        let global = GlobalConfig::default();
        let project = ProjectConfig {
            repos: vec![RepoConfig {
                url: "https://example.com/repo.git".to_string(),
                dir: "repo".to_string(),
                branch: None,
            }],
            ssh_key: None,
            layers: None,
            image: None,
            host_dir: None,
        };
        assert_eq!(resolve_image(&project, &global), "claudine:latest");
    }

    #[test]
    fn project_config_roundtrip() {
        let config = ProjectConfig {
            repos: vec![
                RepoConfig {
                    url: "git@github.com:user/repo.git".to_string(),
                    dir: "repo".to_string(),
                    branch: Some("main".to_string()),
                },
                RepoConfig {
                    url: "https://github.com/user/backend.git".to_string(),
                    dir: "backend".to_string(),
                    branch: None,
                },
            ],
            ssh_key: Some("/Users/test/.ssh/id_ed25519".to_string()),
            layers: None,
            image: None,
            host_dir: None,
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: ProjectConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.repos.len(), 2);
        assert_eq!(deserialized.repos[0].url, "git@github.com:user/repo.git");
        assert_eq!(deserialized.repos[0].dir, "repo");
        assert_eq!(deserialized.repos[0].branch.as_deref(), Some("main"));
        assert_eq!(
            deserialized.repos[1].url,
            "https://github.com/user/backend.git"
        );
        assert_eq!(deserialized.repos[1].dir, "backend");
        assert!(deserialized.repos[1].branch.is_none());
        assert!(deserialized.image.is_none());
    }

    #[test]
    fn repo_dir_from_ssh_url() {
        assert_eq!(
            repo_dir_from_url("git@github.com:acme/frontend.git"),
            "frontend"
        );
    }

    #[test]
    fn repo_dir_from_https_url() {
        assert_eq!(
            repo_dir_from_url("https://github.com/acme/backend.git"),
            "backend"
        );
    }

    #[test]
    fn repo_dir_from_dotted_url() {
        assert_eq!(
            repo_dir_from_url("https://github.com/acme/my.dotted.repo.git"),
            "my.dotted.repo"
        );
    }

    #[test]
    fn repo_dir_from_url_no_git_suffix() {
        assert_eq!(
            repo_dir_from_url("https://github.com/acme/tools"),
            "tools"
        );
    }

    #[test]
    fn repo_dir_from_url_trailing_slash() {
        assert_eq!(
            repo_dir_from_url("https://github.com/acme/tools.git/"),
            "tools"
        );
    }

    #[test]
    fn migrate_legacy_config() {
        let legacy = r#"
[project]
repo_url = "git@github.com:user/myrepo.git"
branch = "develop"
"#;
        let migrated = migrate_project_config(legacy).unwrap();
        assert_eq!(migrated.repos.len(), 1);
        assert_eq!(migrated.repos[0].url, "git@github.com:user/myrepo.git");
        assert_eq!(migrated.repos[0].dir, "myrepo");
        assert_eq!(migrated.repos[0].branch.as_deref(), Some("develop"));
        assert!(migrated.image.is_none());
    }

    #[test]
    fn migrate_legacy_config_with_image() {
        let legacy = r#"
[project]
repo_url = "https://github.com/org/app.git"

[image]
name = "custom:v2"
"#;
        let migrated = migrate_project_config(legacy).unwrap();
        assert_eq!(migrated.repos.len(), 1);
        assert_eq!(migrated.repos[0].url, "https://github.com/org/app.git");
        assert_eq!(migrated.repos[0].dir, "app");
        assert!(migrated.repos[0].branch.is_none());
        assert_eq!(migrated.image.unwrap().name, "custom:v2");
    }

    #[test]
    fn migrate_returns_none_for_invalid() {
        let bad = r#"
[something_else]
key = "value"
"#;
        assert!(migrate_project_config(bad).is_none());
    }
}
