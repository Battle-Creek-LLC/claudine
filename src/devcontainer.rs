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

    let workspace_folder = match repo {
        Some(r) => format!("/project/{}", r),
        None => "/project".to_string(),
    };

    let json = serde_json::json!({
        "name": name,
        "image": image,
        "overrideCommand": true,
        "remoteUser": "claude",
        "workspaceMount": format!(
            "source={},target=/project,type=volume",
            project::volume_name(project)
        ),
        "workspaceFolder": workspace_folder,
        "mounts": [
            "source=/var/run/docker.sock,target=/var/run/docker.sock,type=bind"
        ],
        "runArgs": ["--shm-size=256m"],
        "containerEnv": {
            "HOME": "/project/home"
        }
    });

    serde_json::to_string_pretty(&json)
        .map_err(|e| anyhow::anyhow!("Failed to serialize devcontainer.json: {e}"))
}

/// Return the base directory for devcontainer output.
/// With a repo, returns ~/share/<project>/<repo>/;
/// without, returns ~/share/<project>/.
fn devcontainer_base(project: &str, repo: Option<&str>) -> anyhow::Result<std::path::PathBuf> {
    let share = project::share_dir(project)?;
    if !share.exists() {
        anyhow::bail!(
            "Share directory does not exist: {}. Run 'claudine init {}' first.",
            share.display(),
            project
        );
    }
    match repo {
        Some(r) => Ok(share.join(r)),
        None => Ok(share),
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
            let status = Command::new("zed")
                .arg(&base)
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to launch Zed: {e}"))?;

            if !status.success() {
                eprintln!("Zed exited with non-zero status. Open manually:");
                println!("  zed {}", base.display());
            }
        }
        Err(_) => {
            println!("Zed not found on PATH. Open this directory in Zed:");
            println!("  zed {}", base.display());
        }
    }

    Ok(())
}
