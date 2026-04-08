use std::process::Command;

/// Validate that a name is safe for use as a project or directory name.
/// Must start with an alphanumeric character, contain only alphanumeric
/// characters, hyphens, underscores, and dots, and be at most 64 characters.
fn validate_safe_name(name: &str, label: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("{} cannot be empty.", label);
    }

    if name.len() > 64 {
        anyhow::bail!("{} is too long (max 64 characters).", label);
    }

    if name == "." || name == ".." || name == "home" {
        anyhow::bail!("{} '{}' is reserved.", label, name);
    }

    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphanumeric() {
        anyhow::bail!(
            "{} must start with a letter or digit, got '{}'.",
            label, first
        );
    }

    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' && ch != '.' {
            anyhow::bail!(
                "{} contains invalid character '{}'. \
                 Only letters, digits, hyphens, underscores, and dots are allowed.",
                label, ch
            );
        }
    }

    Ok(())
}

/// Validate a project name.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    validate_safe_name(name, "Project name")
}

/// Validate a repository directory name.
pub fn validate_dir(dir: &str) -> anyhow::Result<()> {
    validate_safe_name(dir, "Directory name")
}

/// Return the Docker volume name for a project.
pub fn volume_name(project: &str) -> String {
    format!("claudine_{project}")
}

/// Return the Docker container name for a project.
pub fn container_name(project: &str) -> String {
    format!("claudine_{project}")
}

/// Return the host-side shared directory path for a project: ~/share/<project>/
pub fn share_dir(project: &str) -> anyhow::Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(home.join("share").join(project))
}

/// Check whether the Docker volume for a project exists.
pub fn volume_exists(project: &str) -> anyhow::Result<bool> {
    let name = volume_name(project);
    let status = Command::new("docker")
        .args(["volume", "inspect", &name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker volume inspect': {e}"))?;

    Ok(status.success())
}

/// Check whether the Docker container for a project is currently running.
pub fn container_running(project: &str) -> anyhow::Result<bool> {
    let name = container_name(project);
    let filter = format!("name=^{name}$");
    let output = Command::new("docker")
        .args(["ps", "--filter", &filter, "--format", "{{.Names}}"])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker ps': {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.trim().is_empty())
}

/// Create a Docker volume for a project.
pub fn create_volume(project: &str) -> anyhow::Result<()> {
    let name = volume_name(project);
    let status = Command::new("docker")
        .args(["volume", "create", &name])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker volume create': {e}"))?;

    if !status.success() {
        anyhow::bail!("Failed to create Docker volume '{name}'.");
    }

    Ok(())
}

/// Remove the Docker volume for a project.
pub fn remove_volume(project: &str) -> anyhow::Result<()> {
    let name = volume_name(project);
    let status = Command::new("docker")
        .args(["volume", "rm", &name])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker volume rm': {e}"))?;

    if !status.success() {
        anyhow::bail!("Failed to remove Docker volume '{name}'.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_project_names() {
        assert!(validate_name("myproject").is_ok());
        assert!(validate_name("my-project").is_ok());
        assert!(validate_name("my_project_2").is_ok());
        assert!(validate_name("my.project").is_ok());
    }

    #[test]
    fn invalid_project_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("my project").is_err());
        assert!(validate_name("../escape").is_err());
        assert!(validate_name(".hidden").is_err());
        assert!(validate_name("home").is_err());
        assert!(validate_name("a".repeat(65).as_str()).is_err());
    }

    #[test]
    fn valid_dir_names() {
        assert!(validate_dir("plotzy-api").is_ok());
        assert!(validate_dir("my.dotted.repo").is_ok());
        assert!(validate_dir("repo_v2").is_ok());
    }

    #[test]
    fn invalid_dir_names() {
        assert!(validate_dir("").is_err());
        assert!(validate_dir("..").is_err());
        assert!(validate_dir("home").is_err());
        assert!(validate_dir("../escape").is_err());
        assert!(validate_dir("-flag").is_err());
    }

    #[test]
    fn volume_name_format() {
        assert_eq!(volume_name("myproject"), "claudine_myproject");
        assert_eq!(volume_name("test-123"), "claudine_test-123");
    }

    #[test]
    fn container_name_format() {
        assert_eq!(container_name("myproject"), "claudine_myproject");
        assert_eq!(container_name("test-123"), "claudine_test-123");
    }
}
