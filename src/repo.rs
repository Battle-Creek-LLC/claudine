use std::process::Command;

use dialoguer::Confirm;

use crate::cli::RepoCommand;
use crate::{config, init, project};

/// Handle repo subcommands: add, remove, list.
pub fn cmd_repo(command: RepoCommand) -> anyhow::Result<()> {
    match command {
        RepoCommand::Add {
            project: proj,
            url,
            dir,
            branch,
        } => repo_add(&proj, &url, dir, branch),
        RepoCommand::Remove {
            project: proj,
            dir,
        } => repo_remove(&proj, &dir),
        RepoCommand::List { project: proj } => repo_list(&proj),
    }
}

/// Add a repository to an existing project.
fn repo_add(
    project_name: &str,
    url: &str,
    dir: Option<String>,
    branch: Option<String>,
) -> anyhow::Result<()> {
    project::validate_name(project_name)?;

    if !project::volume_exists(project_name)? {
        anyhow::bail!(
            "Project '{}' is not initialized. Run 'claudine init {}' first.",
            project_name,
            project_name
        );
    }

    let mut project_config = config::load_project(project_name)?;
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    let dir = dir.unwrap_or_else(|| config::repo_dir_from_url(url));

    // Check for directory name conflicts
    if project_config.repos.iter().any(|r| r.dir == dir) {
        anyhow::bail!(
            "A repository with directory name '{}' already exists in project '{}'.",
            dir,
            project_name
        );
    }

    let repo = config::RepoConfig {
        url: url.to_string(),
        dir: dir.clone(),
        branch,
    };

    // Clone the repo into the volume
    init::clone_repo(project_name, &image, &repo)?;

    // Update and save config
    project_config.repos.push(repo);
    config::save_project(project_name, &project_config)?;

    println!("Repository '{}' added to project '{}'.", dir, project_name);
    Ok(())
}

/// Remove a repository from an existing project.
fn repo_remove(project_name: &str, dir: &str) -> anyhow::Result<()> {
    project::validate_name(project_name)?;

    let mut project_config = config::load_project(project_name)?;

    let repo_index = project_config
        .repos
        .iter()
        .position(|r| r.dir == dir)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No repository with directory '{}' found in project '{}'.",
                dir,
                project_name
            )
        })?;

    let confirmed = Confirm::new()
        .with_prompt(format!(
            "Remove directory '{}' from volume?",
            dir
        ))
        .default(false)
        .interact()?;

    if !confirmed {
        anyhow::bail!("Remove cancelled.");
    }

    // Remove the directory from the volume via a docker run --rm command
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    let rm_target = format!("/project/{}", dir);
    let status = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/project", project::volume_name(project_name)),
            &image,
            "rm",
            "-rf",
            &rm_target,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker run' for removal: {e}"))?;

    if !status.success() {
        eprintln!(
            "Warning: failed to remove directory '{}' from volume (exit code: {}).",
            dir, status
        );
    }

    // Update and save config
    project_config.repos.remove(repo_index);
    config::save_project(project_name, &project_config)?;

    println!(
        "Repository '{}' removed from project '{}'.",
        dir, project_name
    );
    Ok(())
}

/// List all repositories in a project.
fn repo_list(project_name: &str) -> anyhow::Result<()> {
    project::validate_name(project_name)?;

    let project_config = config::load_project(project_name)?;

    if project_config.repos.is_empty() {
        println!("No repositories configured for project '{}'.", project_name);
        return Ok(());
    }

    // Calculate column widths
    let name_width = project_config
        .repos
        .iter()
        .map(|r| r.dir.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let url_width = project_config
        .repos
        .iter()
        .map(|r| r.url.len())
        .max()
        .unwrap_or(4)
        .max(4);

    // Print header
    println!(
        "{:<name_w$}  {:<url_w$}  BRANCH",
        "NAME", "REPO",
        name_w = name_width,
        url_w = url_width,
    );

    // Print rows
    for repo in &project_config.repos {
        let branch = repo.branch.as_deref().unwrap_or("-");
        println!(
            "{:<name_w$}  {:<url_w$}  {}",
            repo.dir, repo.url, branch,
            name_w = name_width,
            url_w = url_width,
        );
    }

    Ok(())
}
