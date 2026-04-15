use std::io::{IsTerminal, Write};
use std::process::Command;

use dialoguer::Confirm;

use crate::{config, layer, project, sources};

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

/// Stop and remove all containers using a given image tag.
fn remove_containers_for_image(image: &str) {
    let output = Command::new("docker")
        .args(["ps", "-a", "--filter", &format!("ancestor={}", image), "--format", "{{.Names}}"])
        .output();

    if let Ok(output) = output {
        let names = String::from_utf8_lossy(&output.stdout);
        for name in names.lines().filter(|n| !n.is_empty()) {
            println!("Removing container '{}' (old image)...", name);
            let _ = Command::new("docker")
                .args(["stop", name])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            let _ = Command::new("docker")
                .args(["rm", name])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

/// Build the claudine Docker image.
pub fn cmd_build(no_cache: bool) -> anyhow::Result<()> {
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

    let mut args = vec!["build", "-t", "claudine:latest"];
    if no_cache {
        args.push("--no-cache");
    }

    let status = Command::new("docker")
        .args(&args)
        .arg(tmp.path())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker build': {e}"))?;

    if !status.success() {
        anyhow::bail!("Docker build failed with exit code: {}", status);
    }

    remove_containers_for_image("claudine:latest");
    println!("Successfully built claudine:latest");
    Ok(())
}

/// Build a project-specific Docker image from a generated Dockerfile.
///
/// Writes the Dockerfile content to a temporary directory and runs
/// `docker build -t claudine:<project>` against it.
pub fn cmd_build_project(project: &str, dockerfile_content: &str, no_cache: bool) -> anyhow::Result<()> {
    check_docker()?;

    let tmp = tempfile::tempdir()
        .map_err(|e| anyhow::anyhow!("Failed to create temporary directory: {e}"))?;

    let dockerfile_path = tmp.path().join("Dockerfile");
    let mut f = std::fs::File::create(&dockerfile_path)
        .map_err(|e| anyhow::anyhow!("Failed to write Dockerfile: {e}"))?;
    f.write_all(dockerfile_content.as_bytes())?;

    sources::stage_sources(tmp.path())?;

    let tag = format!("claudine:{}", project);
    println!("Building project image {}...", tag);

    let mut args = vec!["build", "-t"];
    args.push(&tag);
    if no_cache {
        args.push("--no-cache");
    } else {
        args.push("--cache-from");
        args.push(&tag);
    }

    let status = Command::new("docker")
        .args(&args)
        .arg(tmp.path())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker build': {e}"))?;

    if !status.success() {
        anyhow::bail!("Docker build failed with exit code: {}", status);
    }

    remove_containers_for_image(&tag);
    println!("Successfully built {}", tag);
    Ok(())
}

/// Launch Claude Code in a container for the given project.
///
/// If the project container is already running, attaches via `docker exec`.
/// Otherwise, starts a new named container. Uses `exec()` to replace the process.
pub fn cmd_run(project: &str, repo: Option<&str>, resume: Option<&str>, prompt: Option<&str>, extra_args: &[String]) -> anyhow::Result<()> {
    validate_project(project, repo)?;

    let mut container_cmd: Vec<String> = vec!["claude".to_string(), "--dangerously-skip-permissions".to_string()];
    if let Some(session) = resume {
        container_cmd.push("--resume".to_string());
        container_cmd.push(session.to_string());
    }
    if let Some(p) = prompt {
        container_cmd.push("--output-format".to_string());
        container_cmd.push("stream-json".to_string());
        container_cmd.push("--verbose".to_string());
        container_cmd.push("-p".to_string());
        container_cmd.push(p.to_string());
    }
    container_cmd.extend_from_slice(extra_args);

    exec_in_project(project, repo, &container_cmd)
}

/// Open an interactive shell in a project's container, or run a command non-interactively.
///
/// If `args` is empty, launches an interactive zsh. Otherwise, runs the given command
/// in the container and exits. If the project container is already running, attaches
/// via `docker exec`; otherwise starts a new named container first.
pub fn cmd_shell(project: &str, repo: Option<&str>, args: &[String]) -> anyhow::Result<()> {
    validate_project(project, repo)?;
    if args.is_empty() {
        exec_in_project(project, repo, &["zsh".to_string()])
    } else {
        exec_in_project(project, repo, args)
    }
}

/// Validate project exists and repo is valid.
fn validate_project(project: &str, repo: Option<&str>) -> anyhow::Result<()> {
    project::validate_name(project)?;
    check_docker()?;

    if !project::volume_exists(project)? {
        anyhow::bail!(
            "Project '{}' is not initialized. Run 'claudine init {}' first.",
            project,
            project
        );
    }

    if let Some(r) = repo {
        let project_config = config::load_project(project)?;
        if !project_config.repos.iter().any(|rc| rc.dir == r) {
            let available: Vec<&str> = project_config.repos.iter().map(|rc| rc.dir.as_str()).collect();
            anyhow::bail!(
                "Repository '{}' not found in project '{}'. Available: {}",
                r, project, available.join(", ")
            );
        }
    }

    Ok(())
}

/// Execute a command in the project container.
/// If the container is already running, uses `docker exec`.
/// Otherwise, starts a new named container with `docker run`.
fn exec_in_project(project: &str, repo: Option<&str>, container_cmd: &[String]) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;

    let workdir = match repo {
        Some(r) => format!("/project/{}", r),
        None => "/project".to_string(),
    };

    // State 1: Container is running — exec into it
    if project::container_running(project)? {
        let project_config = config::load_project(project)?;
        let layers = project_config.layers.as_deref().unwrap_or(&[]);
        let path = layer::compute_path(layers);

        let mut cmd = Command::new("docker");
        cmd.arg("exec");

        if std::io::stdin().is_terminal() {
            cmd.arg("-it");
        }

        cmd.args(["-u", "claude"]);
        cmd.args(["-w", &workdir]);
        cmd.args(["-e", "HOME=/project/home"]);
        cmd.args(["-e", &format!("PATH={}", path)]);
        cmd.arg(project::container_name(project));
        cmd.args(container_cmd);

        let err = cmd.exec();
        return Err(anyhow::anyhow!("Failed to exec docker: {}", err));
    }

    // State 2: Container exists but stopped — restart it, then exec
    if project::container_exists(project)? {
        project::container_start(project)?;

        let project_config = config::load_project(project)?;
        let layers = project_config.layers.as_deref().unwrap_or(&[]);
        let path = layer::compute_path(layers);

        let mut cmd = Command::new("docker");
        cmd.arg("exec");

        if std::io::stdin().is_terminal() {
            cmd.arg("-it");
        }

        cmd.args(["-u", "claude"]);
        cmd.args(["-w", &workdir]);
        cmd.args(["-e", "HOME=/project/home"]);
        cmd.args(["-e", &format!("PATH={}", path)]);
        cmd.arg(project::container_name(project));
        cmd.args(container_cmd);

        let err = cmd.exec();
        return Err(anyhow::anyhow!("Failed to exec docker: {}", err));
    }

    // State 3: No container — create one with a keep-alive, then exec
    let project_config = config::load_project(project)?;
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    let docker_args = build_run_args(project, &image, repo);

    let status = Command::new("docker")
        .args(&docker_args)
        .args(["sleep", "infinity"])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker run': {e}"))?;

    if !status.success() {
        anyhow::bail!("Failed to start container for project '{}'.", project);
    }

    let layers = project_config.layers.as_deref().unwrap_or(&[]);
    let path = layer::compute_path(layers);

    let mut cmd = Command::new("docker");
    cmd.arg("exec");

    if std::io::stdin().is_terminal() {
        cmd.arg("-it");
    }

    cmd.args(["-u", "claude"]);
    cmd.args(["-w", &workdir]);
    cmd.args(["-e", "HOME=/project/home"]);
    cmd.args(["-e", &format!("PATH={}", path)]);
    cmd.arg(project::container_name(project));
    cmd.args(container_cmd);

    let err = cmd.exec();
    Err(anyhow::anyhow!("Failed to exec docker: {}", err))
}

/// Destroy a project by removing its container and optionally its volume and configuration.
///
/// By default, only removes the container. With `purge: true`, also removes the
/// Docker volume and project config directory.
pub fn cmd_destroy(project: &str, purge: bool) -> anyhow::Result<()> {
    project::validate_name(project)?;

    // Check that the project has some presence (config or volume)
    let has_volume = project::volume_exists(project)?;
    let has_container = project::container_exists(project)?;
    let config_dir = config::config_dir()?.join("projects").join(project);
    let has_config = config_dir.exists();

    if !has_volume && !has_config && !has_container {
        anyhow::bail!(
            "No project '{}' found. Nothing to destroy.",
            project
        );
    }

    let prompt_msg = if purge {
        format!("This will delete ALL data for '{}' (container, volume, config). Continue?", project)
    } else {
        format!("This will remove the container for '{}'. Volume and config are preserved. Continue?", project)
    };

    let confirmed = Confirm::new()
        .with_prompt(prompt_msg)
        .default(false)
        .interact()?;

    if !confirmed {
        anyhow::bail!("Destroy cancelled.");
    }

    // Stop and remove the container if it exists
    if has_container {
        let cname = project::container_name(project);
        println!("Removing container '{}'...", cname);
        let _ = Command::new("docker")
            .args(["stop", &cname])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let status = Command::new("docker")
            .args(["rm", &cname])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to run 'docker rm': {e}"))?;

        if !status.success() {
            eprintln!(
                "Warning: failed to remove container '{}'.",
                cname
            );
        }
    }

    if purge {
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

        println!("Project '{}' purged.", project);
    } else {
        println!("Container for '{}' removed. Volume and config preserved.", project);
    }

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
        repos: Vec<String>,
        status: String,
    }

    let mut rows: Vec<ProjectRow> = Vec::new();

    for name in &projects {
        let repos = match config::load_project(name) {
            Ok(cfg) => {
                cfg.repos
                    .iter()
                    .map(|r| r.dir.clone())
                    .collect()
            }
            Err(_) => vec!["<config error>".to_string()],
        };

        let status = if !project::volume_exists(name).unwrap_or(false) {
            "no volume".to_string()
        } else if project::container_running(name).unwrap_or(false) {
            "running".to_string()
        } else {
            "stopped".to_string()
        };

        rows.push(ProjectRow { name: name.clone(), repos, status });
    }

    // Calculate column widths
    let name_width = rows.iter().map(|r| r.name.len()).max().unwrap_or(7).max(7);

    // Print header
    println!(
        "{:<name_w$}  {:<8}  REPOS",
        "PROJECT",
        "STATUS",
        name_w = name_width,
    );

    // Print rows
    for row in &rows {
        let repo_summary = if row.repos.len() <= 4 {
            row.repos.join(", ")
        } else {
            format!("{}, {} (+{} more)",
                row.repos[0],
                row.repos[1],
                row.repos.len() - 2,
            )
        };
        println!(
            "{:<name_w$}  {:<8}  {}",
            row.name,
            row.status,
            repo_summary,
            name_w = name_width,
        );
    }

    Ok(())
}

/// Assemble Docker run arguments for launching a project container.
///
/// This function is shared between `cmd_run` and `cmd_shell` to ensure
/// consistent container configuration.
pub(crate) fn build_run_args(project: &str, image: &str, repo: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        project::container_name(project),
        "-v".to_string(),
        format!("{}:/project", project::volume_name(project)),
        "-v".to_string(),
        "/var/run/docker.sock:/var/run/docker.sock".to_string(),
    ];

    // Mount the shared directory if it exists
    if let Ok(share) = project::share_dir(project) {
        if share.exists() {
            args.push("-v".to_string());
            args.push(format!("{}:/share", share.display()));
        }
    }

    args.push("-w".to_string());
    match repo {
        Some(r) => args.push(format!("/project/{}", r)),
        None => args.push("/project".to_string()),
    };

    args.push("-e".to_string());
    args.push("HOME=/project/home".to_string());

    args.push("--shm-size=256m".to_string());

    // Only allocate a TTY if stdin is a terminal
    if std::io::stdin().is_terminal() {
        args.push("-it".to_string());
    }

    // Image name is the last positional arg before the command
    args.push(image.to_string());

    args
}
