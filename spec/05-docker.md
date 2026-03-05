# Docker Specification

## Overview

Three container types:
1. `hive-server` - Control plane
2. `hive-agent` - Agent executor (N instances)
3. `app-container` - Shared dev environment with app-daemon

## Design Principle: Project-Local Images

Dockerfiles live in `.hive/` inside the user's project. They are **not** pulled from a registry.
Instead:

1. `hive init` writes Dockerfiles to `.hive/` (embedded in the CLI binary)
2. `hive start` builds images from those Dockerfiles if not already built
3. Image names are scoped to the project to avoid conflicts across projects

Users can edit `.hive/Dockerfile.*` to add custom tools or dependencies.

### User Workflow

```
cd ~/my-project
hive init          # writes .hive/Dockerfile.*, .hive/config.toml
hive start         # builds images if needed, starts containers
hive stop          # stops containers (images + db intact)
hive start         # fast restart, images already built
```

### Image Naming

Images are named using the project ID stored in `.hive/config.toml`:

```
hive-server-{project-id}:latest
hive-agent-{project-id}:latest
app-container-{project-id}:latest
```

Where `project-id` is a short slug derived from the project directory name (e.g., `my-project`),
stored on first `hive init`.

## Deployment Diagram

```mermaid
flowchart TB
    subgraph DockerHost["Docker Host"]
        subgraph Network["Network: hive-net-{project-id}"]
            Server["hive-server<br/>(:8080)"]
            App["app-container<br/>(:8081, :3000)"]

            subgraph Agents["hive-agent containers"]
                A0["hive-agent-0<br/>(kilo)"]
                A1["hive-agent-1<br/>(claude)"]
            end
        end

        subgraph Volumes["Volume Mounts"]
            V1[".hive/ → /data<br/>(hive-server)"]
            V2[". → /app<br/>(app, agents)"]
            V3[".hive/ → /app/.hive:ro<br/>(hidden)"]
        end
    end

    HostCLI["hive-cli"] -->|"docker API"| Server
    HostCLI -->|"docker API"| App
    HostCLI -->|"docker API"| A0
    HostCLI -->|"docker API"| A1

    A0 <-->|"WS"| Server
    A1 <-->|"WS"| Server

    A0 <-->|"HTTP"| App
    A1 <-->|"HTTP"| App

    style Network fill:#eef
    style Server fill:#bbf,stroke:#333
    style App fill:#bfb,stroke:#333
    style Agents fill:#fbb,stroke:#333
```

## Networks

**Bridge network: `hive-net-{project-id}`**

Created by `hive start`, scoped per-project so multiple projects can run simultaneously.

## `.hive/` Directory Layout

```
.hive/
  config.toml          # Config (project id, agents, etc.)
  hive.db              # SQLite database (created by hive-server)
  Dockerfile.server    # Editable by user
  Dockerfile.agent     # Editable by user
  Dockerfile.app       # Editable by user
```

The Dockerfiles are written by `hive init` from templates embedded in the CLI binary.

## Container: hive-server

### Dockerfile (`.hive/Dockerfile.server`)

```dockerfile
FROM rust:1.85-slim AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release -p hive-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/hive-server /usr/local/bin/

RUN useradd -m -u 1000 hive
USER hive
WORKDIR /data

ENV RUST_LOG=info
ENV HIVE_SERVER_PORT=8080
ENV HIVE_DB_PATH=/data/hive.db

CMD ["hive-server"]
```

### Ports

| Port | Protocol | Description |
|------|----------|-------------|
| 8080 | WS/TCP   | Agent connections, API |

### Volumes

| Host Path | Container Path | Options |
|-----------|----------------|---------|
| `.hive/`  | `/data`        | rw      |

## Container: hive-agent

### Dockerfile (`.hive/Dockerfile.agent`)

```dockerfile
FROM rust:1.85-slim AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release -p hive-agent

FROM debian:bookworm-slim

# Install base tools
RUN apt-get update && apt-get install -y \
    curl \
    git \
    findutils \
    grep \
    sed \
    gawk \
    jq \
    fzf \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && npm install -g pnpm \
    && rm -rf /var/lib/apt/lists/*

# Install Rust (for agents that compile code)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Install Kilo
RUN npm install -g @kilocode/cli

# Install Claude Code
RUN curl -fsSL https://claude.ai/install.sh | sh

COPY --from=builder /build/target/release/hive-agent /usr/local/bin/

RUN useradd -m -u 1000 agent
USER agent
WORKDIR /app

CMD ["hive-agent"]
```

**Note**: Users can add tools to this Dockerfile (e.g., `python3`, `golang-go`) to match their
project's needs. Run `hive rebuild` after editing.

### Volumes

| Host Path | Container Path | Options    |
|-----------|----------------|------------|
| `.`       | `/app`         | rw         |
| `.hive/`  | `/app/.hive`   | ro (hidden)|

The `.hive/` mount is read-only so agents cannot tamper with hive config or the DB.

### Security

- **User**: Runs as non-root `agent` user
- **Filesystem**: Project directory (`/app`) is writable; `/app/.hive` is read-only
- **Network**: Can reach hive-server, app-container, localhost only (no internet by default)

## Container: app-container

### Dockerfile (`.hive/Dockerfile.app`)

```dockerfile
FROM rust:1.85-slim AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release -p app-daemon

FROM debian:bookworm-slim

# Install base tools
RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && npm install -g pnpm bun \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

COPY --from=builder /build/target/release/app-daemon /usr/local/bin/

EXPOSE 8081 3000
WORKDIR /app

ENV HIVE_APP_DAEMON_PORT=8081

CMD ["app-daemon"]
```

### Volumes

| Host Path | Container Path | Options    |
|-----------|----------------|------------|
| `.`       | `/app`         | rw         |
| `.hive/`  | `/app/.hive`   | ro (hidden)|

## app-daemon

A simple HTTP server running in `app-container` that wraps project commands.

### API

```
GET  /health
POST /exec            { command, pattern? }
POST /dev/start
POST /dev/stop
POST /dev/restart
POST /obs/test        { pattern? }
POST /obs/check
POST /obs/logs
```

### Exec Request/Response

```json
// Request
{ "command": "test", "pattern": "auth" }

// Success
{ "status": "ok", "output": "...", "exit_code": 0 }

// Error
{ "status": "error", "error": "command failed", "exit_code": 1 }
```

## Volume Mounts

| Container     | Host Path | Container Path | Options    |
|---------------|-----------|----------------|------------|
| hive-server   | `.hive/`  | `/data`        | rw         |
| app-container | `.`       | `/app`         | rw         |
| app-container | `.hive/`  | `/app/.hive`   | ro         |
| hive-agent    | `.`       | `/app`         | rw         |
| hive-agent    | `.hive/`  | `/app/.hive`   | ro         |

**Key insight**: Mounting `.hive/` as `ro` inside agent/app containers hides it from agents while
keeping it accessible to hive-cli on the host.

## `hive init` Command

`hive init` prepares a project for use with The Hive:

1. Creates `.hive/` directory
2. Generates a project ID (e.g., `my-project-a3f2`)
3. Writes Dockerfiles from embedded templates
4. Runs interactive config wizard if no `config.toml` exists
5. Writes `.hive/config.toml`
6. Appends `.hive/hive.db` to `.gitignore` (keeps Dockerfiles + config in git)

```
$ hive init
Initializing Hive in /home/dan/my-project/.hive/

? How many agents? (2)
? Agent 1: kilo or claude? kilo
? Agent 1 tags? backend
? Agent 2: kilo or claude? claude
? Agent 2 tags? frontend
? Start command? npm run dev
? Test command? npm test
? Check command? npm run check

Created .hive/config.toml
Created .hive/Dockerfile.server
Created .hive/Dockerfile.agent
Created .hive/Dockerfile.app

Run 'hive start' to build images and start the hive.
```

## `hive start` Flow

```mermaid
flowchart TD
    A[hive start] --> B{.hive/config.toml exists?}
    B -->|No| C[hive init]
    C --> D
    B -->|Yes| D[Read config]
    D --> E{Images built?}
    E -->|No| F[docker build from .hive/Dockerfiles]
    F --> G
    E -->|Yes| G[Ensure hive-net-{id} exists]
    G --> H[Create + start hive-server]
    H --> I[Wait healthy]
    I --> J[Create + start app-container]
    J --> K[Create + start hive-agent x N]
    K --> L[Connected!]
```

## `hive rebuild` Command

Rebuilds images when Dockerfiles change:

```bash
hive rebuild           # rebuild all images
hive rebuild agent     # rebuild just the agent image
```

## Development (hive contributors only)

The `docker/` directory contains Dockerfiles for building from the hive-repo source tree.
These are used by `just docker-build` and `just docker-up` when developing hive itself.

They are **not** used by end users.

```bash
just docker-build   # build dev images from source
just docker-up      # start with docker-compose (dev only)
```

---

## References

### Related Sections

- [Overview](./00-overview.md) - Problem statement
- [Architecture](./01-architecture.md) - System overview
- [hive-cli](./02-hive-cli.md) - Container management commands
- [Configuration](./06-configuration.md) - Config format

### See Also

- [Glossary](./07-glossary.md) - Term definitions
- [Index](./index.md) - File index
