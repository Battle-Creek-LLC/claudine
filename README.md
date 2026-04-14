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
- **Host file sharing** — `~/share/<project>/` is mounted at `/share` inside the container
- **Docker-outside-of-Docker** — Claude can run Docker commands that execute on the host daemon
- **Security hooks** — [ward](https://github.com/jstockdi/ward) PII/secrets scanning built into Claude Code hooks
- **Built-in layers** — add Node.js, Heroku CLI, Rust, etc. to project images with one command
- **Post-build validation** — every layer includes smoke tests that run automatically after image builds

## Commands

```
claudine init <project>                  Create volume, clone repo(s)
claudine run <project> [repo] [-- ...]   Run Claude Code
claudine shell <project> [repo]          Open bash shell
claudine destroy <project>               Remove volume + config
claudine repo add <project> <url>        Add a repo to a project
claudine repo remove <project> <dir>     Remove a repo
claudine repo list <project>             List repos in a project
claudine layer add <project> <name>      Add a layer, rebuild + validate
claudine layer remove <project> <name>   Remove a layer, rebuild + validate
claudine layer list <project>            List installed layers
claudine layer available                 Show all available layers
claudine layer validate <layer>          Standalone build + validate a layer
claudine build                           Build/rebuild the base Docker image
claudine list                            List all projects
claudine completions <shell>             Generate shell completions
```

## Layers

Add project-specific tools without writing Dockerfiles:

```bash
claudine layer add myproject node-20    # add Node.js 20
claudine layer add myproject heroku     # add Heroku CLI (requires node)
```

Every `layer add`, `layer remove`, and `build <project>` automatically runs validation checks against the final image to catch installation failures and conflicts.

Available layers: `node-20`, `node-22`, `node-24`, `gh`, `heroku`, `python-venv`, `rust`, `go`, `java`, `flyway`, `aws`, `terraform`, `doctl`, `lin`, `exp`, `terra`, `glab`, `rodney`

### Standalone Validation

Validate a single layer in isolation (builds a temporary image):

```bash
claudine layer validate rust             # build + validate one layer
claudine layer validate                  # build + validate every layer
```

### Creating Layers

Layers are defined in `src/layer.rs` as entries in the `catalog()` function. Each `Layer` has:

| Field         | Type                     | Description                                                    |
|---------------|--------------------------|----------------------------------------------------------------|
| `name`        | `&str`                   | Unique identifier (lowercase, hyphenated)                      |
| `description` | `&str`                   | One-line summary shown in `layer available`                    |
| `requires`    | `&[&str]`                | Layers that satisfy a dependency (at least one must be present) |
| `build_tool`  | `Option<BuildTool>`      | `Rust` or `Go` — toolchain is installed for build, then removed |
| `dockerfile`  | `String`                 | Dockerfile snippet (`RUN`, `ENV`, etc.)                        |
| `validate`    | `&[&str]`                | Shell commands that must exit 0 when the layer works correctly  |

Example:

```rust
Layer {
    name: "gh",
    description: "GitHub CLI",
    requires: &[],
    build_tool: None,
    dockerfile: "RUN curl -fsSL https://cli.github.com/packages/... \
        && apt-get install -y --no-install-recommends gh \
        && rm -rf /var/lib/apt/lists/*".to_string(),
    validate: &["gh --version"],
},
```

#### Best Practices

1. **Always include validation commands.** Every layer must have at least one `validate` entry. The test suite enforces this (`all_layers_have_validate_commands`).

2. **Validate the binary, not the install.** Use `<tool> --version` or `<tool> --help` rather than checking file paths. This catches PATH issues, missing shared libraries, and broken installs.

3. **Use `ENV` for PATH and runtime variables.** Do not rely on `/etc/bash.bashrc` or `/etc/profile.d/` — these are only sourced in interactive/login shells. Containers run commands via `gosu` which is neither.

   ```dockerfile
   # Good — works in all shells
   ENV PATH="/usr/local/cargo/bin:${PATH}"

   # Bad — only works in interactive bash
   RUN echo 'export PATH=...' >> /etc/bash.bashrc
   ```

4. **Clean up after install.** Remove package lists (`rm -rf /var/lib/apt/lists/*`) and build artifacts to keep images small.

5. **Declare dependencies.** If a layer needs another (e.g., heroku needs node), add it to `requires`. The first listed dependency is used automatically during standalone validation.

6. **Use `build_tool` for compile-from-source layers.** Set `build_tool: Some(BuildTool::Rust)` or `BuildTool::Go` — the Dockerfile generator installs the toolchain before your `RUN` and removes it after, so the final image stays lean.

7. **Test standalone and in combination.** Run `claudine layer validate <layer>` for isolation, then `claudine build <project>` to catch conflicts between layers (port collisions, PATH shadowing, library conflicts).

8. **Add an integration test.** Add a `#[test] #[ignore]` entry in `tests/layer_validate.rs` so the layer is covered by `cargo test --test layer_validate -- --ignored`.

## Container Layout

```
/project/              Volume mount (persistent)
├── home/              $HOME — configs, credentials, SSH key
├── <repo1>/           First repository
├── <repo2>/           Second repository
└── ...

/share/                Bind mount to ~/share/<project>/
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
