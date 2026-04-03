# Claudine

Run [Claude Code](https://docs.anthropic.com/en/docs/claude-code) inside isolated Docker containers with per-project persistent volumes, multi-repo support, and automatic host config forwarding.

## Quick Start

```bash
# Install
cargo install --path .

# Build the Docker image
claudine build

# Initialize a project
claudine init myproject

# Run Claude Code
claudine run myproject my-repo
```

## Features

- **Isolated environments** — each project runs in its own Docker container with a persistent volume
- **Multi-repo projects** — init multiple repos into one project, switch between them
- **SSH key isolation** — only the key you choose is available inside the container
- **Container reuse** — multiple terminals share one container via `docker exec`
- **Host file sharing** — `~/claudine-share/<project>/` is mounted at `/share` inside the container
- **Docker-outside-of-Docker** — Claude can run Docker commands that execute on the host daemon
- **Security hooks** — [ward](https://github.com/jstockdi/ward) PII/secrets scanning built into Claude Code hooks
- **Built-in plugins** — add Node.js, Heroku CLI, Rust, etc. to project images with one command

## Commands

```
claudine init <project>                  Create volume, clone repo(s)
claudine run <project> [repo] [-- ...]   Run Claude Code
claudine shell <project> [repo]          Open bash shell
claudine destroy <project>               Remove volume + config
claudine repo add <project> <url>        Add a repo to a project
claudine repo remove <project> <dir>     Remove a repo
claudine repo list <project>             List repos in a project
claudine plugin add <project> <name>     Add a plugin, rebuild project image
claudine plugin remove <project> <name>  Remove a plugin, rebuild project image
claudine plugin list <project>           List installed plugins
claudine plugin available                Show all available plugins
claudine build                           Build/rebuild the base Docker image
claudine list                            List all projects
claudine completions <shell>             Generate shell completions
```

## Plugins

Add project-specific tools without writing Dockerfiles:

```bash
claudine plugin add myproject node-20    # add Node.js 20
claudine plugin add myproject heroku     # add Heroku CLI (requires node)
```

Available plugins: `node-20`, `node-22`, `node-24`, `heroku`, `python-venv`, `rust`

## Container Layout

```
/project/              Volume mount (persistent)
├── home/              $HOME — configs, credentials, SSH key
├── <repo1>/           First repository
├── <repo2>/           Second repository
└── ...

/share/                Bind mount to ~/claudine-share/<project>/
```

## Documentation

- [Architecture](docs/architecture.md) — design, data flows, and decisions
- [Implementation Plan](docs/implementation.md) — step-by-step build plan

### Issues

- [001 — Build Notes](docs/issues/001-build-notes.md) — resolved issues from initial build
- [002 — Plugin Support](docs/issues/002-plugin-support.md) — original plugin proposal (implemented as built-in catalog)
- [003 — Config Dir Platform Difference](docs/issues/003-config-dir-platform-difference.md) — resolved macOS/Linux path difference
- [004 — Multi-Repo Projects](docs/issues/004-multi-repo-projects.md) — implemented multi-repo support
- [005 — Security Review](docs/issues/005-security-review.md) — security findings and fixes
- [006 — Plugin Remove Dependency Check](docs/issues/006-plugin-remove-dependency-check.md) — reverse dependency check on removal

## Security

Claudine mounts the host Docker socket into containers for Docker-outside-of-Docker (DooD) functionality. This gives Claude Code the ability to run Docker commands on the host daemon. Combined with `--dangerously-skip-permissions`, this is effectively root access to the host machine. This is by design for local development — the container is an isolation boundary for project separation, not a security sandbox.

See [Security Review](docs/issues/005-security-review.md) for full findings.

## Requirements

- Docker
- Rust (for building from source)

## License

[MIT](LICENSE)
