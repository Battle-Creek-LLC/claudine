use std::process::Command;

use crate::{config, project};

/// Generate devcontainer.json content for a project.
pub fn generate(project: &str, repo: Option<&str>) -> anyhow::Result<String> {
    let project_config = config::load_project(project)?;
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    let name = match repo {
        Some(r) => format!("{}_{}", project::container_name(project), r),
        None => project::container_name(project),
    };

    let host_dir = project_config.host_dir
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            project::default_host_dir(project)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    let workspace_folder = match repo {
        Some(r) => format!("{}/{}", host_dir, r),
        None => host_dir.clone(),
    };
    let json = serde_json::json!({
        "name": name,
        "image": image,
        "overrideCommand": true,
        "remoteUser": "claude",
        "workspaceMount": format!("source={host_dir},target={host_dir},type=bind"),
        "workspaceFolder": workspace_folder,
        "mounts": [
            format!(
                "source={},target=/home/claude,type=volume",
                project::home_volume_name(project)
            ),
            "source=/var/run/docker.sock,target=/var/run/docker.sock,type=bind"
        ],
        "runArgs": ["--shm-size=256m"],
        "containerEnv": {
            "HOME": "/home/claude"
        }
    });

    serde_json::to_string_pretty(&json)
        .map_err(|e| anyhow::anyhow!("Failed to serialize devcontainer.json: {e}"))
}

/// Return the base directory for devcontainer output.
fn devcontainer_base(project: &str, repo: Option<&str>) -> anyhow::Result<std::path::PathBuf> {
    let project_config = config::load_project(project)?;
    let base = match &project_config.host_dir {
        Some(dir) => std::path::PathBuf::from(dir),
        None => project::default_host_dir(project)?,
    };

    if !base.exists() {
        anyhow::bail!(
            "Project directory does not exist: {}. Run 'claudine init {}' first.",
            base.display(),
            project
        );
    }
    match repo {
        Some(r) => Ok(base.join(r)),
        None => Ok(base),
    }
}

/// Write devcontainer.json into the project's share directory.
/// Returns the path to the written file.
pub fn write(project: &str, repo: Option<&str>) -> anyhow::Result<std::path::PathBuf> {
    let base = devcontainer_base(project, repo)?;

    let devcontainer_dir = base.join(".devcontainer");
    std::fs::create_dir_all(&devcontainer_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create .devcontainer directory: {e}"))?;

    let path = devcontainer_dir.join("devcontainer.json");
    let content = generate(project, repo)?;
    std::fs::write(&path, &content)
        .map_err(|e| anyhow::anyhow!("Failed to write devcontainer.json: {e}"))?;

    Ok(path)
}

/// Generate devcontainer.json and open the project in Zed.
pub fn cmd_zed(project: &str, repo: Option<&str>) -> anyhow::Result<()> {
    let path = write(project, repo)?;
    println!("Generated {}", path.display());

    let base = devcontainer_base(project, repo)?;

    match which::which("zed") {
        Ok(_) => {
            // Spawn Zed and return immediately — Zed is a long-running GUI.
            Command::new("zed")
                .arg(&base)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .spawn()
                .map_err(|e| anyhow::anyhow!("Failed to launch Zed: {e}"))?;
            println!("Opened {} in Zed.", base.display());
        }
        Err(_) => {
            println!("Zed not found on PATH. Open this directory in Zed:");
            println!("  zed {}", base.display());
        }
    }

    Ok(())
}
