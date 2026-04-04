use std::io::{BufRead, BufReader, IsTerminal};
use std::path::PathBuf;
use std::process::Command;

use dialoguer::{Confirm, Input};
use serde::Deserialize;

use crate::{config, plugin, project};

/// Initialize a project. If `flag_repos` are provided, runs non-interactively.
/// Otherwise, prompts for input (requires a TTY).
pub fn cmd_init(name: &str, flag_ssh_key: Option<&str>, flag_repos: &[String], flag_plugins: &[String]) -> anyhow::Result<()> {
    project::validate_name(name)?;

    let interactive = flag_repos.is_empty();

    if interactive && !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "No TTY detected. Use --repo and --ssh-key flags for non-interactive init.\n\
             Example: claudine init {} --ssh-key ~/.ssh/id_rsa --repo git@github.com:user/repo.git",
            name
        );
    }

    // Check if volume already exists
    let volume_already_exists = project::volume_exists(name)?;
    if volume_already_exists && interactive {
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

    // Validate plugins upfront
    for p in flag_plugins {
        if plugin::find(p).is_none() {
            anyhow::bail!(
                "Unknown plugin '{}'. Run 'claudine plugin available' to see options.",
                p
            );
        }
    }
    // Check plugin requirements (order matters — check in order they're given)
    for (i, p) in flag_plugins.iter().enumerate() {
        let installed_so_far: Vec<String> = flag_plugins[..i].to_vec();
        plugin::check_requires(p, &installed_so_far)?;
    }

    // Collect repos
    let repos = if interactive {
        collect_repos_interactive()?
    } else {
        collect_repos_from_flags(flag_repos)?
    };

    execute_init(name, ssh_key, repos, flag_plugins.to_vec())
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
struct SuggestedPlugin {
    name: String,
    reason: String,
}

#[derive(Deserialize)]
struct AgentResult {
    repos: Vec<AgentRepo>,
    plugins: Vec<String>,
    #[serde(default)]
    suggested_plugins: Vec<SuggestedPlugin>,
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

    // Check if volume already exists
    if project::volume_exists(name)? {
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

    // Validate all suggested plugins exist
    for p in &result.plugins {
        if plugin::find(p).is_none() {
            eprintln!("Warning: agent suggested unknown plugin '{}', skipping.", p);
        }
    }
    let mut plugins: Vec<String> = result.plugins.iter()
        .filter(|p| plugin::find(p).is_some())
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
    if plugins.is_empty() {
        println!("Plugins:  (none)");
    } else {
        println!("Plugins:  {}", plugins.join(", "));
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

    if !result.suggested_plugins.is_empty() {
        println!("\n--- Suggested New Plugins ---");
        println!("The following technologies were detected but have no claudine plugin yet:");
        for suggestion in &result.suggested_plugins {
            println!("  {} — {}", suggestion.name, suggestion.reason);
        }
    }

    // Offer to add extra plugins
    let catalog = plugin::catalog();
    let available: Vec<&str> = catalog.iter()
        .map(|p| p.name)
        .filter(|n| !plugins.contains(&n.to_string()))
        .collect();
    if !available.is_empty() {
        println!("\nAvailable plugins: {}", available.join(", "));
        loop {
            let extra: String = Input::new()
                .with_prompt("Add plugin (enter to continue)")
                .default(String::new())
                .show_default(false)
                .interact_text()?;

            let trimmed = extra.trim();
            if trimmed.is_empty() {
                break;
            }

            if plugins.contains(&trimmed.to_string()) {
                println!("'{}' is already included.", trimmed);
            } else if plugin::find(trimmed).is_some() {
                if let Err(e) = plugin::check_requires(trimmed, &plugins) {
                    println!("{}", e);
                } else {
                    plugins.push(trimmed.to_string());
                    println!("Added '{}'.", trimmed);
                }
            } else {
                println!("Unknown plugin '{}'. Available: {}", trimmed, available.join(", "));
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

    // Convert agent repos to config repos, skipping repos with no remote
    let repos: Vec<config::RepoConfig> = result.repos.into_iter()
        .filter_map(|r| {
            r.url.map(|url| config::RepoConfig {
                url,
                dir: r.dir,
                branch: r.branch,
            })
        })
        .collect();

    execute_init(name, ssh_key, repos, plugins)
}

/// Try to detect the SSH key for a set of git remote URLs by parsing ~/.ssh/config.
///
/// Extracts the hostname from `git@host:...` URLs, looks for a matching Host
/// entry in ~/.ssh/config, and returns the IdentityFile path if found. Falls
/// back to common key filenames (~/.ssh/id_ed25519, id_rsa) if they exist.
fn detect_ssh_key(urls: &[&str]) -> Option<String> {
    let home = dirs::home_dir()?;
    let ssh_config = home.join(".ssh/config");

    // Extract unique SSH hostnames from git@ URLs
    let hosts: Vec<&str> = urls.iter()
        .filter_map(|url| {
            url.strip_prefix("git@")
                .and_then(|rest| rest.split(':').next())
        })
        .collect();

    if hosts.is_empty() {
        return None;
    }

    // Parse ~/.ssh/config for matching Host entries
    if let Ok(contents) = std::fs::read_to_string(&ssh_config) {
        let mut current_hosts: Vec<String> = Vec::new();
        let mut identity_file: Option<String> = None;

        for line in contents.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Host ").or_else(|| trimmed.strip_prefix("Host\t")) {
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
                current_hosts = rest.split_whitespace().map(|s| s.to_string()).collect();
                identity_file = None;
            } else if let Some(rest) = trimmed.strip_prefix("IdentityFile ").or_else(|| trimmed.strip_prefix("IdentityFile\t")) {
                identity_file = Some(rest.trim().to_string());
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

/// Parse ~/.ssh/config and return a map of Host alias → HostName.
fn parse_ssh_aliases() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return map,
    };
    let contents = match std::fs::read_to_string(home.join(".ssh/config")) {
        Ok(c) => c,
        Err(_) => return map,
    };

    let mut current_hosts: Vec<String> = Vec::new();
    let mut hostname: Option<String> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Host ").or_else(|| trimmed.strip_prefix("Host\t")) {
            // Flush previous block
            if let Some(ref hn) = hostname {
                for host in &current_hosts {
                    if !host.contains('*') {
                        map.insert(host.clone(), hn.clone());
                    }
                }
            }
            current_hosts = rest.split_whitespace().map(|s| s.to_string()).collect();
            hostname = None;
        } else if let Some(rest) = trimmed.strip_prefix("HostName ").or_else(|| trimmed.strip_prefix("HostName\t")) {
            hostname = Some(rest.trim().to_string());
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
    let mut remotes: Vec<String> = Vec::new();
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

        if remote != "NONE" {
            remotes.push(remote.clone());
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

    // Detect SSH key from remotes
    let remote_refs: Vec<&str> = remotes.iter().map(|s| s.as_str()).collect();
    if let Some(key) = detect_ssh_key(&remote_refs) {
        out.push_str(&format!("\n=== SSH ===\n{}\n", key));
    }

    // List available plugins from catalog
    out.push_str("\n=== PLUGINS ===\n");
    for p in plugin::catalog() {
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

/// Execute the init steps: create volume, setup home, clone repos, build plugins.
fn execute_init(
    name: &str,
    ssh_key: Option<String>,
    repos: Vec<config::RepoConfig>,
    plugins: Vec<String>,
) -> anyhow::Result<()> {
    // Create volume if it does not already exist
    if !project::volume_exists(name)? {
        println!("Creating volume '{}'...", project::volume_name(name));
        project::create_volume(name)?;
    }

    // Create shared directory on the host
    let share_dir = project::share_dir(name)?;
    if !share_dir.exists() {
        std::fs::create_dir_all(&share_dir)
            .map_err(|e| anyhow::anyhow!("Failed to create share directory '{}': {e}", share_dir.display()))?;
        println!("Created share directory: {}", share_dir.display());
    }

    // Build and save project config
    let plugins_opt = if plugins.is_empty() {
        None
    } else {
        Some(plugins)
    };
    let image_override = if plugins_opt.is_some() {
        Some(config::ImageConfig { name: format!("claudine:{}", name) })
    } else {
        None
    };
    let project_config = config::ProjectConfig {
        repos: repos.clone(),
        ssh_key,
        plugins: plugins_opt,
        image: image_override,
    };
    config::save_project(name, &project_config)?;

    // Build project image if plugins were specified (must happen before setup_home)
    if let Some(ref plugin_list) = project_config.plugins {
        if !plugin_list.is_empty() {
            let dockerfile = plugin::generate_dockerfile(plugin_list)?;
            crate::docker::cmd_build_project(name, &dockerfile)?;
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

/// Set up the home directory in the volume with configs, credentials, and settings.
/// Runs a one-shot container with the embedded setup script.
fn setup_home(
    project_name: &str,
    image: &str,
    ssh_key: Option<&str>,
) -> anyhow::Result<()> {
    let volume = project::volume_name(project_name);
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;

    // Write setup script to a temp file
    let mut tmp = tempfile::NamedTempFile::new()
        .map_err(|e| anyhow::anyhow!("Failed to create temp file: {e}"))?;
    std::io::Write::write_all(&mut tmp, SETUP_HOME_SCRIPT.as_bytes())?;

    let mut args: Vec<String> = vec![
        "run".to_string(),
        "--rm".to_string(),
        "-v".to_string(),
        format!("{}:/project", volume),
        "-v".to_string(),
        format!("{}:/tmp/setup-home.sh:ro", tmp.path().display()),
    ];

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

/// Clone a single repository into the project volume.
/// Assumes setup_home has already run, so SSH keys and git config are in the volume.
pub fn clone_repo(
    project_name: &str,
    image: &str,
    repo: &config::RepoConfig,
) -> anyhow::Result<()> {
    let volume = project::volume_name(project_name);

    // Clone command — clone into /project/<dir>
    let clone_target = format!("/project/{}", repo.dir);
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
        "-v".to_string(),
        format!("{}:/project", volume),
        "-e".to_string(),
        "HOME=/project/home".to_string(),
        image.to_string(),
    ];
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
