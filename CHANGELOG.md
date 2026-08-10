# Changelog

All notable changes to claudine are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- `lin` layer pins the source checkout to `v0.8.0` (was the default branch).
  Picks up `cycle edit`, top-level `download`, threaded comment replies,
  project start/target dates, label edit/delete, issue relations, raw GraphQL
  commands, and `--due-date` on `issue create`/`issue edit`.
- Pin extraction reads the clone URL past any leading flags, so a
  `git clone --depth 1 --branch <tag> <url>` layer still reports its pin to
  `claudine doctor`.

## [0.12.0] - 2026-07-29

### Added
- Base image installs `mdpdf` 0.1.0 (Markdown → PDF) from its GitHub release
  binaries, sha256-verified, for amd64 and arm64. Not installed via
  `cargo binstall`: `bcl-mdpdf` is not on crates.io and the `mdpdf` crate
  there is an unrelated third-party project.

### Changed
- `brdg` layer pins the release wheel to `v0.4.0` (was `v0.2.4`). Picks up
  moot automatic meeting recording (v0.3.0), meeting video export via
  `recording --video`, and sending moot to an ad-hoc meeting URL (v0.4.0).
- `sntry` layer pins `bcl-sntry@0.3.0` (was `0.2.0`). Picks up
  `sntry monitors` — Cron monitor list/get/checkins/audit plus a
  `--yes`-gated create.

## [0.11.1] - 2026-07-23

### Changed
- `secops` layer pins `bcl-secunit@0.6.0` (was `0.5.0`), picking up the 0.6.0
  release: `secunit report data --week|--month|--quarter|--year` for
  weekly/monthly stakeholder reports (rendered and published as tracker
  issues by the bundled `report` skill), held-not-forgiven missed due dates
  (`secunit due --overdue` now fires), canonicalized `--period` spellings,
  and in-band `manifest_errors` / `register_errors` / `lapsed_exceptions`
  degradation across reports, `risks list`, and verify.

## [0.11.0] - 2026-07-22

### Changed
- Bump ward pin to `bcl-ward@0.2.0` (adds PostToolUse output redaction).
- `setup-home.sh`: register `ward pii` / `ward leaks` under `PostToolUse`
  (matcher `Bash|Read|WebFetch`) so tool output is redacted before Claude
  sees it. Existing home volumes need the same entries added to their
  `~/.claude/settings.json`; redaction is opt-in without them.

## [0.10.0] - 2026-06-28

### Changed
- **Base image migrated from Debian bookworm to Debian trixie** (glibc 2.36 →
  2.41). Required because terra HEAD now pulls in `fastembed`/`ort` (ONNX
  Runtime), whose prebuilt static library links against glibc ≥ 2.38 and a
  newer libstdc++; `sp` failed to link on bookworm (`undefined reference to
  __isoc23_strtol` / `__cxa_call_terminate`). All `bookworm`-pinned apt repos
  retargeted to `trixie`: the Docker CLI repo (`Dockerfile`) and the `adoptium`
  (java) and `hashicorp` (terraform) layers. Every base and project image must
  be rebuilt to pick up the new base.

### Added
- `gcloud` layer: installs the Google Cloud SDK (`gcloud`).

### Fixed
- `msodbc` layer: retargeted to the Debian 13 (`debian/13/prod trixie`)
  Microsoft repo and now imports **both** `microsoft.asc` and
  `microsoft-2025.asc`. Microsoft signs the Debian 13 repo with a new key
  (`EE4D7792F748182B`) absent from the legacy `microsoft.asc`, so apt rejected
  the repo as "not signed" with the old key alone.

## [0.9.3] - 2026-06-02

### Added
- `claudine cp <project> <src> [<dest>]`: copies a file from the host into a
  project container via `docker cp`. Destination defaults to `/tmp/`.

## [0.9.2] - 2026-05-29

### Changed
- `secops` layer pins `bcl-secunit@0.5.0` (was `0.4.2`), picking up the 0.5.0
  release: `secunit wisp` renders the WISP markdown set under `security/` into a
  single branded PDF entirely in Rust (markdown → Typst → PDF, no external
  toolchain), with cover page, table of contents, PDF bookmarks, and a
  git-commit + SHA-256 provenance stamp.

## [0.9.1] - 2026-05-28

### Changed
- `brdg` layer pins the release wheel to `v0.2.4` (was `v0.2.3`).
- `go` layer pins the Go toolchain to `1.26.3` (was `1.25.8`).

### Fixed
- `/dctr` skill: corrected the rebuild semantics. `claudine build <project>` is
  **non-disruptive** — its `remove_containers_for_image` filters by
  `ancestor=claudine:<project>`, but by the time it runs the tag already points
  at the new image, so a container running the *old* image no longer matches and
  survives the build. The skill no longer prompts before rebuilding running
  projects; instead it correctly attributes the disruption to the destroy step,
  which is what makes a running project pick up its new image (next `run`/`shell`
  recreates from it).

## [0.9.0] - 2026-05-27

### Added
- `claudine layer pins` command: lists every layer's pinned upstream version,
  classified by where "latest" is published — `crates.io` (the `cargo binstall`
  crates), `github-release` (`brdg`), `github-source` (the `git clone` / host
  checkout layers: `lin`, `glab`, `rodney`, `terra`/`guild`), and `go.dev`
  (`go`). Text table by default, `--json` for tooling. Layers that fetch latest
  at build (`flyway`, `doctl`) or track apt (`node-*`) carry no pin and are
  omitted.
- `/dctr` doctor skill (`.claude/commands/dctr.md`): checks the catalog pins
  against upstream latest (`/dctr upgrades`), dry-runs the maintenance plan
  (`/dctr check`), or runs the full sweep — apply behind pins, rebuild the
  projects that use the upgraded layers (prompting before rebuilding a running
  container, since a rebuild ends its session), prune stopped containers, and
  prompt for running ones. Built on `claudine layer pins --json`.

## [0.8.8] - 2026-05-27

### Changed
- `secops` layer pins `bcl-secunit@0.4.2` (was `0.1.2`), picking up everything
  released since 0.1.2: the `secunit doctor` environment/registry preflight, the
  internal risk register (`secunit risks`), the bundled skill standard library
  (`secunit skills`), and the macOS GUI. 0.4.2 is also the first secunit release
  to reach crates.io since 0.1.2 — the 0.2.x–0.4.1 tags were never published, so
  the layer could not move off 0.1.2 until now.

## [0.8.7] - 2026-05-26

### Changed
- `secops` layer pins `bcl-repocat@0.5.0` (was `0.4.0`), picking up the 0.5.0
  release: GitLab support (audit/diff/apply) alongside GitHub, and the
  `.repo.yml` → `.repo.github.yml` config rename. Layer description updated to
  note repocat now hardens GitHub and GitLab.

## [0.8.6] - 2026-05-26

### Changed
- `ddog` layer pins `bcl-ddog@0.4.0` (was `0.3.0`), picking up the 0.4.0 release.

## [0.8.5] - 2026-05-26

### Changed
- `brdg` layer pins the release wheel to `v0.2.3` (was `v0.2.1`). v0.2.3 defaults
  the CLI's `api_url` to `https://brdg.fly.dev`, so the layer's `brdg` talks to
  the hosted backend out of the box without per-container configuration.
  (v0.2.2 was CI-only and had no published wheel.)

## [0.8.4] - 2026-05-26

### Changed
- `brdg` layer pins the release wheel to `v0.2.1` (was `v0.2.0`). v0.2.1 fixes
  a `NoKeyringError` crash in `brdg auth login` on headless systems with no OS
  keychain backend (e.g. Debian Bookworm, which is the base image) — tokens now
  fall back to a `0600` `~/.brdg/config.yaml`. This matters because the layer
  runs the CLI inside exactly such a headless container.
- Added `build-host-projects.sh`: rebuilds claudine project images from inside
  a sandbox/devcontainer against the host's claudine config, via a one-off
  `claudine-builder` image (bakes in the local `claudine` + `gh` binaries) that
  mounts the host config, host `~/.ssh`, and the docker socket.

## [0.8.3] - 2026-05-26

### Changed
- `brdg` layer installs the release wheel into a dedicated virtualenv at
  `/opt/brdg` and symlinks `brdg` onto `PATH`, instead of
  `pip install --break-system-packages` into the base image's PEP 668
  externally-managed system interpreter. This isolates brdg's dependency tree
  (`typer`/`rich`/`httpx`/`keyring` → `cryptography`) from the system packages
  and from other layers, and removes the `--break-system-packages` override.
  Requires `python3-venv` in the base image (added below).

### Added
- Base image installs `python3-venv` (provides `ensurepip`), so layers can
  create virtualenvs. Used by the reworked `brdg` layer.

## [0.8.2] - 2026-05-25

### Changed
- `secops` layer installs `repocat` with `cargo binstall --disable-strategies
  compile`, removing the silent fall back to building from source. If a
  prebuilt `bcl-repocat` binary is ever unavailable for the target, the image
  build now fails fast instead of pulling in a from-source compile. Verified
  against `bcl-repocat@0.4.0`, which ships prebuilt binaries for both
  `x86_64-` and `aarch64-unknown-linux-gnu`.

## [0.8.1] - 2026-05-25

### Changed
- `secops` layer now pins `repocat` to `bcl-repocat@0.4.0` (was `0.1.3`),
  picking up the `changelog` command. `secunit` stays at `0.1.2`.

## [0.8.0] - 2026-05-25

### Added
- `brdg` layer: installs the Battle-Creek-LLC `brdg` Python CLI from its
  published GitHub release wheel (`v0.2.0`), installed with
  `pip install --break-system-packages` to satisfy the base image's
  PEP 668 externally-managed Python.
- Release-asset layer source: a layer may declare a `release` (repo, tag,
  asset glob) that claudine downloads host-side via `gh` into
  `sources/<layer>/` before each build, then stages into the build context.
  `gh` uses the host's authentication, so private release artifacts install
  without exposing a token to the Docker build — mirroring how `source_repo`
  uses the host's SSH key.

### Changed
- `source_ref` on a `source_repo` layer now resolves tags and commits, not
  just remote branches. The checkout refresh fetches tags and reset-resolves
  the ref via `origin/<ref>` (branch) or the ref verbatim (tag/commit),
  erroring on an unresolvable ref instead of silently falling back.

## [0.7.0] - 2026-05-23

### Added
- `secops` layer bundling two security-ops CLIs in one layer, both installed
  via `cargo binstall` from crates.io: `secunit` (WISP control registry
  helper, `bcl-secunit@0.1.2`) and `repocat` (GitHub repository hardening,
  `bcl-repocat@0.1.3`). Versions are pinned via `ARG <NAME>_VERSION`.

### Removed
- `secunit` layer — its tool is now part of the `secops` layer.
- `node-20` layer (Node.js 20.x LTS is deprecated). Use `node-22` or
  `node-24` instead. `heroku` now requires one of `node-22`/`node-24`.

## [0.6.0] - 2026-05-23

### Added
- Three new layers, all installed via `cargo binstall` from crates.io:
  `sntry` (Sentry read-side CLI, `bcl-sntry@0.2.0`), `ddog` (Datadog
  logs CLI, `bcl-ddog@0.3.0`, including the `metrics query` command), and
  `secunit` (WISP control registry helper CLI, `bcl-secunit@0.1.2`).
  Versions are pinned via `ARG <NAME>_VERSION` in each layer's
  Dockerfile snippet.

## [0.5.2] - 2026-05-06

### Changed
- Base image shrunk from ~4.0GB to ~1.9GB (-52%) via three Dockerfile
  tweaks: `rustup` now installs with `--profile minimal` (drops rust-docs);
  `just` is fetched via `cargo binstall` instead of compiled from source;
  and the `chmod -R a+rwX` previously applied in the `useradd` layer was
  removed — that layer was creating a copy-on-write duplicate of every
  file under `/usr/local/rustup` and `/usr/local/cargo` (~500MB). Cargo
  perms are still set in the prior `just`-install RUN. Rebuilt project
  images shrink proportionally (e.g. `plotzy` 9.8GB → 5.0GB).

## [0.5.1] - 2026-05-05

### Added
- `cargo-binstall` is now installed in the base image and used to fetch
  prebuilt binaries for `ward`, `exp`, and `sumo` instead of cloning each
  repo and running `cargo build --release`. This skips ~3 release builds
  per fresh image — the Rust toolchain is no longer on the hot path for
  these tools. Pinned versions: `bcl-ward@0.1.2` (base image),
  `exp@0.1.2`, `bcl-sumo@0.1.4` (stacked layers). Versions are
  `--build-arg`-overridable via `WARD_VERSION` / `EXP_VERSION` /
  `SUMO_VERSION`. Resolution depends on each crate's
  `[package.metadata.binstall]` block, so URL/binary naming is owned by
  the upstream repo, not claudine.

## [0.5.0] - 2026-05-05

### Added
- `sumo` layer: Sumo Logic log query CLI, built from
  [Battle-Creek-LLC/sumo](https://github.com/Battle-Creek-LLC/sumo).

### Changed
- Compiled-from-source layers (`lin`, `exp`, `sumo`, `glab`, `rodney`) now
  clean up their build caches at the end of each `RUN`. Rust layers remove
  `/usr/local/cargo/{registry,git}`; Go layers remove `/root/go` and
  `/root/.cache/go-build`. This shrinks per-project images by hundreds of
  megabytes per compiled tool.

## [0.4.1] - 2026-05-04

### Added
- `groff` in the base image so CLI tools that render man pages on demand
  (notably AWS CLI v2's `aws help`) work without errors.

## [0.4.0] - 2026-05-02

### Added
- `pnpm` is now pre-installed (via `corepack prepare pnpm@latest --activate`)
  in the `node-20`, `node-22`, and `node-24` layers, so the binary is baked
  into the image instead of downloaded on first use.
- `SECURITY.md` documenting the project's vulnerability reporting policy.
- GitHub Actions `dependency-review` workflow that flags risky dependency
  changes on pull requests.

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

[Unreleased]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.12.0...HEAD
[0.12.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.9.3...v0.10.0
[0.9.3]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.8...v0.9.0
[0.8.8]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.7...v0.8.8
[0.8.7]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.6...v0.8.7
[0.8.6]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.5...v0.8.6
[0.8.5]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.4...v0.8.5
[0.8.4]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.3...v0.8.4
[0.8.3]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.2...v0.8.3
[0.8.2]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.5.2...v0.6.0
[0.5.2]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Battle-Creek-LLC/claudine/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Battle-Creek-LLC/claudine/releases/tag/v0.1.0
