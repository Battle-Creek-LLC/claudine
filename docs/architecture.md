# Claudine Architecture

Claudine is a standalone CLI tool that runs Claude Code inside isolated Docker containers, with per-project persistent volumes and automatic host config forwarding.

## Overview

```
Host                                    Container (claudine:latest)
────────────────────────                ────────────────────────────────
~/.gitconfig ──────┐
~/.ssh/<key> ──────┤ one-shot setup    Copied into volume at init time
~/.claude/ ────────┘ (claudine init)   by setup-home.sh
                                             │
Docker volume                                ▼
  claudine_<project> ──────── mounted ──► /project/
  ├── home/                                ├── home/       ($HOME)
  │   ├── .claude/                         │   ├── .claude/
  │   ├── .ssh/id_key                      │   ├── .ssh/id_key
  │   └── .gitconfig                       │   └── .gitconfig
  ├── <repo1>/                             ├── <repo1>/
  └── <repo2>/                             └── <repo2>/

~/claudine-share/<project>/ ── mounted ──► /share/   (host ↔ container)

/var/run/docker.sock ──────── mounted ──► /var/run/docker.sock (DooD)
```

## Components

### CLI (`claudine`)

Rust binary built with `clap` for argument parsing and `serde` + `toml` for config. Distributed as a single binary via `cargo install` or direct copy.

**Dependencies (Cargo.toml):**
- `clap` — CLI argument parsing with derive macros, shell completions
- `serde` + `toml` — config serialization/deserialization
- `dialoguer` — interactive prompts during init
- `which` — Docker binary detection

**Command structure (clap subcommands):**
```
claudine init <project>                →  create volume, clone repo(s)
claudine run <project> [repo] [-- …]  →  run Claude Code (default action)
claudine shell <project> [repo]        →  open bash shell
claudine destroy <project>             →  remove volume + config
claudine repo add <project> <url>      →  add a repo to a project
claudine repo remove <project> <dir>   →  remove a repo from a project
claudine repo list <project>           →  list repos in a project
claudine build                         →  build/rebuild the Docker image
claudine list                          →  list projects and their status
claudine completions <shell>           →  generate shell completions
```

Project name is always a positional argument to a subcommand — standard clap derive pattern, no disambiguation hacks.

**Embedded build assets:**
The `Dockerfile` and `entrypoint.sh` are embedded into the binary at compile time via `include_str!`. When `claudine build` runs, it writes them to a temp directory and runs `docker build` from there. This makes the binary fully self-contained — no need to locate the source repo or pull from a registry.

```rust
const DOCKERFILE: &str = include_str!("../Dockerfile");
const ENTRYPOINT: &str = include_str!("../entrypoint.sh");
```

**Module layout:**
```
src/
├── main.rs          # clap app definition, command routing
├── cli.rs           # clap derive structs
├── config.rs        # TOML config loading/saving, defaults, migration
├── docker.rs        # Docker command assembly, execution, and embedded build
├── init.rs          # interactive project init flow (multi-repo)
├── project.rs       # project validation, volume/container helpers
└── repo.rs          # repo add/remove/list subcommands

setup-home.sh        # embedded script for home directory setup at init time
```

### Docker Image (`Dockerfile`)

Generic, project-agnostic image based on Debian bookworm:

| Layer | Contents |
|-------|----------|
| Base | `debian:bookworm` |
| System | `ca-certificates curl gnupg gosu git openssh-client python3 python3-pip vim` |
| Docker CLI | `docker-ce-cli docker-buildx-plugin docker-compose-plugin` (DooD pattern) |
| Claude Code | Native installer via `claude.ai/install.sh` |
| Ward | PII/secrets scanner for Claude Code hooks |
| User | Non-root `claude` user with home at `/project/home` |
| Alias | `claude="claude --dangerously-skip-permissions"` |

The image contains no project-specific tooling. Additional tools can be layered via custom Dockerfiles that extend `claudine:latest`.

### Entrypoint (`entrypoint.sh`)

Runs as root, performs minimal runtime setup, then drops to the `claude` user via `gosu`.

**Sequence:**
1. Detect Docker socket GID, add `claude` user to matching group (for DooD access)
2. Ensure `~/.local/bin` is on PATH
3. `exec gosu claude "$@"` (or `bash` if no args)

The entrypoint is intentionally minimal. All home directory setup (config copying, SSH key installation, Claude credentials) is handled by `setup-home.sh` during `claudine init`, not at container start time.

### Home Setup (`setup-home.sh`)

Runs as root inside a one-shot container during `claudine init`. Sets up `/project/home` with configs, credentials, and Claude settings. Embedded into the binary via `include_str!` in `init.rs`.

**Sequence:**
1. Create `/project/home` and set ownership to `claude`
2. Ensure `/project` is writable by `claude` (for cloning repos)
3. Copy host gitconfig to `/project/home/.gitconfig`
4. Install SSH key to `/project/home/.ssh/id_key` with restrictive permissions and auto-generated SSH config
5. Copy `~/.claude/` credentials directory and `~/.claude.json` into the volume
6. Write container-specific Claude settings (`settings.json`) with ward hooks for PII/secrets scanning
7. Set `git config --global safe.directory '*'`

**SSH key isolation:** Only a single SSH key (selected during `claudine init`) is copied into the volume. The setup script writes a minimal `~/.ssh/config` that uses this key for all hosts with `IdentitiesOnly yes`. No other keys from the host `~/.ssh/` directory are accessible inside the container.

### Config (`~/.config/claudine/`)

```
~/.config/claudine/
├── config.toml                 # global defaults
└── projects/
    └── <project>/
        └── config.toml         # per-project settings
```

**Global config** (`config.toml`):
```toml
[image]
name = "claudine:latest"
```

**Project config** (`projects/<project>/config.toml`):
```toml
ssh_key = "/Users/you/.ssh/my_key"

[[repos]]
url = "git@github.com:user/frontend.git"
dir = "frontend"
branch = "main"

[[repos]]
url = "git@github.com:user/backend.git"
dir = "backend"

# [image]
# name = "claudine-node:latest"
```

**Rust structs:**
```rust
#[derive(Deserialize, Serialize)]
struct GlobalConfig {
    image: ImageConfig,
}

#[derive(Deserialize, Serialize)]
struct ProjectConfig {
    repos: Vec<RepoConfig>,
    ssh_key: Option<String>,
    image: Option<ImageConfig>,
}

#[derive(Deserialize, Serialize)]
struct ImageConfig {
    name: String,
}

#[derive(Deserialize, Serialize)]
struct RepoConfig {
    url: String,
    dir: String,
    branch: Option<String>,
}
```

## Data Flow

### Init Flow (`claudine init <project>`)

```
1. Validate project name (alphanumeric, hyphens, underscores)
2. Prompt for SSH key path (optional — leave empty for HTTPS repos)
3. Loop: prompt for repo URL, directory name, branch (repeat until empty URL)
4. docker volume create claudine_<project>
5. Create ~/claudine-share/<project>/ on the host
6. Serialize ProjectConfig to config dir
7. Run setup-home.sh in a one-shot container to set up /project/home:
   docker run --rm \
     -v claudine_<project>:/project \
     -v <setup-home.sh>:/tmp/setup-home.sh:ro \
     -v ~/.gitconfig:/tmp/host-gitconfig:ro \
     -v <ssh_key>:/tmp/host-ssh-key:ro \
     -v ~/.claude:/tmp/host-claude:ro \
     -v ~/.claude.json:/tmp/host-claude-json:ro \
     --entrypoint bash \
     claudine:latest \
     /tmp/setup-home.sh
8. For each repo, clone into the volume:
   docker run --rm \
     -v claudine_<project>:/project \
     -e HOME=/project/home \
     claudine:latest \
     git clone [--branch <branch>] <url> /project/<dir>
```

The clone runs as the `claude` user (entrypoint drops privileges via gosu before executing the command), so all files in `/project/<dir>` have correct ownership from the start. No post-hoc chown needed.

### Run Flow (`claudine run <project> [repo]`)

```
1. Verify volume exists (else error: "run init first")
2. If container already running → docker exec into it
3. Otherwise, start a new named container:
   docker run --rm
     --name claudine_<project>
     -v claudine_<project>:/project
     -v /var/run/docker.sock:/var/run/docker.sock
     -v ~/claudine-share/<project>/:/share   (if exists)
     -w /project/<repo>                      (or /project if no repo specified)
     -e HOME=/project/home
     --shm-size=256m
     + ANTHROPIC_API_KEY passthrough (if set)
     + -it flags only if stdin is a TTY
     claudine:latest
     claude
```

No host config bind mounts at run time. All credentials and configs were copied into the volume during init.

## Design Decisions

### Why Rust
- **clap** gives argument parsing, help text, and shell completions with zero effort
- **serde + toml** gives type-safe config with clear defaults — no hand-rolled parsers
- **Single binary** distribution — no runtime dependencies (bash version, jq, etc.)
- **`Command` builder** for Docker args eliminates shell quoting bugs
- **`Result<>`** error handling vs bash `set -e` — errors propagate with context
- Scales cleanly as commands are added without accumulating shell script debt

### Ephemeral Containers
Containers run with `--rm`. All persistent state lives in the Docker volume. This eliminates container lifecycle management — no `docker stop`/`start`, no orphans after crashes.

### Container Reuse
The `--name claudine_<project>` flag identifies the container for a project. When a container is already running:
- **`claudine run <project>`** — detects the running container and uses `docker exec` to attach. This allows multiple sessions (e.g., Claude in one terminal, shell in another) without conflicting.
- **`claudine shell <project>`** — same behavior, uses `docker exec` to attach a new bash session.

The first `run` or `shell` command starts the named container. Subsequent commands exec into the existing one. Both set the working directory and environment variables on the exec call to match the requested repo context.

### Docker-outside-of-Docker (DooD)
The host Docker socket is bind-mounted into the container, allowing Claude to run Docker commands that execute on the host daemon. The entrypoint detects the socket's GID and adds the `claude` user to the matching group at runtime.

This enables a key capability: **Claude inside a claudine container can manage other Docker containers**, including:
- Running project-specific services (`docker compose up`)
- Spinning up other claudine instances for parallel work
- Running test databases, build containers, or any Docker workload

Because the socket is the **host daemon's socket**, all containers launched from inside claudine are siblings (not nested). They share the host's Docker engine, network, and volume namespace.

### Init-Time Setup vs Runtime Copying
Host configs (gitconfig, SSH key, Claude credentials) are copied into the volume once during `claudine init` by `setup-home.sh`, not on every container start. This means:
- **No host bind mounts at run time** — the container only mounts the volume, Docker socket, and share directory
- **Writable copies** inside the container (tools can modify their own config)
- **Persistence** across container restarts (volume survives `--rm`)
- To update credentials after init, re-run `claudine init` (it prompts before overwriting)

### Authentication Forwarding
Three auth mechanisms are forwarded:
1. **Claude OAuth** (default): `~/.claude/` directory is copied into the volume during init
2. **API key**: `ANTHROPIC_API_KEY` environment variable is passed through to the container at run time
3. **SSH key**: A single SSH key (selected during `claudine init`) is copied into the volume and configured as the default identity. Only the required key is exposed — no other keys from the host `~/.ssh/` directory are accessible inside the container.

### Shared Directory Mount
Each project gets a host-side shared directory at `~/claudine-share/<project>/`, mounted into the container at `/share`. This provides a simple mechanism for transferring files between the host and container without going through git. The directory is created automatically during `claudine init`.

### TTY Detection
The CLI checks `std::io::stdin().is_terminal()` before adding `-it` flags to `docker run` and `docker exec`. Interactive terminal sessions get full TTY allocation; piped or scripted usage (e.g., `echo "fix the bug" | claudine run myproject`) runs without TTY flags so Docker doesn't error on missing terminal.

### Permission Skip
The `--dangerously-skip-permissions` flag is set via a bash alias. This is appropriate because the container is already an isolation boundary — interactive permission prompts would be redundant.

## Extending the Image

To add project-specific tools, create a custom Dockerfile:

```dockerfile
FROM claudine:latest

# Example: add Node.js
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs

# Example: add a custom CLI
COPY my-tool /usr/local/bin/my-tool
```

Then override `image.name` in the project config:
```toml
# ~/.config/claudine/projects/myproject/config.toml
[image]
name = "claudine-node:latest"
```

## File Map

| File | Purpose |
|------|---------|
| `Cargo.toml` | Rust dependencies and project metadata |
| `src/main.rs` | Clap app definition, command routing |
| `src/cli.rs` | Clap derive structs for all commands |
| `src/config.rs` | TOML config loading/saving/defaults/migration |
| `src/docker.rs` | Docker command assembly and execution |
| `src/init.rs` | Interactive project init flow (multi-repo, SSH key) |
| `src/project.rs` | Project name validation, volume/container helpers |
| `src/repo.rs` | Repo add/remove/list subcommands |
| `Dockerfile` | Generic container image definition |
| `entrypoint.sh` | Docker socket GID detection, privilege drop via gosu |
| `setup-home.sh` | Home directory setup script (configs, SSH, credentials, ward hooks) |
| `docs/architecture.md` | This document |
