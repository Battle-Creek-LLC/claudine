FROM debian:bookworm

# Prevent interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Install system packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    file \
    gnupg \
    gosu \
    git \
    groff \
    jq \
    libdbus-1-dev \
    libssl-dev \
    unzip \
    openssh-client \
    pkg-config \
    python3 \
    less \
    netcat-openbsd \
    sudo \
    zsh \
    python3-pip \
    python3-venv \
    vim \
    && rm -rf /var/lib/apt/lists/*

# Install Docker CLI (docker-ce-cli and docker-compose-plugin)
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg \
       | gpg --dearmor -o /etc/apt/keyrings/docker.gpg \
    && chmod a+r /etc/apt/keyrings/docker.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
       https://download.docker.com/linux/debian bookworm stable" \
       > /etc/apt/sources.list.d/docker.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
       docker-ce-cli \
       docker-buildx-plugin \
       docker-compose-plugin \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI
RUN curl -fsSL https://claude.ai/install.sh | bash \
    && cp /root/.local/bin/claude /usr/local/bin/claude \
    && chmod 755 /usr/local/bin/claude

# Install Rust toolchain to /usr/local so it survives volume mounts
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --profile minimal

# Install cargo-binstall (downloads prebuilt binaries instead of compiling them).
# Used here for ward and just, and reused by stacked layers (exp, sumo).
RUN curl -L --proto '=https' --tlsv1.2 -sSf \
      https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
      | bash

# Install ward (PII/secrets scanner for Claude Code hooks) from prebuilt release
ARG WARD_VERSION=0.1.2
RUN cargo binstall -y --root /usr/local "bcl-ward@${WARD_VERSION}"

# Install `just` command runner via prebuilt binary so every project has it
RUN cargo binstall -y --root /usr/local just \
    && rm -rf /usr/local/cargo/registry /usr/local/cargo/git \
    && chmod -R a+rwX /usr/local/cargo

# Create non-root user. The home volume is mounted at /home/claude at runtime,
# shadowing the image's /home/claude so shell state persists across containers.
# Note: cargo perms are set in the just-install RUN above to avoid creating a
# duplicate copy-on-write layer of /usr/local/cargo. /usr/local/rustup is left
# at rustup's default perms (world-readable, root-writable).
RUN useradd -m -d /home/claude -s /bin/zsh claude \
    && echo 'claude ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/claude \
    && chmod 0440 /etc/sudoers.d/claude

# Add alias and ensure ~/.local/bin is on PATH for all shell types
RUN echo 'alias claude="claude --dangerously-skip-permissions"' >> /etc/bash.bashrc \
    && echo 'export PATH="$HOME/.local/bin:$PATH"' >> /etc/bash.bashrc \
    && mkdir -p /etc/zsh \
    && echo 'alias claude="claude --dangerously-skip-permissions"' >> /etc/zsh/zshrc \
    && echo 'export PATH="$HOME/.local/bin:$PATH"' >> /etc/zsh/zshrc

# Copy entrypoint script
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

WORKDIR /project

# The entrypoint intentionally starts as root: it detects the host Docker
# socket's GID and adds the 'claude' user to that group, and chowns the mounted
# /home/claude volume so shell state is writable. Once that root-only setup is
# done, entrypoint.sh drops privileges and runs the workload as the non-root
# 'claude' user via `exec gosu claude ...`. The explicit `USER root` below
# documents this intent; switching the final USER to 'claude' would break the
# docker-socket GID setup and the home chown, so it is left as root on purpose.
# The "missing non-root USER" scanner finding is a false positive and is
# dismissed accordingly (refs #5).
USER root
ENTRYPOINT ["/entrypoint.sh"]
