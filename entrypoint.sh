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

# Drop privileges and execute the requested command (or bash if none given)
if [ $# -eq 0 ]; then
    exec gosu claude bash
else
    exec gosu claude "$@"
fi
