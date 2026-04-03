#!/usr/bin/env bash
set -euo pipefail

# This script runs as root inside a one-shot container during `claudine init`.
# It sets up /project/home with git config, SSH key, and Claude settings.
# Claude auth is handled by the user inside the container (not copied from host).

# Create and own home directory
mkdir -p /project/home
chown claude:claude /project/home

# Ensure /project is writable by claude (for cloning repos)
chown claude:claude /project

# gitconfig
if [ -f /tmp/host-gitconfig ]; then
    cp /tmp/host-gitconfig /project/home/.gitconfig
    chown claude:claude /project/home/.gitconfig
fi

# SSH key
if [ -f /tmp/host-ssh-key ]; then
    mkdir -p /project/home/.ssh
    cp /tmp/host-ssh-key /project/home/.ssh/id_key
    chmod 700 /project/home/.ssh
    chmod 600 /project/home/.ssh/id_key
    chown -R claude:claude /project/home/.ssh
    printf 'Host *\n    IdentityFile /project/home/.ssh/id_key\n    IdentitiesOnly yes\n    StrictHostKeyChecking accept-new\n' > /project/home/.ssh/config
    chmod 600 /project/home/.ssh/config
    chown claude:claude /project/home/.ssh/config
fi

# Write container-specific Claude settings
mkdir -p /project/home/.claude
cat > /project/home/.claude/settings.json <<'SETTINGS'
{
  "permissions": {
    "allow": [
      "Bash(aws:*)",
      "Bash(docker:*)",
      "Bash(find:*)",
      "Bash(git:*)",
      "Bash(gh:*)",
      "Bash(ls:*)",
      "Bash(npm:*)",
      "Bash(tail:*)",
      "Bash(wc:*)",
      "WebSearch"
    ]
  },
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ward pii",
            "timeout": 5,
            "statusMessage": "Scanning for PII..."
          },
          {
            "type": "command",
            "command": "ward leaks",
            "timeout": 5,
            "statusMessage": "Scanning for secrets..."
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash|Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "ward pii",
            "timeout": 5,
            "statusMessage": "Scanning for PII..."
          },
          {
            "type": "command",
            "command": "ward leaks",
            "timeout": 5,
            "statusMessage": "Scanning for secrets..."
          }
        ]
      }
    ]
  }
}
SETTINGS
chown -R claude:claude /project/home/.claude

# Install claude CLI at ~/.local/bin (where Claude Code expects to find itself)
mkdir -p /project/home/.local/bin
cp /usr/local/bin/claude /project/home/.local/bin/claude
chmod 755 /project/home/.local/bin/claude
chown -R claude:claude /project/home/.local

# git safe directory
gosu claude git config --global --add safe.directory '*'
