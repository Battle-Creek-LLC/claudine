#!/usr/bin/env bash
set -euo pipefail

# This script runs as root inside a one-shot container during `claudine init`.
# It sets up /home/claude (the HOME volume) with git config, SSH key, and Claude
# settings. Claude auth is handled by the user inside the container (not copied
# from host).

# Create and own home directory
mkdir -p /home/claude
chown claude:claude /home/claude

# gitconfig
if [ -f /tmp/host-gitconfig ]; then
    cp /tmp/host-gitconfig /home/claude/.gitconfig
    chown claude:claude /home/claude/.gitconfig
fi

# SSH key
if [ -f /tmp/host-ssh-key ]; then
    mkdir -p /home/claude/.ssh
    cp /tmp/host-ssh-key /home/claude/.ssh/id_key
    chmod 700 /home/claude/.ssh
    chmod 600 /home/claude/.ssh/id_key
    chown -R claude:claude /home/claude/.ssh
    printf 'Host *\n    IdentityFile /home/claude/.ssh/id_key\n    IdentitiesOnly yes\n    StrictHostKeyChecking accept-new\n' > /home/claude/.ssh/config
    chmod 600 /home/claude/.ssh/config
    chown claude:claude /home/claude/.ssh/config
fi

# Write container-specific Claude settings
mkdir -p /home/claude/.claude
cat > /home/claude/.claude/settings.json <<'SETTINGS'
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
chown -R claude:claude /home/claude/.claude

# Seed terra config into the user's home if the terra layer is installed
if [ -d /opt/terra-defaults ]; then
    mkdir -p /home/claude/.terra
    if [ -f /opt/terra-defaults/services.toml ] && [ ! -f /home/claude/.terra/services.toml ]; then
        cp /opt/terra-defaults/services.toml /home/claude/.terra/services.toml
    fi
    if [ -f /opt/terra-defaults/agents.yaml ] && [ ! -f /home/claude/.terra/agents.yaml ]; then
        cp /opt/terra-defaults/agents.yaml /home/claude/.terra/agents.yaml
    fi
    chown -R claude:claude /home/claude/.terra
fi

# Install claude CLI at ~/.local/bin (where Claude Code expects to find itself)
mkdir -p /home/claude/.local/bin
cp /usr/local/bin/claude /home/claude/.local/bin/claude
chmod 755 /home/claude/.local/bin/claude
chown -R claude:claude /home/claude/.local

# git safe directory
gosu claude git config --global --add safe.directory '*'
