# claudine doctor (`/dctr`)

Maintain claudine: find layer pins that have fallen behind upstream, apply the
upgrades, rebuild the projects that use them, and tidy up containers. Run this
from the claudine repo (it reads and edits `src/layer.rs`).

`$ARGUMENTS` selects the mode:

| `$ARGUMENTS` | Mode | Mutates? |
|---|---|---|
| `upgrades` | **Report** which pins are behind upstream + pending release state. | No |
| `check` | **Dry run** of the sweep: behind pins, projects that would rebuild, container inventory. | No |
| *(empty)* | **Sweep**: apply upgrades → rebuild affected projects → clean up containers → offer release. | Yes (gated) |

Pick the mode from `$ARGUMENTS`. If it's anything else, show this table and stop.

---

## The pin inventory (source of truth)

Always start from the catalog, never by hand-parsing the Dockerfile:

```bash
claudine layer pins --json    # stdout is clean JSON; the Bruce Lee quote is on stderr
```

Each entry is `{ layer, tool, kind, version, source }`. `kind` tells you where
"latest" lives and how to check it:

### Finding the latest upstream version per `kind`

- **`crates.io`** — `source` is the crate name. Query the sparse index (no auth,
  authoritative). Path rule (lowercase the name):
  - 1 char → `1/{name}` · 2 → `2/{name}` · 3 → `3/{name[0]}/{name}` · 4+ → `{name[0:2]}/{name[2:4]}/{name}`
  ```bash
  # e.g. bcl-secunit → bc/l-/bcl-secunit ; exp → 3/e/exp
  curl -s https://index.crates.io/bc/l-/bcl-secunit \
    | python3 -c "import sys,json; vs=[json.loads(l) for l in sys.stdin if l.strip()]; print([v['vers'] for v in vs if not v.get('yanked')][-1])"
  ```
  Latest = the last non-yanked `vers`. Compare to the pin with semver (behind if pin < latest).

- **`github-release`** — `source` is `owner/repo`; pin is a tag like `v0.2.3`.
  ```bash
  gh api repos/<owner/repo>/releases/latest -q .tag_name
  ```
  Behind if the latest tag is newer than the pinned tag.

- **`go.dev`** — pin is a version like `1.25.8`.
  ```bash
  curl -s 'https://go.dev/dl/?mode=json' | python3 -c "import sys,json;print(json.load(sys.stdin)[0]['version'])"
  # -> go1.25.8 ; strip the leading 'go' before comparing
  ```

- **`github-source`** — these layers clone a repo's **default branch** at build
  time; `version` is `<default-branch>`, so there is **no pin to be behind**.
  Treat them as *informational*: report the latest tag and the current HEAD so the
  user can decide, but the only way to pick up changes is a rebuild.
  ```bash
  gh api repos/<owner/repo>/tags -q '.[0].name'      # latest tag (may be empty)
  gh api repos/<owner/repo>/commits/HEAD -q .sha     # current default-branch HEAD
  ```

---

## Mode: `upgrades` (read-only report)

1. Run `claudine layer pins --json`.
2. For each pin, fetch the latest upstream per its `kind` (above) and decide `behind?`.
3. Print a table: `layer | tool | kind | pinned | latest | behind?`.
   - `github-source` rows: show latest tag / short HEAD instead of behind, marked *(tracks branch — rebuild to update)*.
4. **Release state** — also surface upgrades already applied but not shipped:
   - `git -C <claudine repo> diff --stat src/layer.rs` (uncommitted pin edits)
   - whether `## [Unreleased]` in `CHANGELOG.md` has content
   If either is non-empty, note: "pin changes pending commit/release."
5. Summarise: "N crates.io/release pins behind; M source layers tracking branches." Make **no changes**.

---

## Mode: `check` (dry run of the sweep)

Do everything `upgrades` does, then additionally compute and print, **without changing anything**:

- **Affected projects** — for each behind pin, which projects would rebuild:
  ```bash
  claudine list                              # project names + STATUS
  claudine layer list <project>              # does it include the behind layer?
  ```
  A project is affected if its layers include a layer with a behind pin.
- **Container inventory** — for every project, whether its container is running or stopped:
  ```bash
  docker ps    --filter "name=^claudine_<project>$" --format '{{.Names}}'   # running
  docker ps -a --filter "name=^claudine_<project>$" --format '{{.Names}}'   # exists
  ```
- Print the plan the real sweep *would* execute (bump list, rebuild list with a ⚠ on running ones, stopped containers to remove, running ones it would ask about). Stop.

---

## Mode: *(empty)* — the sweep

Run the steps in order. Confirm before the first mutating action.

### 1. Detect
Run the `check` computation: behind pins (crates.io / github-release / go.dev) and the affected projects. Present it and **ask the user to confirm** before applying anything. If nothing is behind, say so and skip to step 4 (container cleanup may still be wanted — ask).

### 2. Apply the pin bumps
For each confirmed behind pin, edit `src/layer.rs`:
- **crates.io** → change the `ARG <VAR>=<old>` value (e.g. `ARG SECUNIT_VERSION=0.4.2`).
- **github-release** → change the `ReleaseAsset { ... tag: "<old>" }`.
- **go.dev** → change the `GO_VERSION` const.
Then make the new pins live so `claudine build` uses them:
```bash
cargo install --path .
```
Re-run `claudine layer pins` to confirm the new values.

### 3. Rebuild affected projects
Rebuild every affected project. **Safety gate:** `claudine build <project>` removes
that project's container on success — so rebuilding a **running** project ends its
live session.
- Projects whose container is **stopped or absent** → rebuild directly: `claudine build <project>`.
- Projects whose container is **running** → list them and **ask** before rebuilding ("this ends the running session for `<project>` — rebuild now?"). Skip the ones the user declines.
Report each rebuild's result (the build prints layer validation).

### 4. Destroy containers that are NOT running
For every project whose container **exists but is stopped**, remove just the
container (keep the volume + config, so it's recreated on next `run`/`shell`):
```bash
claudine destroy <project> -y      # NO --purge
```
List what you removed.

### 5. Prompt for RUNNING containers
List the still-running containers and ask, per container (or as a batch), whether
to destroy them. Default to **keeping** them. Only `claudine destroy <project> -y`
(never `--purge`) for the ones the user approves.

### 6. Offer to release claudine
The bumps in step 2 are "upgrades to be committed." If `src/layer.rs` changed,
offer to cut a claudine release per `CLAUDE.md`:
- add a `## [x.y.z] - YYYY-MM-DD` entry to `CHANGELOG.md` (patch bump for pin bumps),
- bump `version` in `Cargo.toml`, run `cargo build --release`,
- commit `Release v<version>: ...`, tag `v<version>`, update the comparison links.
Confirm before committing/tagging/pushing.

---

## Notes
- Read JSON from **stdout**; claudine prints a quote on **stderr** — don't let it corrupt parsing.
- `destroy` without `--purge` never touches home volumes or config — it's safe data-wise; only the container layer is reclaimed.
- `github-source` layers (`lin`, `glab`, `rodney`, `terra`/`guild`) and the
  always-latest layers (`flyway`, `doctl`, node) have no pin to bump; they only
  refresh on rebuild. Mention them, don't try to "upgrade" them.
