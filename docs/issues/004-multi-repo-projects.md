# Multi-Repo Project Support

**Status:** Implemented  
**Type:** Feature  

## Summary

A single claudine project can contain multiple git repositories, cloned as sibling directories under `/project/`. The container layout places repos and the home directory directly under `/project/` (the volume mount point), with no symlinks.

## Container Layout

```
Docker volume claudine_<project> → /project/
├── home/         ← $HOME (/project/home)
├── frontend/     ← repo 1
├── backend/      ← repo 2
└── infra/        ← repo 3
```

The Docker volume mounts directly at `/project`. The `claude` user's home directory is set to `/project/home` via `useradd -d`. Single-repo projects just have one entry alongside `home/`.

## UX

### Init flow

```
$ claudine init mystack

SSH key path (leave empty for HTTPS repos): ~/.ssh/id_ed25519

Repository URL (leave empty to finish): git@github.com:acme/frontend.git
Directory name [frontend]:
Branch [default]:

Repository URL (leave empty to finish): git@github.com:acme/backend.git
Directory name [backend]:
Branch [default]: develop

Repository URL (leave empty to finish):

Creating volume 'claudine_mystack'...
Created share directory: ~/share/mystack
Setting up home directory...
Cloning frontend...
Cloning backend...
Project 'mystack' initialized successfully.
```

- First repo is required (can't init with zero repos)
- Directory name defaults to the repo name (derived from URL — strip `.git`, take last path segment)
- User can override the directory name to avoid collisions or for clarity
- Branch is per-repo

### Adding repos later

```
$ claudine repo add mystack git@github.com:acme/infra.git
Cloning infra...
Repository 'infra' added to project 'mystack'.

$ claudine repo remove mystack infra
Remove directory 'infra' from volume? (y/N): y
Repository 'infra' removed from project 'mystack'.

$ claudine repo list mystack
NAME        REPO                                  BRANCH
frontend    git@github.com:acme/frontend.git      main
backend     git@github.com:acme/backend.git       develop
```

The `repo` subcommand provides `add`, `remove`, and `list` sub-subcommands. The `add` command accepts optional `--dir` and `--branch` flags.

## Config Format

```toml
ssh_key = "/Users/you/.ssh/id_ed25519"

[[repos]]
url = "git@github.com:acme/frontend.git"
dir = "frontend"
branch = "main"

[[repos]]
url = "git@github.com:acme/backend.git"
dir = "backend"
branch = "develop"
```

TOML array of tables (`[[repos]]`) maps cleanly to `Vec<RepoConfig>` in Rust.

### Rust structs

```rust
#[derive(Deserialize, Serialize)]
struct ProjectConfig {
    repos: Vec<RepoConfig>,
    ssh_key: Option<String>,
    image: Option<ImageConfig>,
}

#[derive(Deserialize, Serialize)]
struct RepoConfig {
    url: String,
    dir: String,
    branch: Option<String>,
}
```

## Implementation

### Files changed

| File | Change |
|------|--------|
| `src/cli.rs` | Added `Repo` subcommand with `Add`, `Remove`, `List` children |
| `src/config.rs` | Replaced `ProjectInfo` with `Vec<RepoConfig>`, added `ssh_key` field, migration for old format |
| `src/init.rs` | Loop prompt for SSH key + multiple repos, clone each to `/project/<dir>` |
| `src/docker.rs` | Volume mounted at `/project`, added `/share` mount, container reuse via exec |
| `src/repo.rs` | New module for repo add/remove/list subcommands |
| `src/main.rs` | Route `Repo` subcommand |
| `Dockerfile` | User home set to `/project/home`, `WORKDIR /project` |
| `entrypoint.sh` | Simplified to Docker socket GID + gosu only |
| `setup-home.sh` | New script for init-time home directory setup |

### Clone logic

Each repo clones to `/project/<dir>/`:

```
git clone <url> /project/<dir>
```

The Docker working directory (`-w /project` or `-w /project/<repo>`) is set at run time. Claude sees all repos as subdirectories and can navigate between them.

### Volume mount strategy

Single volume mounted directly at `/project`. No symlinks.

```
Volume claudine_<project> → /project
  /project/home/       (persistent $HOME, set via useradd -d)
  /project/<repo1>/    (cloned repositories)
  /project/<repo2>/
```

Docker args at run time:
```
-v claudine_<project>:/project
-e HOME=/project/home
-w /project
```

### Backwards compatibility

Existing single-repo configs using the old `[project]` format are auto-migrated on load. The migration function in `config.rs` detects the legacy format, converts it to `[[repos]]`, and saves the updated config. This is a one-way migration.

### Repo name derivation

Extract directory name from URL:
- `git@github.com:acme/frontend.git` → `frontend`
- `https://github.com/acme/backend.git` → `backend`
- `https://github.com/acme/my.dotted.repo.git` → `my.dotted.repo`

Strip `.git` suffix, take the last path segment.
