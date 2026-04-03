# Claudine Architecture

Claudine is a standalone CLI tool that runs Claude Code inside isolated Docker containers, with per-project persistent volumes and automatic host config forwarding.

## Overview

```
Host                                    Container (claudine:latest)
────────────────────────                ────────────────────────────────
~/.gitconfig ──────────────┐
~/.ssh/<key> ──────────────┤ bind-mount   /host-config/ (read-only)
~/.claude/ ────────────────┘    (ro)           │
                                          entrypoint.sh copies
                                          into /workspace/home/
                                               │
Docker volume                                  │         Symlinks
  claudine_<project> ──────── mounted ──► /workspace/    ─────────
  ├── home/                                ├── home/  ◄── /home ($HOME)
  │   ├── .claude/                         │   ├── .claude/
  │   ├── .ssh/id_key                      │   ├── .ssh/id_key
  │   └── .gitconfig                       │   └── .gitconfig
  └── project/                             └── project/ ◄── /project (workdir)
      ├── <repo1>/                             ├── <repo1>/
      └── <repo2>/                             └── <repo2>/

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
claudine run <project>                 →  run Claude Code (default action)
claudine shell <project>               →  open bash shell
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
```

### Docker Image (`Dockerfile`)

Generic, project-agnostic image based on Debian bookworm:

| Layer | Contents |
|-------|----------|
| Base | `debian:bookworm` |
| System | `ca-certificates curl gnupg gosu git python3 python3-pip vim` |
| Docker CLI | `docker-ce-cli docker-compose-plugin` (DooD pattern) |
| Claude Code | Native installer via `claude.ai/install.sh` |
| User | Non-root `claude` user |
| Alias | `claude="claude --dangerously-skip-permissions"` |

The image contains no project-specific tooling. Additional tools can be layered via custom Dockerfiles that extend `claudine:latest`.

### Entrypoint (`entrypoint.sh`)

Runs as root, performs runtime setup, then drops to the `claude` user via `gosu`.

**Sequence:**
1. Ensure `/workspace/home` and `/workspace/project` exist with correct ownership
2. Create root-level symlinks: `/home` → `/workspace/home`, `/project` → `/workspace/project`
3. Copy configs from `/host-config/` bind mounts into `/workspace/home/`
   - `gitconfig` → `.gitconfig`
   - `ssh_key` → `.ssh/id_key` (single key file, 600 perms, with auto-generated SSH config)
   - `claude-credentials/` → `.claude/` and `.claude.json`
4. Set `git config --global safe.directory '*'`
5. Detect Docker socket GID, add `claude` user to matching group
6. `exec gosu claude "$@"` (or `bash` if no args)

Configs are **copied** (not symlinked) because the bind mounts are read-only but tools like git and ssh may attempt writes to these paths.

**SSH key isolation:** Only a single SSH key (selected during `claudine init`) is mounted into the container. The entrypoint writes a minimal `~/.ssh/config` that uses this key for all hosts. This limits the container's access to only the key needed for the project's repositories.

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
5. Serialize ProjectConfig to config dir
6. For each repo, run container through the entrypoint then clone:
   docker run --rm \
     -v claudine_<project>:/workspace \
     -v <ssh_key>:/host-config/ssh_key:ro \
     -v ~/.gitconfig:/host-config/gitconfig:ro \
     claudine:latest \
     git clone <url> /workspace/project/<dir>
```

The clone runs as the `claude` user (entrypoint drops privileges via gosu before executing the command), so all files in `/workspace/project/<dir>` have correct ownership from the start. No post-hoc chown needed.

### Run Flow (`claudine run <project>`)

```
1. Verify volume exists (else error: "run init first")
2. If container already running, error (use "claudine shell <project>" for a second terminal)
3. Deserialize project config.toml
4. docker::build_run_args() assembles:
   --rm -it
   --name claudine_<project>
   -v claudine_<project>:/workspace
   -v /var/run/docker.sock:/var/run/docker.sock
   -v ~/.gitconfig:/host-config/gitconfig:ro        (if exists)
   -v <ssh_key>:/host-config/ssh_key:ro             (if configured)
   -v ~/.claude:/host-config/claude-credentials:ro  (if exists)
   -w /project
   -e HOME=/home
   --shm-size=256m
   + ANTHROPIC_API_KEY passthrough (if set)
5. Add -it flags only if stdin is a TTY (std::io::stdin().is_terminal())
6. Command::new("docker").args(...).exec() (replaces process)
```

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

### Docker-outside-of-Docker (DooD)
The host Docker socket is bind-mounted into the container, allowing Claude to run Docker commands that execute on the host daemon. The entrypoint detects the socket's GID and adds the `claude` user to the matching group at runtime.

This enables a key capability: **Claude inside a claudine container can manage other Docker containers**, including:
- Running project-specific services (`docker compose up`)
- Spinning up other claudine instances for parallel work
- Running test databases, build containers, or any Docker workload

Because the socket is the **host daemon's socket**, all containers launched from inside claudine are siblings (not nested). They share the host's Docker engine, network, and volume namespace.

### Config Copying vs Bind Mounting
Host configs are bind-mounted read-only to `/host-config/`, then **copied** into the volume's `home/` directory by the entrypoint. This gives us:
- **Fresh configs every run** (from the host bind mount)
- **Writable copies** inside the container (tools can modify their own config)
- **Persistence** across container restarts (volume survives `--rm`)

### Authentication Forwarding
Three auth mechanisms are forwarded:
1. **Claude OAuth** (default): `~/.claude/` directory is bind-mounted, credentials copied into the volume
2. **API key**: `ANTHROPIC_API_KEY` environment variable is passed through to the container
3. **SSH key**: A single SSH key (selected during `claudine init`) is mounted and configured as the default identity. Only the required key is exposed — no other keys from the host `~/.ssh/` directory are accessible inside the container.

### TTY Detection
The CLI checks `std::io::stdin().is_terminal()` before adding `-it` flags to `docker run`. Interactive terminal sessions get full TTY allocation; piped or scripted usage (e.g., `echo "fix the bug" | claudine run myproject`) runs without TTY flags so Docker doesn't error on missing terminal.

### Permission Skip
The `--dangerously-skip-permissions` flag is set via a bash alias. This is appropriate because the container is already an isolation boundary — interactive permission prompts would be redundant.

### Container Reuse
The `--name claudine_<project>` flag identifies the container for a project. Behavior when a container is already running:
- **`claudine shell <project>`** — detects the running container and uses `docker exec` to attach a new bash session. Supports the common workflow of Claude in one terminal and a shell in another.
- **`claudine run <project>`** — refuses with a clear error. Two Claude instances in the same workspace would conflict.

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
| `entrypoint.sh` | Runtime setup, symlinks, SSH key setup, privilege drop |
| `docs/architecture.md` | This document |
