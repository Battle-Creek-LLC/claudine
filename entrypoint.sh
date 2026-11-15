#!/usr/bin/env bash
set -euo pipefail

# Docker socket GID detection: allow the claude user to access the host Docker daemon
if [ -S /var/run/docker.sock ]; then
    DOCKER_SOCK_GID=$(stat -c '%g' /var/run/docker.sock)
    if ! getent group "${DOCKER_SOCK_GID}" > /dev/null 2>&1; then
        groupadd -g "${DOCKER_SOCK_GID}" dockerhost
    fi
    DOCKER_GROUP_NAME=$(getent group "${DOCKER_SOCK_GID}" | cut -d: -f1)
    usermod -aG "${DOCKER_GROUP_NAME}" claude
fi

# Ensure /home/claude belongs to the current claude UID. The HOME volume was
# seeded by setup-home as root; after Zed's UID remap (or any passwd change)
# claude's UID may differ, so reassert ownership every startup.
if [ -d /home/claude ]; then
    chown -R claude:claude /home/claude 2>/dev/null || true
fi

# Seed a default .zshrc on first run so zsh-newuser-install doesn't prompt
if [ ! -f /home/claude/.zshrc ] && [ -f /etc/zsh/newuser.zshrc.recommended ]; then
    install -o claude -g claude -m 0644 \
        /etc/zsh/newuser.zshrc.recommended /home/claude/.zshrc
fi

# Ensure ~/.local/bin is on PATH for all shell types
export PATH="/home/claude/.local/bin:$PATH"

# Start the T3 Code server for projects with the t3 layer. It listens on
# 0.0.0.0:3773 inside the container; the project's `ports` config publishes it
# to the host. Pairing details are written to ~/.t3/serve.log.
if [ "${CLAUDINE_T3_SERVE:-}" = "1" ] && command -v t3 > /dev/null 2>&1; then
    install -d -o claude -g claude /home/claude/.t3
    gosu claude env HOME=/home/claude \
        nohup t3 serve --host 0.0.0.0 --port 3773 --no-browser "$PWD" \
        >> /home/claude/.t3/serve.log 2>&1 &
fi

# Drop privileges and execute the requested command (or bash if none given)
if [ $# -eq 0 ]; then
    exec gosu claude zsh
else
    exec gosu claude "$@"
fi
