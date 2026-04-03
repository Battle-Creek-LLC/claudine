#!/usr/bin/env bash
set -euo pipefail

# This script runs as root inside a one-shot container during `claudine init`.
# It sets up /project/home with configs, credentials, and Claude settings.

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

# Claude credentials directory
if [ -d /tmp/host-claude ]; then
    mkdir -p /project/home/.claude
    cp -a /tmp/host-claude/. /project/home/.claude/
    chown -R claude:claude /project/home/.claude
fi

# Claude config file
if [ -f /tmp/host-claude-json ]; then
    cp /tmp/host-claude-json /project/home/.claude.json
    chown claude:claude /project/home/.claude.json
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
chown claude:claude /project/home/.claude/settings.json

# git safe directory
gosu claude git config --global --add safe.directory '*'
