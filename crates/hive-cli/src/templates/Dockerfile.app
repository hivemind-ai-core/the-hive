# app-container (runs app-daemon + your project)
# This file is managed by 'hive' but is safe to edit.
# Run 'hive rebuild app' after making changes.
# Common customizations: add language runtimes your project needs.
# The binary at .hive/bin/app-daemon was placed there by 'hive init'.

FROM ubuntu:24.04

RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Node.js + package managers
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && npm install -g pnpm bun \
    && rm -rf /var/lib/apt/lists/*

# Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Add other runtimes your project needs, e.g.:
# RUN apt-get update && apt-get install -y python3 python3-pip golang-go && rm -rf /var/lib/apt/lists/*

COPY .hive/bin/app-daemon /usr/local/bin/

EXPOSE 8081 3000
WORKDIR /app

ENV HIVE_APP_DAEMON_PORT=8081

CMD ["app-daemon"]
