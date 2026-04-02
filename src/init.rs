use std::path::PathBuf;
use std::process::Command;

use dialoguer::{Confirm, Input};

use crate::{config, project};

/// Run the interactive project initialization flow.
///
/// Prompts the user for a repository URL and optional branch, creates a Docker
/// volume, saves the project config, and clones the repository into the volume.
pub fn cmd_init(name: &str) -> anyhow::Result<()> {
    project::validate_name(name)?;

    // Check if volume already exists
    let volume_already_exists = project::volume_exists(name)?;
    if volume_already_exists {
        let proceed = Confirm::new()
            .with_prompt(format!(
                "Volume already exists for '{}'. Re-initialize? This will not delete existing data.",
                name
            ))
            .default(false)
            .interact()?;

        if !proceed {
            anyhow::bail!("Init cancelled.");
        }
    }

    // Prompt for repository URL (required)
    let repo_url: String = Input::new()
        .with_prompt("Repository URL")
        .interact_text()?;

    if repo_url.trim().is_empty() {
        anyhow::bail!("Repository URL cannot be empty.");
    }

    // Prompt for branch (optional)
    let branch_input: String = Input::new()
        .with_prompt("Branch (leave empty for default)")
        .default(String::new())
        .show_default(false)
        .interact_text()?;

    let branch = if branch_input.trim().is_empty() {
        None
    } else {
        Some(branch_input.trim().to_string())
    };

    // Create volume if it does not already exist
    if !volume_already_exists {
        println!("Creating volume '{}'...", project::volume_name(name));
        project::create_volume(name)?;
    }

    // Build and save project config
    let project_config = config::ProjectConfig {
        project: config::ProjectInfo {
            repo_url: repo_url.clone(),
            branch: branch.clone(),
        },
        image: None,
    };
    config::save_project(name, &project_config)?;

    // Resolve the image name from global config
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    // Build docker run args for the clone operation
    let mut args: Vec<String> = vec![
        "run".to_string(),
        "--rm".to_string(),
        "-v".to_string(),
        format!("{}:/workspace", project::volume_name(name)),
    ];

    // Mount host gitconfig if it exists
    if let Some(gitconfig_path) = host_gitconfig_path() {
        if gitconfig_path.exists() {
            args.push("-v".to_string());
            args.push(format!(
                "{}:/host-config/gitconfig:ro",
                gitconfig_path.display()
            ));
        }
    }

    // Mount host SSH directory if it exists
    if let Some(ssh_path) = host_ssh_path() {
        if ssh_path.exists() {
            args.push("-v".to_string());
            args.push(format!("{}:/host-config/ssh:ro", ssh_path.display()));
        }
    }

    // Image name
    args.push(image);

    // Clone command
    let mut clone_cmd = vec!["git".to_string(), "clone".to_string()];
    if let Some(ref b) = branch {
        clone_cmd.push("--branch".to_string());
        clone_cmd.push(b.clone());
    }
    clone_cmd.push(repo_url);
    clone_cmd.push("/workspace/project".to_string());

    args.extend(clone_cmd);

    // Run the clone
    println!("Cloning repository into volume...");
    let status = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker run' for clone: {e}"))?;

    if !status.success() {
        eprintln!(
            "Clone failed (exit code: {}). The volume has been kept — \
             you can fix the issue and run 'claudine init {}' again.",
            status,
            name
        );
        anyhow::bail!("Repository clone failed.");
    }

    println!("Project '{}' initialized successfully.", name);
    Ok(())
}

/// Return the path to the host's ~/.gitconfig file, if the home directory is known.
fn host_gitconfig_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".gitconfig"))
}

/// Return the path to the host's ~/.ssh directory, if the home directory is known.
fn host_ssh_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh"))
}
