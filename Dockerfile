FROM debian:bookworm

# Prevent interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Install system packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    gnupg \
    gosu \
    git \
    openssh-client \
    python3 \
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
       docker-compose-plugin \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI
RUN curl -fsSL https://claude.ai/install.sh | bash \
    && cp /root/.local/bin/claude /usr/local/bin/claude \
    && chmod 755 /usr/local/bin/claude

# Remove default /home so the entrypoint can symlink it to the persistent volume
RUN rm -rf /home

# Create non-root user with home at /home (symlinked to /workspace/home at runtime)
RUN useradd -d /home -s /bin/bash claude

# Add alias so claude always runs with --dangerously-skip-permissions
RUN echo 'alias claude="claude --dangerously-skip-permissions"' >> /etc/bash.bashrc

# Copy entrypoint script
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

WORKDIR /workspace

ENTRYPOINT ["/entrypoint.sh"]
