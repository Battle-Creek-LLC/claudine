use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct GlobalConfig {
    pub image: ImageConfig,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProjectConfig {
    pub project: ProjectInfo,
    pub image: Option<ImageConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageConfig {
    pub name: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProjectInfo {
    pub repo_url: String,
    pub branch: Option<String>,
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

/// Return the base configuration directory: `~/.config/claudine/`.
pub fn config_dir() -> anyhow::Result<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine user config directory"))?;
    Ok(base.join("claudine"))
}

/// Load the global config from `~/.config/claudine/config.toml`.
/// Creates the config directory and a default config file if they do not exist.
pub fn load_global() -> anyhow::Result<GlobalConfig> {
    let dir = config_dir()?;
    let path = dir.join("config.toml");

    if !path.exists() {
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

        let default = GlobalConfig::default();
        let content = toml::to_string_pretty(&default)
            .context("Failed to serialize default global config")?;
        fs::write(&path, &content)
            .with_context(|| format!("Failed to write default config: {}", path.display()))?;

        return Ok(default);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let config: GlobalConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config: {}", path.display()))?;

    Ok(config)
}

/// Load a project config from `~/.config/claudine/projects/<name>/config.toml`.
/// Returns an error if the project config does not exist.
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
    let config: ProjectConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse project config: {}", path.display()))?;

    Ok(config)
}

/// Save a project config to `~/.config/claudine/projects/<name>/config.toml`.
/// Creates the project directory if it does not exist.
pub fn save_project(name: &str, config: &ProjectConfig) -> anyhow::Result<()> {
    let dir = config_dir()?.join("projects").join(name);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create project directory: {}", dir.display()))?;

    let path = dir.join("config.toml");
    let content =
        toml::to_string_pretty(config).context("Failed to serialize project config")?;
    fs::write(&path, &content)
        .with_context(|| format!("Failed to write project config: {}", path.display()))?;

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
            project: ProjectInfo {
                repo_url: "https://example.com/repo.git".to_string(),
                branch: None,
            },
            image: Some(ImageConfig {
                name: "custom:latest".to_string(),
            }),
        };
        assert_eq!(resolve_image(&project, &global), "custom:latest");
    }

    #[test]
    fn resolve_image_falls_back_to_global() {
        let global = GlobalConfig::default();
        let project = ProjectConfig {
            project: ProjectInfo {
                repo_url: "https://example.com/repo.git".to_string(),
                branch: None,
            },
            image: None,
        };
        assert_eq!(resolve_image(&project, &global), "claudine:latest");
    }

    #[test]
    fn project_config_roundtrip() {
        let config = ProjectConfig {
            project: ProjectInfo {
                repo_url: "git@github.com:user/repo.git".to_string(),
                branch: Some("main".to_string()),
            },
            image: None,
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: ProjectConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.project.repo_url, "git@github.com:user/repo.git");
        assert_eq!(deserialized.project.branch.as_deref(), Some("main"));
        assert!(deserialized.image.is_none());
    }
}
