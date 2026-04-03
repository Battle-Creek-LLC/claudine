use std::path::PathBuf;
use std::process::Command;

use dialoguer::{Confirm, Input};

use crate::{config, project};

/// Run the interactive project initialization flow.
///
/// Prompts the user for one or more repository URLs, creates a Docker volume,
/// saves the project config, and clones each repository into the volume.
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

    // Prompt for SSH key
    let ssh_key_input: String = Input::new()
        .with_prompt("SSH key path (leave empty for HTTPS repos)")
        .default(String::new())
        .show_default(false)
        .interact_text()?;

    let ssh_key = if ssh_key_input.trim().is_empty() {
        None
    } else {
        let path = PathBuf::from(ssh_key_input.trim());
        if !path.exists() {
            anyhow::bail!("SSH key not found: {}", path.display());
        }
        Some(path.display().to_string())
    };

    // Collect repos in a loop
    let mut repos: Vec<config::RepoConfig> = Vec::new();

    loop {
        let url_input: String = Input::new()
            .with_prompt("Repository URL (leave empty to finish)")
            .default(String::new())
            .show_default(false)
            .interact_text()?;

        if url_input.trim().is_empty() {
            if repos.is_empty() {
                anyhow::bail!("At least one repository is required.");
            }
            break;
        }

        let url = url_input.trim().to_string();
        let default_dir = config::repo_dir_from_url(&url);

        let dir_input: String = Input::new()
            .with_prompt(format!("Directory name [{}]", default_dir))
            .default(default_dir.clone())
            .show_default(false)
            .interact_text()?;

        let dir = if dir_input.trim().is_empty() {
            default_dir
        } else {
            dir_input.trim().to_string()
        };

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

        repos.push(config::RepoConfig { url, dir, branch });
    }

    // Create volume if it does not already exist
    if !volume_already_exists {
        println!("Creating volume '{}'...", project::volume_name(name));
        project::create_volume(name)?;
    }

    // Build and save project config
    let project_config = config::ProjectConfig {
        repos: repos.clone(),
        ssh_key: ssh_key.clone(),
        image: None,
    };
    config::save_project(name, &project_config)?;

    // Resolve the image name from global config
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    // Clone each repo
    for repo in &repos {
        clone_repo(name, &image, repo, ssh_key.as_deref())?;
    }

    println!("Project '{}' initialized successfully.", name);
    Ok(())
}

/// Clone a single repository into the project volume.
pub fn clone_repo(
    project_name: &str,
    image: &str,
    repo: &config::RepoConfig,
    ssh_key: Option<&str>,
) -> anyhow::Result<()> {
    let mut args: Vec<String> = vec![
        "run".to_string(),
        "--rm".to_string(),
        "-v".to_string(),
        format!("{}:/workspace", project::volume_name(project_name)),
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

    // Mount the specific SSH key if configured
    if let Some(key_path) = ssh_key {
        args.push("-v".to_string());
        args.push(format!("{}:/host-config/ssh_key:ro", key_path));
    }

    // Image name
    args.push(image.to_string());

    // Clone command — clone into /workspace/project/<dir>
    let clone_target = format!("/workspace/project/{}", repo.dir);
    let mut clone_cmd = vec!["git".to_string(), "clone".to_string()];
    if let Some(ref b) = repo.branch {
        clone_cmd.push("--branch".to_string());
        clone_cmd.push(b.clone());
    }
    clone_cmd.push(repo.url.clone());
    clone_cmd.push(clone_target);

    args.extend(clone_cmd);

    // Run the clone
    println!("Cloning {}...", repo.dir);
    let status = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker run' for clone: {e}"))?;

    if !status.success() {
        eprintln!(
            "Clone of '{}' failed (exit code: {}). The volume has been kept — \
             you can fix the issue and try again.",
            repo.url, status,
        );
        anyhow::bail!("Repository clone failed for '{}'.", repo.url);
    }

    Ok(())
}

/// Return the path to the host's ~/.gitconfig file, if the home directory is known.
fn host_gitconfig_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".gitconfig"))
}
