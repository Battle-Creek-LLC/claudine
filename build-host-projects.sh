#!/usr/bin/env bash
#
# build-host-projects.sh — rebuild claudine project images from inside a
# sandbox/devcontainer, against the *host's* claudine config.
#
# Why this exists: when Claude runs inside a claudine container, its own
# ~/.config/claudine is empty — the real projects live in the host's config
# dir. The host's Docker daemon (OrbStack) is shared via the mounted socket,
# and `docker -v` source paths resolve on the *host*, so we can:
#
#   1. Bake the locally-built `claudine` + `gh` binaries into a one-off
#      `claudine-builder` image (FROM claudine:latest, which already has
#      git/ssh/docker-cli).
#   2. Run that image mounting the host's claudine config, the host's ~/.ssh
#      (layer source clones use the ambient SSH identity), and the docker
#      socket, then invoke `claudine build <project>`.
#
# The inner `docker build` context is streamed from inside the container, so it
# works regardless of which paths exist on the host.
#
# Usage:
#   GH_TOKEN="$(gh auth token)" ./build-host-projects.sh jstockdi plotzy advice-cloud
#   GH_TOKEN="$(gh auth token)" ./build-host-projects.sh --rebuild-builder plotzy
#
# GH_TOKEN is required (the brdg layer re-downloads its release wheel via `gh`
# on every build). It is passed through to the container with `docker run
# --env GH_TOKEN` — never written to disk or baked into an image.
#
# Override the host paths if your layout differs:
#   HOST_CLAUDINE_CONFIG   host claudine config dir (default: macOS location)
#   HOST_SSH               host ssh dir            (default: /Users/jstockdi/.ssh)
#   DOCKER_SOCK            host docker socket       (default: /run/docker.sock)
#   BUILDER_IMAGE          builder image tag        (default: claudine-builder)
set -euo pipefail

HOST_CLAUDINE_CONFIG="${HOST_CLAUDINE_CONFIG:-/Users/jstockdi/Library/Application Support/claudine}"
HOST_SSH="${HOST_SSH:-/Users/jstockdi/.ssh}"
DOCKER_SOCK="${DOCKER_SOCK:-/run/docker.sock}"
BUILDER_IMAGE="${BUILDER_IMAGE:-claudine-builder}"

rebuild_builder=0
projects=()
for arg in "$@"; do
  case "$arg" in
    --rebuild-builder) rebuild_builder=1 ;;
    -*) echo "Unknown flag: $arg" >&2; exit 2 ;;
    *) projects+=("$arg") ;;
  esac
done

if [ "${#projects[@]}" -eq 0 ]; then
  echo "Usage: GH_TOKEN=... $0 [--rebuild-builder] <project>..." >&2
  exit 2
fi

# GH_TOKEN must be in the environment; we pass it through, never embed it.
if [ -z "${GH_TOKEN:-}" ]; then
  echo "GH_TOKEN is not set. Run with: GH_TOKEN=... $0 ..." >&2
  exit 2
fi

command -v claudine >/dev/null || { echo "claudine not on PATH (cargo install --path . first)" >&2; exit 1; }
command -v gh >/dev/null        || { echo "gh not on PATH" >&2; exit 1; }
docker image inspect claudine:latest >/dev/null 2>&1 \
  || { echo "claudine:latest image missing — run 'claudine build' first" >&2; exit 1; }

# Build the builder image once (or when --rebuild-builder is passed).
if [ "$rebuild_builder" -eq 1 ] || ! docker image inspect "$BUILDER_IMAGE" >/dev/null 2>&1; then
  echo ">> Building $BUILDER_IMAGE image..."
  ctx="$(mktemp -d)"
  trap 'rm -rf "$ctx"' EXIT
  cp "$(command -v claudine)" "$ctx/claudine"
  cp "$(command -v gh)" "$ctx/gh"
  cat > "$ctx/builder-entrypoint.sh" <<'ENTRY'
#!/bin/sh
set -e
# Host ~/.ssh is mounted read-only at /ssh-host; copy it to /root/.ssh with the
# ownership/perms ssh requires (mounted host files are owned by a non-root uid,
# which ssh rejects for config/keys). Layer source clones use this identity.
if [ -d /ssh-host ]; then
  mkdir -p /root/.ssh
  cp -a /ssh-host/. /root/.ssh/ 2>/dev/null || true
  chown -R root:root /root/.ssh
  chmod 700 /root/.ssh
  chmod 600 /root/.ssh/* 2>/dev/null || true
fi
exec claudine "$@"
ENTRY
  cat > "$ctx/Dockerfile" <<'DOCKER'
FROM claudine:latest
COPY claudine /usr/local/bin/claudine
COPY gh /usr/local/bin/gh
COPY builder-entrypoint.sh /builder-entrypoint.sh
RUN chmod +x /usr/local/bin/claudine /usr/local/bin/gh /builder-entrypoint.sh
ENTRYPOINT ["/builder-entrypoint.sh"]
DOCKER
  docker build -t "$BUILDER_IMAGE" "$ctx"
  rm -rf "$ctx"; trap - EXIT
fi

rc=0
for p in "${projects[@]}"; do
  echo "===== BUILD $p START $(date -u +%H:%M:%S) ====="
  if docker run --rm \
      --env GH_TOKEN \
      -v "$HOST_CLAUDINE_CONFIG:/root/.config/claudine" \
      -v "$HOST_SSH:/ssh-host:ro" \
      -v "$DOCKER_SOCK:/var/run/docker.sock" \
      "$BUILDER_IMAGE" build "$p"; then
    echo "===== BUILD $p OK $(date -u +%H:%M:%S) ====="
  else
    code=$?
    echo "===== BUILD $p FAILED (exit $code) $(date -u +%H:%M:%S) =====" >&2
    rc=1
  fi
done
exit "$rc"
