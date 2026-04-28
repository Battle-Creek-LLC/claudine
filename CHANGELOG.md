# Changelog

All notable changes to claudine are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-04-28

### Added
- `libdbus-1-dev` in the base image so projects linking dbus-rs / zbus build
  out of the box.

## [0.2.1] - 2026-04-27

### Fixed
- Home volume now mounts at `/home/claude` (the passwd entry) instead of
  `/project/home`. OpenSSH resolves `~/.ssh` via `getpwuid()`, not `$HOME`,
  so mounting at the passwd home ensures SSH keys, known_hosts, and `$HOME`
  all point to the same location without any env-var override.

## [0.2.0] - 2026-04-25

### Changed
- Bind-mount + home-volume is now the only supported project layout. `host_dir`
  defaults to `~/projects/<name>/` when not set in config, eliminating the need
  to ever explicitly configure it for new projects.
- Container working directory and volume mounts now use the host path verbatim
  (e.g. `/Users/you/projects/myproject`) rather than the fixed `/project` alias.

### Removed
- `migrate` command — all projects now use the bind-mount layout; the migration
  path is no longer needed.
- Legacy single-volume layout support (`claudine_<project>` Docker volume, `~/share/<project>/`
  fallback, and all associated code paths).

## [0.1.2] - 2026-04-21

### Fixed
- Corrected v0.1.1 tag (was force-updated after initial release; this is the
  clean re-release of the same fix with proper version history)

## [0.1.1] - 2026-04-21

### Fixed
- `repo add` SSH host key verification failure — OpenSSH resolves `~/.ssh` via
  `getpwuid()` (the passwd home `/home/claude`), not `$HOME`, so the key and
  config in `/project/home/.ssh/` were never found. Clone containers now set
  `GIT_SSH_COMMAND` with explicit `-i`, `UserKnownHostsFile`, and
  `StrictHostKeyChecking=accept-new` pointing at `/project/home/.ssh/`.

## [0.1.0] - 2026-04-20

### Added
- Core `init`, `run`, `shell`, `destroy`, `purge`, `build` commands
- `repo add / remove / list` subcommands for managing repositories in a project
- `layer` system with catalog: node-20, node-24, rust, go, python-venv, postgres,
  msodbc, flyway, heroku, gh, glab, lin, exp, terra, rodney
- Per-layer post-build smoke-test validation (`claudine build --validate`)
- `zed` command for Zed dev container integration with per-repo workspace targeting
- Agent-assisted `init` via `claudine init --agent <path>` (Claude analyzes a
  local folder and proposes repos + layers)
- SSH key detection from `~/.ssh/config` with host alias resolution
- Bind-mount + home-volume project layout (host dir + named volume for `$HOME`)
- `migrate` command to move legacy single-volume projects to the new layout
- Shell completion generation (`claudine completions`)
- Passthrough arguments in `claudine shell`
- `terra` layer built from host-side source checkout with guild CLI and default config seeding
- `just` command runner pre-installed in the base image
- Persistent containers across sessions; `destroy` vs `purge` distinction

[Unreleased]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Battle-Creek-LLC/claudine/releases/tag/v0.1.0
