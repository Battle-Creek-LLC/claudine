# Issues and Notes

## Resolved During Implementation

### Claude Code installer path changed
- **Issue**: The architecture doc assumed the Claude Code CLI would install to `~/.claude/local/bin/claude`, but the current installer (`claude.ai/install.sh`) places the binary at `~/.local/bin/claude`.
- **Resolution**: Updated the Dockerfile to copy from `/root/.local/bin/claude`. This path may change in future installer versions; if the build breaks, check the installer output for the actual binary location.

### gosu resets HOME to passwd entry
- **Issue**: Setting `export HOME=/project/home` in the entrypoint was ineffective because `gosu` resets `HOME` based on the user's `/etc/passwd` home directory.
- **Resolution**: Changed `useradd` to set the claude user's home directory to `/project/home` directly (`useradd -d /project/home`), so gosu picks up the correct value. The `-m` flag is omitted since `/project/home` is created at runtime by `setup-home.sh`.
