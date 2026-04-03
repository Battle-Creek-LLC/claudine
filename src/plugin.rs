use crate::{config, docker};

/// A built-in plugin representing a Dockerfile snippet that can be layered
/// on top of the base claudine image.
pub struct Plugin {
    pub name: &'static str,
    pub description: &'static str,
    /// Plugin names that satisfy a dependency. At least ONE must be present.
    pub requires: &'static [&'static str],
    /// Build toolchain needed to compile this plugin from source.
    /// The Dockerfile generator installs and removes the toolchain automatically.
    pub build_tool: Option<BuildTool>,
    pub dockerfile: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
pub enum BuildTool {
    Rust,
    Go,
}

/// Return the full catalog of built-in plugins.
pub fn catalog() -> Vec<Plugin> {
    vec![
        Plugin {
            name: "node-20",
            description: "Node.js 20.x LTS",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/*",
        },
        Plugin {
            name: "node-22",
            description: "Node.js 22.x LTS",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/*",
        },
        Plugin {
            name: "node-24",
            description: "Node.js 24.x",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/*",
        },
        Plugin {
            name: "heroku",
            description: "Heroku CLI",
            requires: &["node-20", "node-22", "node-24"],
            build_tool: None,
            dockerfile: "RUN curl https://cli-assets.heroku.com/install.sh | sh",
        },
        Plugin {
            name: "python-venv",
            description: "Python 3 virtual environment support",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN apt-get update && apt-get install -y python3-venv \\\n    && rm -rf /var/lib/apt/lists/*",
        },
        Plugin {
            name: "rust",
            description: "Rust toolchain (persistent, available at runtime)",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN apt-get update && apt-get install -y build-essential \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \\\n    && echo 'source /root/.cargo/env' >> /etc/bash.bashrc",
        },
        Plugin {
            name: "lin",
            description: "Fast CLI for Linear (built from source)",
            requires: &[],
            build_tool: Some(BuildTool::Rust),
            dockerfile: "RUN . /root/.cargo/env \\\n    && git clone https://github.com/sprouted-dev/lin.git /tmp/lin \\\n    && cd /tmp/lin \\\n    && cargo build --release \\\n    && cp target/release/lin /usr/local/bin/lin \\\n    && chmod 755 /usr/local/bin/lin \\\n    && rm -rf /tmp/lin",
        },
        Plugin {
            name: "glab",
            description: "GitLab CLI (built from source, jstockdi fork)",
            requires: &[],
            build_tool: Some(BuildTool::Go),
            dockerfile: "RUN git clone https://github.com/jstockdi/glab.git /tmp/glab \\\n    && cd /tmp/glab \\\n    && make build \\\n    && cp bin/glab /usr/local/bin/glab \\\n    && chmod 755 /usr/local/bin/glab \\\n    && rm -rf /tmp/glab",
        },
    ]
}

/// Look up a plugin by name in the catalog.
pub fn find(name: &str) -> Option<Plugin> {
    catalog().into_iter().find(|p| p.name == name)
}

/// Check that the dependency requirements for a plugin are satisfied.
///
/// For plugins with a non-empty `requires` list, at least one of the listed
/// plugins must already be present in `installed`.
pub fn check_requires(name: &str, installed: &[String]) -> anyhow::Result<()> {
    let plugin = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown plugin '{}'.", name))?;

    if plugin.requires.is_empty() {
        return Ok(());
    }

    let satisfied = plugin
        .requires
        .iter()
        .any(|req| installed.iter().any(|i| i == req));

    if !satisfied {
        let options = plugin.requires.join(", ");
        anyhow::bail!(
            "Plugin '{}' requires one of: {}. Install one first: claudine plugin add <project> {}",
            name,
            options,
            plugin.requires[0],
        );
    }

    Ok(())
}

/// Generate a Dockerfile from a list of plugin names.
///
/// Plugins are ordered according to their position in the catalog, regardless
/// of the order they were installed. This ensures deterministic builds.
pub fn generate_dockerfile(plugins: &[String]) -> anyhow::Result<String> {
    let cat = catalog();

    // Collect plugins in catalog order
    let ordered: Vec<&Plugin> = cat
        .iter()
        .filter(|p| plugins.iter().any(|name| name == p.name))
        .collect();

    // Verify all requested plugins exist
    for name in plugins {
        if !cat.iter().any(|p| p.name == name) {
            anyhow::bail!("Unknown plugin '{}'.", name);
        }
    }

    let needs_rust = ordered.iter().any(|p| p.build_tool == Some(BuildTool::Rust))
        && !plugins.iter().any(|n| n == "rust");
    let needs_go = ordered.iter().any(|p| p.build_tool == Some(BuildTool::Go));

    let mut lines = vec!["FROM claudine:latest".to_string()];

    // Non-compiled plugins first
    for plugin in ordered.iter().filter(|p| p.build_tool.is_none()) {
        lines.push(String::new());
        lines.push(format!("# Plugin: {}", plugin.name));
        lines.push(plugin.dockerfile.to_string());
    }

    // Install build toolchains as needed
    if needs_rust || needs_go {
        lines.push(String::new());
        lines.push("# Build phase: install build toolchains".to_string());

        let mut install_parts = vec!["RUN apt-get update && apt-get install -y build-essential".to_string()];

        if needs_rust {
            install_parts.push("    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y".to_string());
        }

        if needs_go {
            install_parts.push("    && curl -fsSL https://go.dev/dl/go1.25.8.linux-$(dpkg --print-architecture).tar.gz | tar -C /usr/local -xz".to_string());
        }

        lines.push(install_parts.join(" \\\n"));
    }

    // Compiled plugins (Rust first, then Go — catalog order)
    let compiled: Vec<_> = ordered.iter().filter(|p| p.build_tool.is_some()).collect();
    for plugin in &compiled {
        lines.push(String::new());
        lines.push(format!("# Plugin: {}", plugin.name));
        // Go plugins need PATH set
        if plugin.build_tool == Some(BuildTool::Go) {
            let dockerfile = plugin.dockerfile.replacen("RUN ", "RUN export PATH=$PATH:/usr/local/go/bin && ", 1);
            lines.push(dockerfile);
        } else {
            lines.push(plugin.dockerfile.to_string());
        }
    }

    // Clean up build toolchains
    if needs_rust || needs_go {
        lines.push(String::new());
        lines.push("# Cleanup: remove build toolchains".to_string());

        let mut cleanup = vec!["RUN apt-get purge -y build-essential && apt-get autoremove -y".to_string()];

        if needs_rust {
            cleanup.push("    && rm -rf /root/.cargo /root/.rustup".to_string());
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

/// Add a plugin to a project.
///
/// Validates the plugin exists, checks dependency requirements, updates the
/// project config, generates a new Dockerfile, and builds the project image.
pub fn cmd_plugin_add(project: &str, plugin: &str) -> anyhow::Result<()> {
    // Validate plugin exists
    if find(plugin).is_none() {
        anyhow::bail!(
            "Unknown plugin '{}'. Run 'claudine plugin available' to see options.",
            plugin,
        );
    }

    let mut project_config = config::load_project(project)?;
    let plugins = project_config.plugins.get_or_insert_with(Vec::new);

    // Check if already installed
    if plugins.iter().any(|p| p == plugin) {
        println!("Plugin '{}' is already installed in project '{}'.", plugin, project);
        return Ok(());
    }

    // Check dependency requirements
    check_requires(plugin, plugins)?;

    // Add the plugin
    plugins.push(plugin.to_string());

    // Set the project-specific image
    project_config.image = Some(config::ImageConfig {
        name: format!("claudine:{}", project),
    });

    config::save_project(project, &project_config)?;

    // Generate Dockerfile and build
    let dockerfile = generate_dockerfile(project_config.plugins.as_ref().unwrap())?;
    docker::cmd_build_project(project, &dockerfile)?;

    println!("Plugin '{}' added to project '{}'.", plugin, project);
    Ok(())
}

/// Remove a plugin from a project.
///
/// Updates the project config and rebuilds the project image. If no plugins
/// remain, reverts the image to `claudine:latest`.
pub fn cmd_plugin_remove(project: &str, plugin: &str) -> anyhow::Result<()> {
    let mut project_config = config::load_project(project)?;

    {
        let plugins = project_config.plugins.get_or_insert_with(Vec::new);

        let index = plugins
            .iter()
            .position(|p| p == plugin)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Plugin '{}' is not installed in project '{}'.",
                    plugin,
                    project,
                )
            })?;

        plugins.remove(index);
    }

    let remaining = project_config
        .plugins
        .as_ref()
        .map(|p| p.is_empty())
        .unwrap_or(true);

    if remaining {
        // Revert to base image
        project_config.plugins = None;
        project_config.image = None;
        config::save_project(project, &project_config)?;
        println!("No plugins remaining. Image reverted to claudine:latest.");
    } else {
        project_config.image = Some(config::ImageConfig {
            name: format!("claudine:{}", project),
        });
        config::save_project(project, &project_config)?;

        let plugins = project_config.plugins.as_ref().unwrap();
        let dockerfile = generate_dockerfile(plugins)?;
        docker::cmd_build_project(project, &dockerfile)?;
    }

    println!("Plugin '{}' removed from project '{}'.", plugin, project);
    Ok(())
}

/// List plugins installed in a project.
pub fn cmd_plugin_list(project: &str) -> anyhow::Result<()> {
    let project_config = config::load_project(project)?;

    match &project_config.plugins {
        Some(plugins) if !plugins.is_empty() => {
            println!("Plugins for project '{}':", project);
            for name in plugins {
                if let Some(p) = find(name) {
                    println!("  {} - {}", p.name, p.description);
                } else {
                    println!("  {} (unknown)", name);
                }
            }
        }
        _ => {
            println!("No plugins installed for project '{}'.", project);
        }
    }

    Ok(())
}

/// List all available plugins in the catalog.
pub fn cmd_plugin_available() -> anyhow::Result<()> {
    let cat = catalog();

    println!("Available plugins:");
    for plugin in &cat {
        let deps = if plugin.requires.is_empty() {
            String::new()
        } else {
            format!(" (requires one of: {})", plugin.requires.join(", "))
        };
        println!("  {:<15} {}{}", plugin.name, plugin.description, deps);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_existing_plugin() {
        assert!(find("node-20").is_some());
        assert!(find("heroku").is_some());
        assert!(find("rust").is_some());
    }

    #[test]
    fn find_unknown_plugin() {
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
        let plugins = vec!["node-20".to_string()];
        let result = generate_dockerfile(&plugins).unwrap();
        assert!(result.starts_with("FROM claudine:latest"));
        assert!(result.contains("# Plugin: node-20"));
        assert!(result.contains("setup_20.x"));
    }

    #[test]
    fn generate_dockerfile_multiple_ordered() {
        // Install heroku first, node-20 second — output should be node-20 first
        let plugins = vec!["heroku".to_string(), "node-20".to_string()];
        let result = generate_dockerfile(&plugins).unwrap();
        let node_pos = result.find("# Plugin: node-20").unwrap();
        let heroku_pos = result.find("# Plugin: heroku").unwrap();
        assert!(
            node_pos < heroku_pos,
            "node-20 should appear before heroku in the Dockerfile"
        );
    }

    #[test]
    fn generate_dockerfile_unknown() {
        let plugins = vec!["nonexistent".to_string()];
        assert!(generate_dockerfile(&plugins).is_err());
    }

    #[test]
    fn generate_dockerfile_empty() {
        let plugins: Vec<String> = vec![];
        let result = generate_dockerfile(&plugins).unwrap();
        assert!(result.starts_with("FROM claudine:latest"));
        // Should just be the FROM line and a trailing newline
        assert!(!result.contains("# Plugin:"));
    }

    #[test]
    fn catalog_has_expected_plugins() {
        let cat = catalog();
        let names: Vec<&str> = cat.iter().map(|p| p.name).collect();
        assert!(names.contains(&"node-20"));
        assert!(names.contains(&"node-22"));
        assert!(names.contains(&"node-24"));
        assert!(names.contains(&"heroku"));
        assert!(names.contains(&"python-venv"));
        assert!(names.contains(&"rust"));
    }

    #[test]
    fn heroku_requires_node() {
        let heroku = find("heroku").unwrap();
        assert!(!heroku.requires.is_empty());
        assert!(heroku.requires.contains(&"node-20"));
        assert!(heroku.requires.contains(&"node-22"));
        assert!(heroku.requires.contains(&"node-24"));
    }
}
