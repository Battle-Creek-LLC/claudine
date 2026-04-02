use std::process::Command;

/// Validate that a project name is safe and well-formed.
/// Must start with an alphanumeric character and contain only
/// alphanumeric characters, hyphens, and underscores.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("Project name cannot be empty.");
    }

    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphanumeric() {
        anyhow::bail!(
            "Project name must start with a letter or digit, got '{}'.",
            first
        );
    }

    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' {
            anyhow::bail!(
                "Project name contains invalid character '{}'. \
                 Only letters, digits, hyphens, and underscores are allowed.",
                ch
            );
        }
    }

    Ok(())
}

/// Return the Docker volume name for a project.
pub fn volume_name(project: &str) -> String {
    format!("claudine_{project}")
}

/// Return the Docker container name for a project.
pub fn container_name(project: &str) -> String {
    format!("claudine_{project}")
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
    }

    #[test]
    fn invalid_project_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("my project").is_err());
        assert!(validate_name("../escape").is_err());
        assert!(validate_name(".hidden").is_err());
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
