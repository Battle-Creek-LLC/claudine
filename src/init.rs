use std::io::{BufRead, BufReader, IsTerminal};
use std::path::PathBuf;
use std::process::Command;

use dialoguer::{Confirm, Input};
use serde::Deserialize;

use crate::{config, layer, project};

/// Initialize a project. If `flag_repos` are provided, runs non-interactively.
/// Otherwise, prompts for input (requires a TTY).
pub fn cmd_init(name: &str, flag_ssh_key: Option<&str>, flag_repos: &[String], flag_layers: &[String]) -> anyhow::Result<()> {
    project::validate_name(name)?;

    let interactive = flag_repos.is_empty();

    if interactive && !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "No TTY detected. Use --repo and --ssh-key flags for non-interactive init.\n\
             Example: claudine init {} --ssh-key ~/.ssh/id_rsa --repo git@github.com:user/repo.git",
            name
        );
    }

    // Check if project already exists
    let home_vol = project::home_volume_name(name);
    let volume_already_exists = project::docker_volume_exists(&home_vol)?;
    if volume_already_exists && interactive {
        let proceed = Confirm::new()
            .with_prompt(format!(
                "Project '{}' already initialized. Re-initialize? This will not delete existing data.",
                name
            ))
            .default(false)
            .interact()?;

        if !proceed {
            anyhow::bail!("Init cancelled.");
        }
    }

    // Resolve SSH key
    let ssh_key = if let Some(key) = flag_ssh_key {
        let path = PathBuf::from(key);
        if !path.exists() {
            anyhow::bail!("SSH key not found: {}", path.display());
        }
        Some(path.display().to_string())
    } else if interactive {
        let ssh_key_input: String = Input::new()
            .with_prompt("SSH key path (leave empty for HTTPS repos)")
            .default(String::new())
            .show_default(false)
            .interact_text()?;

        if ssh_key_input.trim().is_empty() {
            None
        } else {
            let path = PathBuf::from(ssh_key_input.trim());
            if !path.exists() {
                anyhow::bail!("SSH key not found: {}", path.display());
            }
            Some(path.display().to_string())
        }
    } else {
        None
    };

    // Validate layers upfront
    for p in flag_layers {
        if layer::find(p).is_none() {
            anyhow::bail!(
                "Unknown layer '{}'. Run 'claudine layer available' to see options.",
                p
            );
        }
    }
    // Check layer requirements (order matters — check in order they're given)
    for (i, p) in flag_layers.iter().enumerate() {
        let installed_so_far: Vec<String> = flag_layers[..i].to_vec();
        layer::check_requires(p, &installed_so_far)?;
    }

    // Collect repos
    let repos = if interactive {
        collect_repos_interactive()?
    } else {
        collect_repos_from_flags(flag_repos)?
    };

    execute_init(name, ssh_key, repos, flag_layers.to_vec())
}

// -- Agent-assisted init --------------------------------------------------

const AGENT_PROMPT: &str = include_str!("../agent-init-prompt.md");

#[derive(Deserialize)]
struct AgentRepo {
    url: Option<String>,
    dir: String,
    branch: Option<String>,
}

#[derive(Deserialize)]
struct SuggestedLayer {
    name: String,
    reason: String,
}

#[derive(Deserialize)]
struct AgentResult {
    repos: Vec<AgentRepo>,
    layers: Vec<String>,
    #[serde(default)]
    suggested_layers: Vec<SuggestedLayer>,
    ssh_key_needed: bool,
}

/// Initialize a project by having Claude analyze a local folder first.
///
/// Runs `claude -p` with the analysis prompt, parses the structured JSON output,
/// shows the user a summary, and proceeds with init after confirmation.
pub fn cmd_init_agent(name: &str, agent_path: &str, flag_ssh_key: Option<&str>) -> anyhow::Result<()> {
    project::validate_name(name)?;

    // Validate the target path
    let path = PathBuf::from(agent_path);
    if !path.exists() {
        anyhow::bail!("Path not found: {}", agent_path);
    }
    if !path.is_dir() {
        anyhow::bail!("Not a directory: {}", agent_path);
    }

    // Check claude is available on the host
    which::which("claude")
        .map_err(|_| anyhow::anyhow!("'claude' not found on PATH. Install Claude Code first."))?;

    // Check if project already exists
    if project::docker_volume_exists(&project::home_volume_name(name))? {
        let proceed = Confirm::new()
            .with_prompt(format!(
                "Project '{}' already initialized. Re-initialize? This will not delete existing data.",
                name
            ))
            .default(false)
            .interact()?;

        if !proceed {
            anyhow::bail!("Init cancelled.");
        }
    }

    // Phase 1: Fast pre-scan to gather all filesystem data
    println!("Scanning {}...", agent_path);
    let scan = run_prescan(&path)?;
    println!("{}", scan);

    // Phase 2: Claude interprets the pre-scan data (no tool calls needed)
    println!("Analyzing...\n");
    let prompt = format!("{}\n\n<prescan>\n{}\n</prescan>", AGENT_PROMPT, scan);
    let response_text = run_agent_claude(&prompt, &path)?;

    // Print the full analysis
    println!("\n{}", response_text);

    // Extract the JSON block from the response
    let result = extract_agent_json(&response_text)?;

    if result.repos.is_empty() {
        anyhow::bail!("No repositories found in the analysis.");
    }

    // Validate all suggested layers exist
    for p in &result.layers {
        if layer::find(p).is_none() {
            eprintln!("Warning: agent suggested unknown layer '{}', skipping.", p);
        }
    }
    let mut layers: Vec<String> = result.layers.iter()
        .filter(|p| layer::find(p).is_some())
        .cloned()
        .collect();

    // Show summary
    println!("\n--- Init Plan ---");
    println!("Project:  {}", name);
    println!("Repos:    {}", result.repos.len());
    for repo in &result.repos {
        let branch = repo.branch.as_deref().unwrap_or("(default)");
        match &repo.url {
            Some(url) => println!("          {} → {} [{}]", repo.dir, url, branch),
            None => println!("          {} (local only, skipping)", repo.dir),
        }
    }
    if layers.is_empty() {
        println!("Layers:   (none)");
    } else {
        println!("Layers:   {}", layers.join(", "));
    }
    let prescan_ssh_key = scan.lines()
        .skip_while(|l| !l.starts_with("=== SSH ==="))
        .nth(1)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if result.ssh_key_needed {
        if let Some(ref key) = prescan_ssh_key {
            println!("SSH key:  {}", key);
        } else {
            println!("SSH:      required (no key auto-detected)");
        }
    } else {
        println!("SSH:      not needed");
    }

    if !result.suggested_layers.is_empty() {
        println!("\n--- Suggested New Plugins ---");
        println!("The following technologies were detected but have no claudine layer yet:");
        for suggestion in &result.suggested_layers {
            println!("  {} — {}", suggestion.name, suggestion.reason);
        }
    }

    // Offer to add extra layers
    let catalog = layer::catalog();
    let available: Vec<&str> = catalog.iter()
        .map(|p| p.name)
        .filter(|n| !layers.contains(&n.to_string()))
        .collect();
    if !available.is_empty() {
        println!("\nAvailable layers: {}", available.join(", "));
        loop {
            let extra: String = Input::new()
                .with_prompt("Add layer (enter to continue)")
                .default(String::new())
                .show_default(false)
                .interact_text()?;

            let trimmed = extra.trim();
            if trimmed.is_empty() {
                break;
            }

            if layers.contains(&trimmed.to_string()) {
                println!("'{}' is already included.", trimmed);
            } else if layer::find(trimmed).is_some() {
                if let Err(e) = layer::check_requires(trimmed, &layers) {
                    println!("{}", e);
                } else {
                    layers.push(trimmed.to_string());
                    println!("Added '{}'.", trimmed);
                }
            } else {
                println!("Unknown layer '{}'. Available: {}", trimmed, available.join(", "));
            }
        }
    }

    // Resolve SSH key
    let ssh_key = if result.ssh_key_needed {
        if let Some(key) = flag_ssh_key {
            let key_path = PathBuf::from(key);
            if !key_path.exists() {
                anyhow::bail!("SSH key not found: {}", key_path.display());
            }
            Some(key_path.display().to_string())
        } else {
            // Use SSH key detected during prescan
            let detected = prescan_ssh_key.clone();

            let ssh_key_input: String = if let Some(ref key) = detected {
                Input::new()
                    .with_prompt("SSH key path")
                    .default(key.clone())
                    .interact_text()?
            } else {
                Input::new()
                    .with_prompt("SSH key path")
                    .interact_text()?
            };

            let trimmed = ssh_key_input.trim();
            if trimmed.is_empty() {
                None
            } else {
                let key_path = PathBuf::from(trimmed);
                if !key_path.exists() {
                    anyhow::bail!("SSH key not found: {}", key_path.display());
                }
                Some(key_path.display().to_string())
            }
        }
    } else {
        flag_ssh_key.map(|s| s.to_string())
    };

    // Confirm
    let proceed = Confirm::new()
        .with_prompt("Proceed with init?")
        .default(true)
        .interact()?;

    if !proceed {
        anyhow::bail!("Init cancelled.");
    }

    // Convert agent repos to config repos, resolving SSH aliases and skipping repos with no remote
    let ssh_aliases = parse_ssh_aliases();
    let repos: Vec<config::RepoConfig> = result.repos.into_iter()
        .filter_map(|r| {
            r.url.map(|url| config::RepoConfig {
                url: resolve_ssh_alias(&url, &ssh_aliases),
                dir: r.dir,
                branch: r.branch,
            })
        })
        .collect();

    execute_init(name, ssh_key, repos, layers)
}

/// Try to detect the SSH key for a set of git remote URLs by parsing ~/.ssh/config.
///
/// Extracts the hostname from `git@host:...` URLs, looks for a matching Host
/// entry in ~/.ssh/config, and returns the IdentityFile path if found. Falls
/// back to common key filenames (~/.ssh/id_ed25519, id_rsa) if they exist.
fn detect_ssh_key(urls: &[&str]) -> Option<String> {
    let home = dirs::home_dir()?;
    let ssh_config = home.join(".ssh/config");

    if let Ok(contents) = std::fs::read_to_string(&ssh_config) {
        if let Some(key) = detect_ssh_key_from_config(&contents, urls, &home) {
            return Some(key);
        }
    }

    // Fallback: check common key filenames
    for name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
        let path = home.join(".ssh").join(name);
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }

    None
}

/// Parse SSH config text and find the identity file for the given remote URLs.
/// Returns the expanded path if found and the file exists.
fn detect_ssh_key_from_config(
    contents: &str,
    urls: &[&str],
    home: &std::path::Path,
) -> Option<String> {
    let hosts: Vec<&str> = urls.iter()
        .filter_map(|url| {
            url.strip_prefix("git@")
                .and_then(|rest| rest.split(':').next())
        })
        .collect();

    if hosts.is_empty() {
        return None;
    }

    let mut current_hosts: Vec<String> = Vec::new();
    let mut identity_file: Option<String> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        // Check "hostname" and "identityfile" before "host" since "host" is a prefix of "hostname"
        if lower.starts_with("hostname ") || lower.starts_with("hostname\t") {
            // Skip — not relevant for key detection
        } else if lower.starts_with("identityfile ") || lower.starts_with("identityfile\t") {
            identity_file = Some(trimmed[12..].trim().to_string());
        } else if lower.starts_with("host ") || lower.starts_with("host\t") {
            // Check if the previous block matched
            if let Some(ref key) = identity_file {
                if hosts.iter().any(|h| current_hosts.iter().any(|ch| ch == h)) {
                    let expanded = key.replace("~", &home.to_string_lossy());
                    let path = PathBuf::from(&expanded);
                    if path.exists() {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
            current_hosts = trimmed[4..].split_whitespace().map(|s| s.to_string()).collect();
            identity_file = None;
        }
    }
    // Check final block
    if let Some(ref key) = identity_file {
        if hosts.iter().any(|h| current_hosts.iter().any(|ch| ch == h)) {
            let expanded = key.replace("~", &home.to_string_lossy());
            let path = PathBuf::from(&expanded);
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

/// Parse ~/.ssh/config and return a map of Host alias → HostName.
fn parse_ssh_aliases() -> std::collections::HashMap<String, String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return std::collections::HashMap::new(),
    };
    let contents = match std::fs::read_to_string(home.join(".ssh/config")) {
        Ok(c) => c,
        Err(_) => return std::collections::HashMap::new(),
    };
    parse_ssh_config_aliases(&contents)
}

/// Parse SSH config text and return a map of Host alias → HostName.
fn parse_ssh_config_aliases(contents: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut current_hosts: Vec<String> = Vec::new();
    let mut hostname: Option<String> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        // Check "hostname" before "host" since "host" is a prefix of "hostname"
        if lower.starts_with("hostname ") || lower.starts_with("hostname\t") {
            hostname = Some(trimmed[8..].trim().to_string());
        } else if lower.starts_with("host ") || lower.starts_with("host\t") {
            // Flush previous block
            if let Some(ref hn) = hostname {
                for host in &current_hosts {
                    if !host.contains('*') {
                        map.insert(host.clone(), hn.clone());
                    }
                }
            }
            current_hosts = trimmed[4..].split_whitespace().map(|s| s.to_string()).collect();
            hostname = None;
        }
    }
    // Flush final block
    if let Some(ref hn) = hostname {
        for host in &current_hosts {
            if !host.contains('*') {
                map.insert(host.clone(), hn.clone());
            }
        }
    }

    map
}

/// Replace SSH host aliases in a git remote URL with the real hostname.
fn resolve_ssh_alias(url: &str, aliases: &std::collections::HashMap<String, String>) -> String {
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some(colon_pos) = rest.find(':') {
            let host = &rest[..colon_pos];
            if let Some(real_host) = aliases.get(host) {
                return format!("git@{}:{}", real_host, &rest[colon_pos + 1..]);
            }
        }
    }
    url.to_string()
}

/// Pre-scan a directory to collect git repo info and tech stack indicators.
fn run_prescan(target: &std::path::Path) -> anyhow::Result<String> {
    let mut out = String::new();

    // Find .git directories at root and up to two levels deep (skip worktree .git files)
    let mut repos: Vec<(String, PathBuf)> = Vec::new();
    if target.join(".git").is_dir() {
        repos.push((".".to_string(), target.to_path_buf()));
    }
    for depth_1 in std::fs::read_dir(target).into_iter().flatten().flatten() {
        if !depth_1.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let d1_path = depth_1.path();
        let d1_name = depth_1.file_name().to_string_lossy().to_string();
        if d1_path.join(".git").is_dir() {
            repos.push((d1_name.clone(), d1_path.clone()));
        }
        // Check one level deeper (e.g. python/master)
        for depth_2 in std::fs::read_dir(&d1_path).into_iter().flatten().flatten() {
            if !depth_2.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let d2_path = depth_2.path();
            if d2_path.join(".git").is_dir() {
                let d2_name = depth_2.file_name().to_string_lossy().to_string();
                repos.push((format!("{}/{}", d1_name, d2_name), d2_path));
            }
        }
    }
    repos.sort_by(|a, b| a.0.cmp(&b.0));

    // Collect repo info, resolving SSH host aliases to real hostnames
    let ssh_aliases = parse_ssh_aliases();
    let mut raw_remotes: Vec<String> = Vec::new();
    out.push_str("=== REPOS ===\n");
    for (name, path) in &repos {
        let raw_remote = Command::new("git")
            .args(["-C", &path.to_string_lossy(), "remote", "get-url", "origin"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "NONE".to_string());

        let remote = if raw_remote != "NONE" {
            resolve_ssh_alias(&raw_remote, &ssh_aliases)
        } else {
            raw_remote.clone()
        };

        if raw_remote != "NONE" {
            raw_remotes.push(raw_remote);
        }

        let branch = Command::new("git")
            .args(["-C", &path.to_string_lossy(), "branch", "--show-current"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // Recent branch names (up to 10, sorted by last commit date)
        let branches = Command::new("git")
            .args(["-C", &path.to_string_lossy(), "branch", "--sort=-committerdate", "--format=%(refname:short)"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();

        out.push_str(&format!("{}|{}|{}|{}\n", name, remote, branch, branches));
    }

    // Collect tech stack indicators
    out.push_str("\n=== STACK ===\n");
    let indicators: &[(&str, &str)] = &[
        ("package.json", "package.json"),
        ("pyproject.toml", "pyproject.toml"),
        ("requirements.txt", "requirements.txt"),
        ("Pipfile", "Pipfile"),
        ("Cargo.toml", "Cargo.toml"),
        ("go.mod", "go.mod"),
        ("pom.xml", "pom.xml"),
        ("build.gradle", "build.gradle"),
        ("build.gradle.kts", "build.gradle.kts"),
        ("flyway.toml", "flyway.toml"),
        ("flyway.conf", "flyway.conf"),
        ("Procfile", "Procfile"),
        ("heroku.yml", "heroku.yml"),
        ("justfile", "justfile"),
        ("Makefile", "Makefile"),
        (".gitlab-ci.yml", ".gitlab-ci.yml"),
        (".do/app.yaml", "digitalocean-app"),
        ("do.yaml", "digitalocean-app"),
    ];
    let dir_indicators: &[(&str, &str)] = &[
        (".github", ".github/"),
        (".do", ".do/"),
    ];

    for (name, path) in &repos {
        let mut found: Vec<String> = Vec::new();

        for (file, label) in indicators {
            if path.join(file).is_file() {
                let mut entry = label.to_string();
                // Extract Node.js engine version and detect frontend deps
                if *file == "package.json" {
                    if let Ok(contents) = std::fs::read_to_string(path.join(file)) {
                        if let Some(engines) = contents.find("\"node\"") {
                            let snippet = &contents[engines..];
                            if let Some(end) = snippet.find('}') {
                                let chunk = &snippet[..end];
                                if let Some(start) = chunk.find('"') {
                                    let rest = &chunk[start + 1..];
                                    if let Some(q) = rest.find('"') {
                                        let rest2 = &rest[q + 1..];
                                        if let Some(vs) = rest2.find('"') {
                                            let ve = rest2[vs + 1..].find('"').unwrap_or(0);
                                            let version = &rest2[vs + 1..vs + 1 + ve];
                                            entry = format!("package.json (node: \"{}\")", version);
                                        }
                                    }
                                }
                            }
                        }
                        // Detect frontend projects
                        const FRONTEND_DEPS: &[&str] = &[
                            "\"react\"", "\"vue\"", "\"@angular/core\"", "\"svelte\"",
                            "\"next\"", "\"nuxt\"", "\"gatsby\"", "\"@remix-run/",
                            "\"vite\"", "\"webpack\"", "\"parcel\"",
                        ];
                        if FRONTEND_DEPS.iter().any(|dep| contents.contains(dep)) {
                            found.push("frontend".to_string());
                        }
                    }
                }
                found.push(entry);
            }
        }

        for (dir, label) in dir_indicators {
            if path.join(dir).is_dir() {
                found.push(label.to_string());
            }
        }

        // Check for .nvmrc / .node-version
        for nv in &[".nvmrc", ".node-version"] {
            let nv_path = path.join(nv);
            if nv_path.is_file() {
                if let Ok(v) = std::fs::read_to_string(&nv_path) {
                    found.push(format!("{}={}", nv, v.trim()));
                }
            }
        }

        // Check for terraform .tf files
        if let Ok(entries) = std::fs::read_dir(path) {
            if entries.flatten().any(|e| {
                e.file_name().to_string_lossy().ends_with(".tf")
            }) {
                found.push("terraform".to_string());
            }
        }

        // Check for playwright config
        if let Ok(entries) = std::fs::read_dir(path) {
            if entries.flatten().any(|e| {
                e.file_name().to_string_lossy().starts_with("playwright.config")
            }) {
                found.push("playwright".to_string());
            }
        }

        if !found.is_empty() {
            out.push_str(&format!("{}: {}\n", name, found.join(" ")));
        }
    }

    if repos.is_empty() {
        anyhow::bail!("No git repositories found in {}", target.display());
    }

    // Detect SSH key from raw remotes (before alias resolution, so Host aliases match)
    let remote_refs: Vec<&str> = raw_remotes.iter().map(|s| s.as_str()).collect();
    if let Some(key) = detect_ssh_key(&remote_refs) {
        out.push_str(&format!("\n=== SSH ===\n{}\n", key));
    }

    // List available layers from catalog
    out.push_str("\n=== LAYERS ===\n");
    for p in layer::catalog() {
        if p.requires.is_empty() {
            out.push_str(&format!("{} — {}\n", p.name, p.description));
        } else {
            out.push_str(&format!("{} — {} (requires: {})\n", p.name, p.description, p.requires.join(", ")));
        }
    }

    Ok(out)
}

/// Run `claude -p` with stream-json output, printing one-line tool summaries as
/// Claude works, and return the final response text.
fn run_agent_claude(prompt: &str, cwd: &std::path::Path) -> anyhow::Result<String> {
    let mut child = Command::new("claude")
        .args(["-p", prompt, "--output-format", "stream-json", "--verbose"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run 'claude -p': {e}"))?;

    let stdout = child.stdout.take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture claude stdout"))?;
    let reader = BufReader::new(stdout);
    let mut result_text = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| anyhow::anyhow!("Failed to read claude output: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }

        let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            "assistant" => {
                // Look for tool_use blocks in message content
                if let Some(blocks) = event.pointer("/message/content").and_then(|c| c.as_array()) {
                    for block in blocks {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let tool = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                            let summary = format_tool_summary(tool, block);
                            println!("  → {}", summary);
                        }
                    }
                }
            }
            "result" => {
                if let Some(text) = event.get("result").and_then(|r| r.as_str()) {
                    result_text = text.to_string();
                }
            }
            _ => {}
        }
    }

    let status = child.wait()
        .map_err(|e| anyhow::anyhow!("Failed to wait for claude: {e}"))?;

    if !status.success() {
        anyhow::bail!("Claude analysis failed (exit code: {}).", status);
    }

    if result_text.is_empty() {
        anyhow::bail!("No result received from Claude.");
    }

    Ok(result_text)
}

/// Format a one-line summary for a tool call event.
fn format_tool_summary(tool: &str, block: &serde_json::Value) -> String {
    // Pick the first short string value from the input object as context
    let detail = block.get("input")
        .and_then(|input| input.as_object())
        .and_then(|obj| {
            obj.values()
                .filter_map(|v| v.as_str())
                .next()
                .map(|s| s.lines().next().unwrap_or(s))
        })
        .unwrap_or("");

    if detail.is_empty() {
        return tool.to_string();
    }

    let line = format!("{}: {}", tool, detail);
    if line.len() > 72 {
        format!("{}...", &line[..69])
    } else {
        line
    }
}

/// Extract the last ```json fenced block from Claude's output and parse it.
fn extract_agent_json(text: &str) -> anyhow::Result<AgentResult> {
    let start = text.rfind("```json")
        .ok_or_else(|| anyhow::anyhow!("No JSON block found in Claude's response."))?;
    let json_start = start + "```json".len();
    let remaining = &text[json_start..];
    let json_end = remaining.find("```")
        .ok_or_else(|| anyhow::anyhow!("Unterminated JSON block in Claude's response."))?;
    let json_str = remaining[..json_end].trim();

    serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse init parameters from Claude's response: {e}"))
}

// -- Shared init execution ------------------------------------------------

/// Execute the init steps: create host dir + home volume, setup home, clone repos, build layers.
fn execute_init(
    name: &str,
    ssh_key: Option<String>,
    repos: Vec<config::RepoConfig>,
    layers: Vec<String>,
) -> anyhow::Result<()> {
    // Host directory: ~/projects/<name>/ — repos get cloned here and this is
    // bind-mounted into the container at /project.
    let host_dir = project::default_host_dir(name)?;
    if !host_dir.exists() {
        std::fs::create_dir_all(&host_dir)
            .map_err(|e| anyhow::anyhow!("Failed to create host directory '{}': {e}", host_dir.display()))?;
        println!("Created host directory: {}", host_dir.display());
    }

    // Create the HOME volume (mounted at /home/claude)
    let home_volume = project::home_volume_name(name);
    if !project::docker_volume_exists(&home_volume)? {
        println!("Creating home volume '{}'...", home_volume);
        project::docker_volume_create(&home_volume)?;
    }

    // Build and save project config
    let layers_opt = if layers.is_empty() {
        None
    } else {
        Some(layers)
    };
    let image_override = if layers_opt.is_some() {
        Some(config::ImageConfig { name: format!("claudine:{}", name) })
    } else {
        None
    };
    let project_config = config::ProjectConfig {
        repos: repos.clone(),
        ssh_key,
        layers: layers_opt,
        image: image_override,
        host_dir: Some(host_dir.to_string_lossy().to_string()),
    };
    config::save_project(name, &project_config)?;

    // Build project image if layers were specified (must happen before setup_home)
    if let Some(ref layer_list) = project_config.layers {
        if !layer_list.is_empty() {
            let dockerfile = layer::generate_dockerfile(layer_list)?;
            crate::docker::cmd_build_project(name, &dockerfile, false)?;
        }
    }

    // Resolve the image name from global config
    let global_config = config::load_global()?;
    let image = config::resolve_image(&project_config, &global_config);

    // Set up home directory with configs, credentials, and settings
    println!("Setting up home directory...");
    setup_home(name, &image, project_config.ssh_key.as_deref())?;

    // Clone each repo
    for repo in &repos {
        clone_repo(name, &image, repo)?;
    }

    // Generate devcontainer.json for Zed integration
    if let Err(e) = crate::devcontainer::write(name, None) {
        eprintln!("Warning: failed to generate devcontainer.json: {e}");
    }

    println!("Project '{}' initialized successfully.", name);
    Ok(())
}

// -- Interactive repo collection ------------------------------------------

/// Collect repos interactively via dialoguer prompts.
fn collect_repos_interactive() -> anyhow::Result<Vec<config::RepoConfig>> {
    let mut repos = Vec::new();

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
        if url.starts_with('-') {
            anyhow::bail!("Repository URL cannot start with '-'.");
        }
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
        project::validate_dir(&dir)?;

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

    Ok(repos)
}

/// Build repo configs from CLI flags. Dir is derived from URL, branch is always default.
fn collect_repos_from_flags(urls: &[String]) -> anyhow::Result<Vec<config::RepoConfig>> {
    if urls.is_empty() {
        anyhow::bail!("At least one --repo is required.");
    }

    let mut repos = Vec::new();
    for url in urls {
        if url.starts_with('-') {
            anyhow::bail!("Repository URL cannot start with '-'.");
        }
        let dir = config::repo_dir_from_url(url);
        project::validate_dir(&dir)?;
        repos.push(config::RepoConfig {
            url: url.clone(),
            dir,
            branch: None,
        });
    }

    Ok(repos)
}

// -- Home setup and repo cloning ------------------------------------------

const SETUP_HOME_SCRIPT: &str = include_str!("../setup-home.sh");

/// Set up the home directory in the home volume with configs, credentials, and settings.
/// Runs a one-shot container with the embedded setup script.
fn setup_home(
    project_name: &str,
    image: &str,
    ssh_key: Option<&str>,
) -> anyhow::Result<()> {
    let project_config = config::load_project(project_name).ok();
    let host_dir = project_config.as_ref().and_then(|c| c.host_dir.clone());
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;

    // Write setup script to a temp file
    let mut tmp = tempfile::NamedTempFile::new()
        .map_err(|e| anyhow::anyhow!("Failed to create temp file: {e}"))?;
    std::io::Write::write_all(&mut tmp, SETUP_HOME_SCRIPT.as_bytes())?;

    let mut args: Vec<String> = vec![
        "run".to_string(),
        "--rm".to_string(),
    ];

    let dir = host_dir.as_deref().map(std::path::Path::new)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            project::default_host_dir(project_name)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    args.extend([
        "-v".to_string(),
        format!("{}:{}", dir, dir),
        "-v".to_string(),
        format!("{}:/home/claude", project::home_volume_name(project_name)),
    ]);

    args.extend([
        "-v".to_string(),
        format!("{}:/tmp/setup-home.sh:ro", tmp.path().display()),
    ]);

    // Mount host configs for the setup script to copy
    let gitconfig = home.join(".gitconfig");
    if gitconfig.exists() {
        args.extend(["-v".to_string(), format!("{}:/tmp/host-gitconfig:ro", gitconfig.display())]);
    }

    if let Some(key_path) = ssh_key {
        args.extend(["-v".to_string(), format!("{}:/tmp/host-ssh-key:ro", key_path)]);
    }

    args.extend([
        "--entrypoint".to_string(), "bash".to_string(),
        image.to_string(),
        "/tmp/setup-home.sh".to_string(),
    ]);

    let status = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run setup container: {e}"))?;

    if !status.success() {
        anyhow::bail!("Home directory setup failed.");
    }

    Ok(())
}

/// Clone a single repository into the project's host directory.
/// Assumes setup_home has already run so SSH keys and git config are in the home volume.
pub fn clone_repo(
    project_name: &str,
    image: &str,
    repo: &config::RepoConfig,
) -> anyhow::Result<()> {
    let project_config = config::load_project(project_name).ok();
    let host_dir = project_config.as_ref().and_then(|c| c.host_dir.clone());

    let clone_target = match &host_dir {
        Some(dir) => format!("{}/{}", dir, repo.dir),
        None => format!("/project/{}", repo.dir),
    };
    let mut clone_cmd = vec!["git".to_string(), "clone".to_string()];
    if let Some(ref b) = repo.branch {
        clone_cmd.push("--branch".to_string());
        clone_cmd.push(b.clone());
    }
    clone_cmd.push("--".to_string());
    clone_cmd.push(repo.url.clone());
    clone_cmd.push(clone_target);

    let mut args: Vec<String> = vec![
        "run".to_string(),
        "--rm".to_string(),
    ];

    let bind_dir = host_dir.as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            project::default_host_dir(project_name)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    args.extend([
        "-v".to_string(),
        format!("{}:{}", bind_dir, bind_dir),
        "-v".to_string(),
        format!("{}:/home/claude", project::home_volume_name(project_name)),
    ]);

    // OpenSSH resolves ~/.ssh via pw->pw_dir (getpwuid), not $HOME, so the
    // claude user's passwd home (/home/claude) is always used regardless of
    // the HOME env var. Point git at the correct key and known_hosts directly.
    let has_ssh_key = project_config.as_ref()
        .and_then(|c| c.ssh_key.as_ref())
        .is_some();
    let git_ssh_cmd = if has_ssh_key {
        "ssh -i /home/claude/.ssh/id_key -o IdentitiesOnly=yes \
-o StrictHostKeyChecking=accept-new \
-o UserKnownHostsFile=/home/claude/.ssh/known_hosts"
    } else {
        "ssh -o StrictHostKeyChecking=accept-new"
    };

    args.extend([
        "-e".to_string(),
        "HOME=/home/claude".to_string(),
        "-e".to_string(),
        format!("GIT_SSH_COMMAND={}", git_ssh_cmd),
        image.to_string(),
    ]);
    args.extend(clone_cmd);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_aliases_lowercase_hostname() {
        let config = "\
Host advicecloud-github
    Hostname github.com
    User git
    IdentityFile ~/.ssh/advicecloud
";
        let aliases = parse_ssh_config_aliases(config);
        assert_eq!(aliases.get("advicecloud-github").unwrap(), "github.com");
    }

    #[test]
    fn parse_ssh_aliases_camelcase_hostname() {
        let config = "\
Host myalias
    HostName gitlab.example.com
    IdentityFile ~/.ssh/id_rsa
";
        let aliases = parse_ssh_config_aliases(config);
        assert_eq!(aliases.get("myalias").unwrap(), "gitlab.example.com");
    }

    #[test]
    fn parse_ssh_aliases_multiple_hosts() {
        let config = "\
Host work-gh
    Hostname github.com
    IdentityFile ~/.ssh/work

Host personal-gh
    Hostname github.com
    IdentityFile ~/.ssh/personal

Host staging
    Hostname staging.example.com
";
        let aliases = parse_ssh_config_aliases(config);
        assert_eq!(aliases.get("work-gh").unwrap(), "github.com");
        assert_eq!(aliases.get("personal-gh").unwrap(), "github.com");
        assert_eq!(aliases.get("staging").unwrap(), "staging.example.com");
    }

    #[test]
    fn parse_ssh_aliases_skips_wildcards() {
        let config = "\
Host *
    ServerAliveInterval 60

Host myhost
    Hostname real.host.com
";
        let aliases = parse_ssh_config_aliases(config);
        assert!(!aliases.contains_key("*"));
        assert_eq!(aliases.get("myhost").unwrap(), "real.host.com");
    }

    #[test]
    fn parse_ssh_aliases_empty_config() {
        let aliases = parse_ssh_config_aliases("");
        assert!(aliases.is_empty());
    }

    #[test]
    fn parse_ssh_aliases_no_hostname() {
        let config = "\
Host nohost
    User git
    IdentityFile ~/.ssh/key
";
        let aliases = parse_ssh_config_aliases(config);
        assert!(!aliases.contains_key("nohost"));
    }

    #[test]
    fn resolve_alias_matches() {
        let mut aliases = std::collections::HashMap::new();
        aliases.insert("advicecloud-github".to_string(), "github.com".to_string());

        assert_eq!(
            resolve_ssh_alias("git@advicecloud-github:AdviceCloud/db.git", &aliases),
            "git@github.com:AdviceCloud/db.git"
        );
    }

    #[test]
    fn resolve_alias_no_match() {
        let aliases = std::collections::HashMap::new();
        let url = "git@github.com:user/repo.git";
        assert_eq!(resolve_ssh_alias(url, &aliases), url);
    }

    #[test]
    fn resolve_alias_https_passthrough() {
        let mut aliases = std::collections::HashMap::new();
        aliases.insert("something".to_string(), "other.com".to_string());

        let url = "https://github.com/user/repo.git";
        assert_eq!(resolve_ssh_alias(url, &aliases), url);
    }

    #[test]
    fn resolve_alias_ssh_protocol() {
        let aliases = std::collections::HashMap::new();
        let url = "ssh://git@example.com/repo.git";
        assert_eq!(resolve_ssh_alias(url, &aliases), url);
    }

    #[test]
    fn detect_key_lowercase_hostname() {
        // This is the exact pattern that caused the bug: Hostname (lowercase n)
        // was being caught by the "host " prefix check instead of being skipped
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let key_path = tmp.path().to_string_lossy().to_string();

        let config = format!("\
Host advicecloud-github
    Hostname github.com
    User git
    IdentityFile {}
", key_path);

        let urls = &["git@advicecloud-github:Org/repo.git"];
        let home = PathBuf::from("/dummy");
        let result = detect_ssh_key_from_config(&config, urls, &home);
        assert_eq!(result.unwrap(), key_path);
    }

    #[test]
    fn detect_key_camelcase_hostname() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let key_path = tmp.path().to_string_lossy().to_string();

        let config = format!("\
Host myalias
    HostName example.com
    IdentityFile {}
", key_path);

        let urls = &["git@myalias:user/repo.git"];
        let home = PathBuf::from("/dummy");
        let result = detect_ssh_key_from_config(&config, urls, &home);
        assert_eq!(result.unwrap(), key_path);
    }

    #[test]
    fn detect_key_no_match() {
        let config = "\
Host other-host
    Hostname other.com
    IdentityFile ~/.ssh/other_key
";
        let urls = &["git@myhost:user/repo.git"];
        let home = PathBuf::from("/nonexistent");
        let result = detect_ssh_key_from_config(config, urls, &home);
        assert!(result.is_none());
    }

    #[test]
    fn detect_key_https_urls_ignored() {
        let config = "\
Host myhost
    Hostname example.com
    IdentityFile ~/.ssh/key
";
        let urls = &["https://github.com/user/repo.git"];
        let home = PathBuf::from("/dummy");
        let result = detect_ssh_key_from_config(config, urls, &home);
        assert!(result.is_none());
    }

    #[test]
    fn detect_key_with_tilde_expansion() {
        let home = dirs::home_dir().unwrap();
        // Use a key we know exists on this machine
        let id_rsa = home.join(".ssh/id_rsa");
        if !id_rsa.exists() {
            return; // skip if no id_rsa
        }

        let config = "\
Host testhost
    Hostname test.com
    IdentityFile ~/.ssh/id_rsa
";
        let urls = &["git@testhost:user/repo.git"];
        let result = detect_ssh_key_from_config(config, urls, &home);
        assert!(result.is_some());
        assert!(result.unwrap().ends_with(".ssh/id_rsa"));
    }
}
