use std::process::{Command, Stdio};

use crate::{config, docker};

const GO_VERSION: &str = "1.25.8";

/// A built-in layer representing a Dockerfile snippet that can be layered
/// on top of the base claudine image.
pub struct Layer {
    pub name: &'static str,
    pub description: &'static str,
    /// Layer names that satisfy a dependency. At least ONE must be present.
    pub requires: &'static [&'static str],
    /// Build toolchain needed to compile this layer from source.
    /// The Dockerfile generator installs and removes the toolchain automatically.
    pub build_tool: Option<BuildTool>,
    pub dockerfile: String,
    /// Shell commands that should exit 0 when the layer is installed correctly.
    pub validate: &'static [&'static str],
}

#[derive(Clone, Copy, PartialEq)]
pub enum BuildTool {
    Rust,
    Go,
}

/// Return the full catalog of built-in layers.
pub fn catalog() -> Vec<Layer> {
    vec![
        Layer {
            name: "node-20",
            description: "Node.js 20.x LTS",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && corepack enable".to_string(),
            validate: &["node --version", "npm --version", "corepack --version"],
        },
        Layer {
            name: "node-22",
            description: "Node.js 22.x LTS",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && corepack enable".to_string(),
            validate: &["node --version", "npm --version", "corepack --version"],
        },
        Layer {
            name: "node-24",
            description: "Node.js 24.x",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && corepack enable".to_string(),
            validate: &["node --version", "npm --version", "npx --version", "corepack --version"],
        },
        Layer {
            name: "gh",
            description: "GitHub CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \\\n       | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \\\n    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \\\n    && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" \\\n       > /etc/apt/sources.list.d/github-cli.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends gh \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["gh --version"],
        },
        Layer {
            name: "heroku",
            description: "Heroku CLI",
            requires: &["node-20", "node-22", "node-24"],
            build_tool: None,
            dockerfile: "RUN curl https://cli-assets.heroku.com/install.sh | sh".to_string(),
            validate: &["heroku --version"],
        },
        Layer {
            name: "python-venv",
            description: "Python 3 virtual environment support",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN apt-get update && apt-get install -y python3-venv \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["python3 -m venv --help"],
        },
        Layer {
            name: "rust",
            description: "Rust toolchain (persistent, available at runtime)",
            requires: &[],
            build_tool: None,
            dockerfile: "ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo\nRUN apt-get update && apt-get install -y build-essential \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path \\\n    && chmod -R a+rX /usr/local/rustup /usr/local/cargo \\\n    && curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to /usr/local/bin\nENV PATH=\"/usr/local/cargo/bin:${PATH}\"".to_string(),
            validate: &["cargo --version", "rustc --version", "just --version"],
        },
        Layer {
            name: "go",
            description: "Go toolchain (persistent, available at runtime)",
            requires: &[],
            build_tool: None,
            dockerfile: format!(
                "RUN curl -fsSL https://go.dev/dl/go{ver}.linux-$(dpkg --print-architecture).tar.gz | tar -C /usr/local -xz\nENV PATH=\"/usr/local/go/bin:${{PATH}}\"",
                ver = GO_VERSION
            ),
            validate: &["go version"],
        },
        Layer {
            name: "java",
            description: "OpenJDK 21 LTS runtime",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://packages.adoptium.net/artifactory/api/gpg/key/public | gpg --dearmor -o /usr/share/keyrings/adoptium.gpg \\\n    && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/adoptium.gpg] https://packages.adoptium.net/artifactory/deb bookworm main\" \\\n       > /etc/apt/sources.list.d/adoptium.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends temurin-21-jre \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["java -version"],
        },
        Layer {
            name: "flyway",
            description: "Flyway database migration CLI",
            requires: &["java"],
            build_tool: None,
            dockerfile: "RUN FLYWAY_VERSION=$(curl -fsSL https://api.github.com/repos/flyway/flyway/releases/latest | grep '\"tag_name\"' | sed 's/.*\"flyway-\\(.*\\)\".*/\\1/') \\\n    && curl -fsSL \"https://download.red-gate.com/maven/release/com/redgate/flyway/flyway-commandline/${FLYWAY_VERSION}/flyway-commandline-${FLYWAY_VERSION}.tar.gz\" | tar -C /opt -xz \\\n    && chmod +x /opt/flyway-${FLYWAY_VERSION}/flyway \\\n    && ln -s /opt/flyway-${FLYWAY_VERSION}/flyway /usr/local/bin/flyway".to_string(),
            validate: &["flyway --help"],
        },
        Layer {
            name: "lin",
            description: "Fast CLI for Linear (built from source)",
            requires: &[],
            build_tool: Some(BuildTool::Rust),
            dockerfile: "RUN git clone https://github.com/sprouted-dev/lin.git /tmp/lin \\\n    && cd /tmp/lin \\\n    && cargo build --release \\\n    && cp target/release/lin /usr/local/bin/lin \\\n    && chmod 755 /usr/local/bin/lin \\\n    && rm -rf /tmp/lin".to_string(),
            validate: &["lin --help"],
        },
        Layer {
            name: "glab",
            description: "GitLab CLI (built from source, jstockdi fork)",
            requires: &[],
            build_tool: Some(BuildTool::Go),
            dockerfile: "RUN git clone https://github.com/jstockdi/glab.git /tmp/glab \\\n    && cd /tmp/glab \\\n    && make build \\\n    && cp bin/glab /usr/local/bin/glab \\\n    && chmod 755 /usr/local/bin/glab \\\n    && rm -rf /tmp/glab".to_string(),
            validate: &["glab version"],
        },
        Layer {
            name: "aws",
            description: "AWS CLI v2",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL \"https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip\" -o /tmp/awscliv2.zip \\\n    && unzip -q /tmp/awscliv2.zip -d /tmp \\\n    && /tmp/aws/install \\\n    && rm -rf /tmp/awscliv2.zip /tmp/aws".to_string(),
            validate: &["aws --version"],
        },
        Layer {
            name: "terraform",
            description: "Terraform CLI for infrastructure provisioning",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://apt.releases.hashicorp.com/gpg | gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg \\\n    && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com bookworm main\" \\\n       > /etc/apt/sources.list.d/hashicorp.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends terraform \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["terraform version"],
        },
        Layer {
            name: "doctl",
            description: "DigitalOcean CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN DOCTL_VERSION=$(curl -fsSL https://api.github.com/repos/digitalocean/doctl/releases/latest | grep '\"tag_name\"' | sed 's/.*\"v\\(.*\\)\".*/\\1/') \\\n    && curl -fsSL \"https://github.com/digitalocean/doctl/releases/download/v${DOCTL_VERSION}/doctl-${DOCTL_VERSION}-linux-$(dpkg --print-architecture).tar.gz\" | tar -C /usr/local/bin -xz \\\n    && chmod 755 /usr/local/bin/doctl".to_string(),
            validate: &["doctl version"],
        },
        Layer {
            name: "rodney",
            description: "Chrome automation CLI (built from source, jstockdi fork)",
            requires: &[],
            build_tool: Some(BuildTool::Go),
            dockerfile: "RUN git clone https://github.com/jstockdi/rodney.git /tmp/rodney \\\n    && cd /tmp/rodney \\\n    && go build -o /usr/local/bin/rodney . \\\n    && chmod 755 /usr/local/bin/rodney \\\n    && rm -rf /tmp/rodney".to_string(),
            validate: &["rodney --help"],
        },
    ]
}

/// Look up a layer by name in the catalog.
pub fn find(name: &str) -> Option<Layer> {
    catalog().into_iter().find(|p| p.name == name)
}

/// Check that the dependency requirements for a layer are satisfied.
///
/// For layers with a non-empty `requires` list, at least one of the listed
/// layers must already be present in `installed`.
pub fn check_requires(name: &str, installed: &[String]) -> anyhow::Result<()> {
    let layer = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown layer '{}'.", name))?;

    if layer.requires.is_empty() {
        return Ok(());
    }

    let satisfied = layer
        .requires
        .iter()
        .any(|req| installed.iter().any(|i| i == req));

    if !satisfied {
        let options = layer.requires.join(", ");
        anyhow::bail!(
            "Layer '{}' requires one of: {}. Install one first: claudine layer add <project> {}",
            name,
            options,
            layer.requires[0],
        );
    }

    Ok(())
}

/// Generate a Dockerfile from a list of layer names.
///
/// Layers are ordered according to their position in the catalog, regardless
/// of the order they were installed. This ensures deterministic builds.
pub fn generate_dockerfile(layers: &[String]) -> anyhow::Result<String> {
    let cat = catalog();

    // Collect layers in catalog order
    let ordered: Vec<&Layer> = cat
        .iter()
        .filter(|p| layers.iter().any(|name| name == p.name))
        .collect();

    // Verify all requested layers exist
    for name in layers {
        if !cat.iter().any(|p| p.name == name) {
            anyhow::bail!("Unknown layer '{}'.", name);
        }
    }

    let needs_rust = ordered.iter().any(|p| p.build_tool == Some(BuildTool::Rust))
        && !layers.iter().any(|n| n == "rust");
    let needs_go = ordered.iter().any(|p| p.build_tool == Some(BuildTool::Go))
        && !layers.iter().any(|n| n == "go");

    let mut lines = vec!["FROM claudine:latest".to_string()];

    // Non-compiled layers first
    for layer in ordered.iter().filter(|p| p.build_tool.is_none()) {
        lines.push(String::new());
        lines.push(format!("# Layer: {}", layer.name));
        lines.push(layer.dockerfile.to_string());
    }

    // Install build toolchains as needed
    if needs_rust || needs_go {
        lines.push(String::new());
        lines.push("# Build phase: install build toolchains".to_string());

        let mut install_parts = vec!["RUN apt-get update && apt-get install -y build-essential".to_string()];

        if needs_rust {
            install_parts.push("    && export RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path".to_string());
        }

        if needs_go {
            install_parts.push(format!("    && curl -fsSL https://go.dev/dl/go{GO_VERSION}.linux-$(dpkg --print-architecture).tar.gz | tar -C /usr/local -xz"));
        }

        lines.push(install_parts.join(" \\\n"));
    }

    // Compiled layers (Rust first, then Go — catalog order)
    let compiled: Vec<_> = ordered.iter().filter(|p| p.build_tool.is_some()).collect();
    for layer in &compiled {
        lines.push(String::new());
        lines.push(format!("# Layer: {}", layer.name));
        // Compiled layers need PATH set for their build toolchain
        if layer.build_tool == Some(BuildTool::Go) {
            let dockerfile = layer.dockerfile.replacen("RUN ", "RUN export PATH=$PATH:/usr/local/go/bin && ", 1);
            lines.push(dockerfile);
        } else if layer.build_tool == Some(BuildTool::Rust) {
            let dockerfile = layer.dockerfile.replacen("RUN ", "RUN export PATH=$PATH:/usr/local/cargo/bin && ", 1);
            lines.push(dockerfile);
        } else {
            lines.push(layer.dockerfile.to_string());
        }
    }

    // Clean up build toolchains
    if needs_rust || needs_go {
        lines.push(String::new());
        lines.push("# Cleanup: remove build toolchains".to_string());

        let mut cleanup = vec!["RUN apt-get purge -y build-essential && apt-get autoremove -y".to_string()];

        if needs_rust {
            cleanup.push("    && rm -rf /usr/local/cargo /usr/local/rustup".to_string());
        }
        if needs_go {
            cleanup.push("    && rm -rf /usr/local/go".to_string());
        }

        cleanup.push("    && rm -rf /var/lib/apt/lists/*".to_string());
        lines.push(cleanup.join(" \\\n"));
    }

    // Trailing newline
    lines.push(String::new());

    Ok(lines.join("\n"))
}

/// Rebuild all project images that have layers installed.
pub fn cmd_build_all() -> anyhow::Result<()> {
    let projects = config::list_projects()?;
    let mut failures: Vec<String> = Vec::new();

    for name in &projects {
        let project_config = match config::load_project(name) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let has_layers = project_config
            .layers
            .as_ref()
            .map(|l| !l.is_empty())
            .unwrap_or(false);

        if !has_layers {
            continue;
        }

        println!("=== {} ===", name);
        match cmd_build_project(name) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {}", e);
                failures.push(name.clone());
            }
        }
        println!();
    }

    if failures.is_empty() {
        println!("All project images rebuilt.");
        Ok(())
    } else {
        anyhow::bail!("{} project(s) failed: {}", failures.len(), failures.join(", "))
    }
}

/// Rebuild a project's layer image from its current config.
pub fn cmd_build_project(project: &str) -> anyhow::Result<()> {
    let project_config = config::load_project(project)?;

    let layers = project_config
        .layers
        .as_ref()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Project '{}' has no layers installed.", project))?;

    let dockerfile = generate_dockerfile(layers)?;
    docker::cmd_build_project(project, &dockerfile)?;

    let image = format!("claudine:{}", project);
    validate_image(&image, layers)?;

    println!("Project '{}' image rebuilt.", project);
    Ok(())
}

/// Add a layer to a project.
///
/// Validates the layer exists, checks dependency requirements, updates the
/// project config, generates a new Dockerfile, and builds the project image.
pub fn cmd_layer_add(project: &str, layer: &str) -> anyhow::Result<()> {
    // Validate layer exists
    if find(layer).is_none() {
        anyhow::bail!(
            "Unknown layer '{}'. Run 'claudine layer available' to see options.",
            layer,
        );
    }

    let mut project_config = config::load_project(project)?;
    let layers = project_config.layers.get_or_insert_with(Vec::new);

    // Check if already installed
    if layers.iter().any(|p| p == layer) {
        println!("Layer '{}' is already installed in project '{}'.", layer, project);
        return Ok(());
    }

    // Check dependency requirements
    check_requires(layer, layers)?;

    // Add the layer
    layers.push(layer.to_string());

    // Set the project-specific image
    project_config.image = Some(config::ImageConfig {
        name: format!("claudine:{}", project),
    });

    config::save_project(project, &project_config)?;

    // Generate Dockerfile and build
    let layers = project_config.layers.as_ref().unwrap();
    let dockerfile = generate_dockerfile(layers)?;
    docker::cmd_build_project(project, &dockerfile)?;

    let image = format!("claudine:{}", project);
    validate_image(&image, layers)?;

    println!("Layer '{}' added to project '{}'.", layer, project);
    Ok(())
}

/// Remove a layer from a project.
///
/// Updates the project config and rebuilds the project image. If no layers
/// remain, reverts the image to `claudine:latest`.
pub fn cmd_layer_remove(project: &str, layer: &str) -> anyhow::Result<()> {
    let mut project_config = config::load_project(project)?;

    {
        let layers = project_config.layers.get_or_insert_with(Vec::new);

        let index = layers
            .iter()
            .position(|p| p == layer)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Layer '{}' is not installed in project '{}'.",
                    layer,
                    project,
                )
            })?;

        layers.remove(index);
    }

    let remaining = project_config
        .layers
        .as_ref()
        .map(|p| p.is_empty())
        .unwrap_or(true);

    if remaining {
        // Revert to base image
        project_config.layers = None;
        project_config.image = None;
        config::save_project(project, &project_config)?;
        println!("No layers remaining. Image reverted to claudine:latest.");
    } else {
        project_config.image = Some(config::ImageConfig {
            name: format!("claudine:{}", project),
        });
        config::save_project(project, &project_config)?;

        let layers = project_config.layers.as_ref().unwrap();
        let dockerfile = generate_dockerfile(layers)?;
        docker::cmd_build_project(project, &dockerfile)?;

        let image = format!("claudine:{}", project);
        validate_image(&image, layers)?;
    }

    println!("Layer '{}' removed from project '{}'.", layer, project);
    Ok(())
}

/// List layers installed in a project.
pub fn cmd_layer_list(project: &str) -> anyhow::Result<()> {
    let project_config = config::load_project(project)?;

    match &project_config.layers {
        Some(layers) if !layers.is_empty() => {
            println!("Layers for project '{}':", project);
            for name in layers {
                if let Some(p) = find(name) {
                    println!("  {} - {}", p.name, p.description);
                } else {
                    println!("  {} (unknown)", name);
                }
            }
        }
        _ => {
            println!("No layers installed for project '{}'.", project);
        }
    }

    Ok(())
}

/// List all available layers in the catalog.
pub fn cmd_layer_available() -> anyhow::Result<()> {
    let cat = catalog();

    println!("Available layers:");
    for layer in &cat {
        let deps = if layer.requires.is_empty() {
            String::new()
        } else {
            format!(" (requires one of: {})", layer.requires.join(", "))
        };
        println!("  {:<15} {}{}", layer.name, layer.description, deps);
    }

    Ok(())
}

/// Collect the minimal set of layers needed to validate a given layer.
///
/// Includes the target layer plus any required dependencies (picking the
/// first option from `requires` recursively).
fn collect_validation_layers(name: &str) -> anyhow::Result<Vec<String>> {
    let layer = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown layer '{}'.", name))?;

    let mut layers = Vec::new();

    if !layer.requires.is_empty() {
        let dep = layer.requires[0];
        let dep_layers = collect_validation_layers(dep)?;
        layers.extend(dep_layers);
    }

    layers.push(name.to_string());
    Ok(layers)
}

/// Build a temporary Docker image and return its tag.
fn build_validation_image(tag: &str, dockerfile: &str) -> anyhow::Result<()> {
    docker::check_docker()?;

    let tmp = tempfile::tempdir()?;
    std::fs::write(tmp.path().join("Dockerfile"), dockerfile)?;

    let output = Command::new("docker")
        .args(["build", "-t", tag])
        .arg(tmp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker build': {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to build validation image:\n{}", stderr);
    }

    Ok(())
}

/// Remove a Docker image, ignoring errors.
fn remove_image(tag: &str) {
    let _ = Command::new("docker")
        .args(["rmi", tag])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Run validation commands for the given layers against an existing Docker image.
///
/// Returns Ok if all checks pass, Err listing the failures otherwise.
fn validate_image(image: &str, layer_names: &[String]) -> anyhow::Result<()> {
    println!("Validating layers...");

    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut failed_layers: Vec<String> = Vec::new();

    for name in layer_names {
        let layer = match find(name) {
            Some(l) => l,
            None => continue,
        };

        if layer.validate.is_empty() {
            continue;
        }

        let mut layer_ok = true;
        for cmd in layer.validate {
            let status = Command::new("docker")
                .args([
                    "run", "--rm",
                    "--entrypoint", "bash",
                    "-e", "HOME=/tmp",
                    image, "-c", cmd,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to run 'docker run': {e}"))?;

            if status.success() {
                println!("  PASS  {} — {}", name, cmd);
                total_passed += 1;
            } else {
                println!("  FAIL  {} — {}", name, cmd);
                total_failed += 1;
                layer_ok = false;
            }
        }

        if !layer_ok {
            failed_layers.push(name.clone());
        }
    }

    if total_failed > 0 {
        anyhow::bail!(
            "{} check(s) failed across layer(s): {}",
            total_failed,
            failed_layers.join(", "),
        );
    }

    println!("All {} checks passed.", total_passed);
    Ok(())
}

/// Validate a single layer by building a temporary image and running its checks.
pub fn cmd_layer_validate(name: &str) -> anyhow::Result<()> {
    let _layer = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown layer '{}'.", name))?;

    let layers = collect_validation_layers(name)?;
    let dockerfile = generate_dockerfile(&layers)?;
    let tag = format!("claudine:validate-{}", name);

    println!("Building validation image ({})...", layers.join(", "));
    build_validation_image(&tag, &dockerfile)?;

    let result = validate_image(&tag, &layers);
    remove_image(&tag);
    result
}

/// Validate all layers in the catalog (standalone builds).
pub fn cmd_layer_validate_all() -> anyhow::Result<()> {
    let cat = catalog();
    let mut failures: Vec<String> = Vec::new();

    for layer in &cat {
        match cmd_layer_validate(layer.name) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("  {}", e);
                failures.push(layer.name.to_string());
            }
        }
        println!();
    }

    if failures.is_empty() {
        println!("All {} layers validated.", cat.len());
        Ok(())
    } else {
        anyhow::bail!("{} layer(s) failed validation: {}", failures.len(), failures.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_existing_layer() {
        assert!(find("node-20").is_some());
        assert!(find("heroku").is_some());
        assert!(find("rust").is_some());
    }

    #[test]
    fn find_unknown_layer() {
        assert!(find("does-not-exist").is_none());
    }

    #[test]
    fn check_requires_no_deps() {
        let installed = vec![];
        assert!(check_requires("node-20", &installed).is_ok());
        assert!(check_requires("python-venv", &installed).is_ok());
    }

    #[test]
    fn check_requires_satisfied() {
        let installed = vec!["node-20".to_string()];
        assert!(check_requires("heroku", &installed).is_ok());
    }

    #[test]
    fn check_requires_satisfied_alt() {
        let installed = vec!["node-22".to_string()];
        assert!(check_requires("heroku", &installed).is_ok());
    }

    #[test]
    fn check_requires_not_satisfied() {
        let installed = vec!["python-venv".to_string()];
        let result = check_requires("heroku", &installed);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("requires one of"));
        assert!(msg.contains("node-20"));
    }

    #[test]
    fn generate_dockerfile_single() {
        let layers = vec!["node-20".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(result.starts_with("FROM claudine:latest"));
        assert!(result.contains("# Layer: node-20"));
        assert!(result.contains("setup_20.x"));
    }

    #[test]
    fn generate_dockerfile_multiple_ordered() {
        // Install heroku first, node-20 second — output should be node-20 first
        let layers = vec!["heroku".to_string(), "node-20".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        let node_pos = result.find("# Layer: node-20").unwrap();
        let heroku_pos = result.find("# Layer: heroku").unwrap();
        assert!(
            node_pos < heroku_pos,
            "node-20 should appear before heroku in the Dockerfile"
        );
    }

    #[test]
    fn generate_dockerfile_unknown() {
        let layers = vec!["nonexistent".to_string()];
        assert!(generate_dockerfile(&layers).is_err());
    }

    #[test]
    fn generate_dockerfile_empty() {
        let layers: Vec<String> = vec![];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(result.starts_with("FROM claudine:latest"));
        // Should just be the FROM line and a trailing newline
        assert!(!result.contains("# Layer:"));
    }

    #[test]
    fn catalog_has_expected_layers() {
        let cat = catalog();
        let names: Vec<&str> = cat.iter().map(|p| p.name).collect();
        assert!(names.contains(&"node-20"));
        assert!(names.contains(&"node-22"));
        assert!(names.contains(&"node-24"));
        assert!(names.contains(&"heroku"));
        assert!(names.contains(&"python-venv"));
        assert!(names.contains(&"rust"));
        assert!(names.contains(&"go"));
        assert!(names.contains(&"aws"));
        assert!(names.contains(&"java"));
        assert!(names.contains(&"flyway"));
        assert!(names.contains(&"terraform"));
        assert!(names.contains(&"doctl"));
    }

    #[test]
    fn go_layer_skips_build_toolchain() {
        let layers = vec!["go".to_string(), "rodney".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(!result.contains("Build phase: install build toolchains"));
        assert!(!result.contains("Cleanup: remove build toolchains"));
        assert!(result.contains("# Layer: go"));
        assert!(result.contains("# Layer: rodney"));
    }

    #[test]
    fn heroku_requires_node() {
        let heroku = find("heroku").unwrap();
        assert!(!heroku.requires.is_empty());
        assert!(heroku.requires.contains(&"node-20"));
        assert!(heroku.requires.contains(&"node-22"));
        assert!(heroku.requires.contains(&"node-24"));
    }

    #[test]
    fn flyway_requires_java() {
        let flyway = find("flyway").unwrap();
        assert!(flyway.requires.contains(&"java"));

        let installed = vec![];
        assert!(check_requires("flyway", &installed).is_err());

        let installed = vec!["java".to_string()];
        assert!(check_requires("flyway", &installed).is_ok());
    }

    #[test]
    fn all_layers_have_validate_commands() {
        for layer in catalog() {
            assert!(
                !layer.validate.is_empty(),
                "Layer '{}' has no validation commands",
                layer.name,
            );
        }
    }

    #[test]
    fn collect_validation_layers_no_deps() {
        let layers = collect_validation_layers("rust").unwrap();
        assert_eq!(layers, vec!["rust"]);
    }

    #[test]
    fn collect_validation_layers_with_deps() {
        let layers = collect_validation_layers("heroku").unwrap();
        assert_eq!(layers, vec!["node-20", "heroku"]);
    }

    #[test]
    fn collect_validation_layers_transitive_deps() {
        let layers = collect_validation_layers("flyway").unwrap();
        assert_eq!(layers, vec!["java", "flyway"]);
    }

    #[test]
    fn collect_validation_layers_unknown() {
        assert!(collect_validation_layers("nope").is_err());
    }
}
