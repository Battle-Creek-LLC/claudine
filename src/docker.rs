use std::io::{IsTerminal, Write};
use std::process::Command;

use dialoguer::Confirm;

use crate::{config, project};

const DOCKERFILE: &str = include_str!("../Dockerfile");
const ENTRYPOINT: &str = include_str!("../entrypoint.sh");

/// Verify that Docker is installed and the daemon is running.
pub fn check_docker() -> anyhow::Result<()> {
    which::which("docker")
        .map_err(|_| anyhow::anyhow!("Docker not found on PATH. Please install Docker first."))?;

    let status = Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker info': {e}"))?;

    if !status.success() {
        anyhow::bail!("Docker daemon is not running. Please start Docker and try again.");
    }

    Ok(())
}

/// Build the claudine Docker image.
pub fn cmd_build() -> anyhow::Result<()> {
    check_docker()?;

    let tmp = tempfile::tempdir()
        .map_err(|e| anyhow::anyhow!("Failed to create temporary directory: {e}"))?;

    // Write the Dockerfile into the temp directory
    let dockerfile_path = tmp.path().join("Dockerfile");
    let mut f = std::fs::File::create(&dockerfile_path)
        .map_err(|e| anyhow::anyhow!("Failed to write Dockerfile: {e}"))?;
    f.write_all(DOCKERFILE.as_bytes())?;

    // Write the entrypoint script into the temp directory
    let entrypoint_path = tmp.path().join("entrypoint.sh");
    let mut f = std::fs::File::create(&entrypoint_path)
        .map_err(|e| anyhow::anyhow!("Failed to write entrypoint.sh: {e}"))?;
    f.write_all(ENTRYPOINT.as_bytes())?;

    println!("Building claudine Docker image...");

    let status = Command::new("docker")
        .args(["build", "-t", "claudine:latest"])
        .arg(tmp.path())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker build': {e}"))?;

    if !status.success() {
        anyhow::bail!("Docker build failed with exit code: {}", status);
    }

    println!("Successfully built claudine:latest");
    Ok(())
}

/// Launch Claude Code in a container for the given project.
///
/// Validates the project state, assembles Docker run arguments, and replaces the
/// current process with the Docker container via `exec()`.
pub fn cmd_run(project: &str, extra_args: &[String]) -> anyhow::Result<()> {
    project::validate_name(project)?;

    if !project::volume_exists(project)? {
        anyhow::bail!(
            "Project '{}' is not initialized. Run 'claudine init {}' first.",
            project,
            project
        );
    }

    if project::container_running(project)? {
        anyhow::bail!(
            "Project '{}' is already running. Use 'claudine shell {}' to open another terminal.",
            project,
            project
        );
    }

    let project_config = config::load_project(project)?;
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    let docker_args = build_run_args(project, &image);

    // Build the full command: docker <run_args> <image> claude [extra_args...]
    let mut cmd = Command::new("docker");
    cmd.args(&docker_args);

    // Command to run inside the container
    cmd.arg("claude");
    cmd.args(extra_args);

    // Replace the current process with Docker. This call only returns on error.
    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    Err(anyhow::anyhow!("Failed to exec docker: {}", err))
}

/// Open an interactive shell in a project's container.
///
/// If the container is already running (e.g., Claude is active), attaches a new
/// bash session via `docker exec`. Otherwise, starts a fresh container with the
/// entrypoint's default bash shell.
pub fn cmd_shell(project: &str) -> anyhow::Result<()> {
    project::validate_name(project)?;

    if !project::volume_exists(project)? {
        anyhow::bail!(
            "Project '{}' is not initialized. Run 'claudine init {}' first.",
            project,
            project
        );
    }

    if project::container_running(project)? {
        // Attach to the running container via docker exec
        let mut cmd = Command::new("docker");
        cmd.arg("exec");

        if std::io::stdin().is_terminal() {
            cmd.arg("-it");
        }

        cmd.arg(project::container_name(project));
        cmd.arg("bash");

        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        return Err(anyhow::anyhow!("Failed to exec docker: {}", err));
    }

    // No running container — start a fresh one with bash (no command argument)
    let project_config = config::load_project(project)?;
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    let docker_args = build_run_args(project, &image);

    let mut cmd = Command::new("docker");
    cmd.args(&docker_args);
    // No command argument — the entrypoint defaults to bash when no args are given

    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    Err(anyhow::anyhow!("Failed to exec docker: {}", err))
}

/// Destroy a project by removing its container, volume, and configuration.
///
/// Prompts for confirmation before proceeding. Stops any running container,
/// removes the Docker volume, and deletes the project config directory.
pub fn cmd_destroy(project: &str) -> anyhow::Result<()> {
    project::validate_name(project)?;

    // Check that the project has some presence (config or volume)
    let has_volume = project::volume_exists(project)?;
    let config_dir = config::config_dir()?.join("projects").join(project);
    let has_config = config_dir.exists();

    if !has_volume && !has_config {
        anyhow::bail!(
            "No project '{}' found. Nothing to destroy.",
            project
        );
    }

    let confirmed = Confirm::new()
        .with_prompt(format!(
            "This will delete all data for '{}'. Continue?",
            project
        ))
        .default(false)
        .interact()?;

    if !confirmed {
        anyhow::bail!("Destroy cancelled.");
    }

    // Stop the container if it is running
    if project::container_running(project)? {
        println!("Stopping container '{}'...", project::container_name(project));
        let status = Command::new("docker")
            .args(["stop", &project::container_name(project)])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to run 'docker stop': {e}"))?;

        if !status.success() {
            eprintln!(
                "Warning: failed to stop container '{}' (it may have already exited).",
                project::container_name(project)
            );
        }
    }

    // Remove the Docker volume
    if has_volume {
        println!("Removing volume '{}'...", project::volume_name(project));
        project::remove_volume(project)?;
    }

    // Remove the project config directory
    if has_config {
        println!("Removing config directory...");
        std::fs::remove_dir_all(&config_dir).map_err(|e| {
            anyhow::anyhow!(
                "Failed to remove config directory '{}': {e}",
                config_dir.display()
            )
        })?;
    }

    println!("Project '{}' destroyed.", project);
    Ok(())
}

/// List all configured projects with their repository URL and status.
///
/// Reads the project config directory and checks Docker state for each project
/// to determine whether it is running, stopped, or has no volume.
pub fn cmd_list() -> anyhow::Result<()> {
    let projects = config::list_projects()?;

    if projects.is_empty() {
        println!("No projects configured.");
        return Ok(());
    }

    // Collect project info
    struct ProjectRow {
        name: String,
        repo: String,
        status: String,
    }

    let mut rows: Vec<ProjectRow> = Vec::new();

    for name in &projects {
        let repo = match config::load_project(name) {
            Ok(cfg) => cfg.project.repo_url,
            Err(_) => "<config error>".to_string(),
        };

        let status = if !project::volume_exists(name).unwrap_or(false) {
            "no volume".to_string()
        } else if project::container_running(name).unwrap_or(false) {
            "running".to_string()
        } else {
            "stopped".to_string()
        };

        rows.push(ProjectRow { name: name.clone(), repo, status });
    }

    // Calculate column widths
    let name_width = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let repo_width = rows.iter().map(|r| r.repo.len()).max().unwrap_or(4).max(4);

    // Print header
    println!(
        "{:<name_w$}  {:<repo_w$}  STATUS",
        "NAME",
        "REPO",
        name_w = name_width,
        repo_w = repo_width,
    );

    // Print rows
    for row in &rows {
        println!(
            "{:<name_w$}  {:<repo_w$}  {}",
            row.name,
            row.repo,
            row.status,
            name_w = name_width,
            repo_w = repo_width,
        );
    }

    Ok(())
}

/// Assemble Docker run arguments for launching a project container.
///
/// This function is shared between `cmd_run` and `cmd_shell` to ensure
/// consistent container configuration.
pub(crate) fn build_run_args(project: &str, image: &str) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        project::container_name(project),
        "-v".to_string(),
        format!("{}:/workspace", project::volume_name(project)),
        "-v".to_string(),
        "/var/run/docker.sock:/var/run/docker.sock".to_string(),
    ];

    // Mount host gitconfig if it exists
    if let Some(home) = dirs::home_dir() {
        let gitconfig = home.join(".gitconfig");
        if gitconfig.exists() {
            args.push("-v".to_string());
            args.push(format!("{}:/host-config/gitconfig:ro", gitconfig.display()));
        }

        let ssh_dir = home.join(".ssh");
        if ssh_dir.exists() {
            args.push("-v".to_string());
            args.push(format!("{}:/host-config/ssh:ro", ssh_dir.display()));
        }

        let claude_dir = home.join(".claude");
        if claude_dir.exists() {
            args.push("-v".to_string());
            args.push(format!(
                "{}:/host-config/claude-credentials:ro",
                claude_dir.display()
            ));
        }
    }

    args.push("-w".to_string());
    args.push("/workspace/project".to_string());

    args.push("-e".to_string());
    args.push("HOME=/workspace/home".to_string());

    args.push("--shm-size=256m".to_string());

    // Pass through ANTHROPIC_API_KEY if set in the host environment
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        args.push("-e".to_string());
        args.push("ANTHROPIC_API_KEY".to_string());
    }

    // Only allocate a TTY if stdin is a terminal
    if std::io::stdin().is_terminal() {
        args.push("-it".to_string());
    }

    // Image name is the last positional arg before the command
    args.push(image.to_string());

    args
}
