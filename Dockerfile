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
    unzip \
    openssh-client \
    python3 \
    less \
    netcat-openbsd \
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

# Install ward (PII/secrets scanner for Claude Code hooks)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . /root/.cargo/env \
    && git clone https://github.com/jstockdi/ward.git /tmp/ward \
    && cd /tmp/ward \
    && cargo build --release \
    && cp target/release/ward /usr/local/bin/ward \
    && chmod 755 /usr/local/bin/ward \
    && rm -rf /tmp/ward /root/.cargo /root/.rustup

# Create non-root user with home at /project/home (persistent volume)
RUN useradd -d /project/home -s /bin/zsh claude

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
