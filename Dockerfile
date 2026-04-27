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
    jq \
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

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path

# Install ward (PII/secrets scanner for Claude Code hooks)
RUN git clone https://github.com/jstockdi/ward.git /tmp/ward \
    && cd /tmp/ward \
    && cargo build --release \
    && cp target/release/ward /usr/local/bin/ward \
    && chmod 755 /usr/local/bin/ward \
    && rm -rf /tmp/ward

# Install `just` command runner into the base so every project has it
RUN cargo install just --root /usr/local \
    && chmod -R a+rwX /usr/local/cargo

# Create non-root user. The home volume is mounted at /home/claude at runtime,
# shadowing the image's /home/claude so shell state persists across containers.
RUN useradd -m -d /home/claude -s /bin/zsh claude \
    && chmod -R a+rwX /usr/local/rustup /usr/local/cargo \
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

ENTRYPOINT ["/entrypoint.sh"]
