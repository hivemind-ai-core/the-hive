# hive-cli Specification

## Overview

`hive-cli` is the user-facing CLI and TUI application. It manages Docker containers and provides an interface for interacting with the swarm.

## Binary Name

- **Installed binary**: `hive`
- **Crate/directory**: `hive-cli`

## CLI Interface

```
hive [--version] [--help] <command> [options]
```

### Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `start` | `up` | Start all containers (hive-server, app-container, hive-agent[N]) |
| `stop` | `down` | Stop all containers |
| `restart` | | Restart all containers |
| `ui` | `tui` | Start the TUI (connects to hive-server) |
| `connect` | `attach` | Alias for `ui` |
| `status` | | Show container status |
| `config` | `cfg` | Edit/open config |
| `logs` | | Show logs (all, or specific container) |
| `agent` | | Manage agents (list, spawn, kill) |
| `task` | | Task commands (list, create, show) |
| `topic` | | Message board commands |
| `completion` | | Generate shell completions |

### Global Flags

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Enable verbose logging |
| `-C, --directory` | Project directory (default: current) |
| `--config` | Config file path |

## Project Layout

```
the-hive/
├── hive-cli/           # Rust crate
│   ├── src/
│   │   ├── main.rs     # Entry point, CLI args
│   │   ├── lib.rs     # Shared types
│   │   ├── commands/  # CLI command implementations
│   │   ├── tui/       # TUI application (ratatui)
│   │   ├── docker/    # Docker management (bollard)
│   │   └── config/   # Config reading/writing
│   ├── Cargo.toml
│   └── ...
└── spec/
    └── 02-hive-cli.md
```

## Initialization Flow

```mermaid
flowchart TD
    A[hive start] --> B{Config exists?}
    
    B -->|No| C[Prompt user for config]
    C --> D[How many agents?]
    D --> E[Agent 1: kilo/claude?]
    E --> F[Agent 1 tags?]
    F --> G[Agent 2: kilo/claude?]
    G --> H[Agent 2 tags?]
    H --> I[Start command?]
    I --> J[Test command?]
    J --> K[Check command?]
    K --> L[Write config.toml]
    
    B -->|Yes| M[Read config.toml]
    L --> M
    
    M --> N[Create .hive directory]
    N --> O[Start hive-server]
    O --> P[Start app-container]
    P --> Q[Start hive-agent N]
    Q --> R[Wait for healthy]
    R --> S[Connected!]
```

### First-Run Interaction

```
$ cd my-project
$ hive start

Initializing Hive in my-project/.hive/
? How many agents? (2)
? Agent 1: kilo or claude? [kilo/claude] kilo
? Agent 1 tags? (comma separated) backend
? Agent 2: kilo or claude? [kilo/claude] claude
? Agent 2 tags? (comma separated) frontend
? Start command for dev server? npm run dev
? Test command? npm test
? Check command? npm run check

Created .hive/config.toml
Starting containers...
✓ hive-server
✓ app-container
✓ hive-agent-0
✓ hive-agent-1
✓ Connected to hive-server

Run 'hive ui' to open the TUI
```

**First-run detection:** Check for `.hive/config.toml`. If missing, prompt for config.

## Docker Management

`hive-cli` uses the `bollard` crate (Docker API for Rust) to manage containers.

### Container Lifecycle

```mermaid
stateDiagram-v2
    [*] --> NotCreated
    
    NotCreated --> Creating : hive start
    Creating --> Running : docker run
    Running --> Stopping : hive stop
    Stopping --> Stopped : docker stop
    
    Stopped --> Starting : hive start
    Starting --> Running : docker start
    
    Running --> Restarting : hive restart
    Restarting --> Running
    
    Running --> Removing : hive clean
    Removing --> [*]
```

### Container Setup

**hive-server:**
```bash
docker run -d \
  --name hive-server \
  --network hive-net \
  -p 8080:8080 \
  -v $(pwd)/.hive:/data \
  hive-server:latest
```

**app-container:**
```bash
docker run -d \
  --name app-container \
  --network hive-net \
  -v $(pwd):/app:Delegated \
  -v $(pwd)/.hive:/app/.hive:ro \
  -p 3000:3000 \
  app-container:latest
```

**hive-agent (per agent):**
```bash
docker run -d \
  --name hive-agent-0 \
  --network hive-net \
  -v $(pwd):/app:Delegated \
  -v $(pwd)/.hive:/app/.hive:ro \
  -e HIVE_AGENT_ID=agent-0 \
  -e HIVE_SERVER_URL=ws://hive-server:8080 \
  -e HIVE_APP_DAEMON_URL=http://app-container:8081 \
  -e CODING_AGENT=kilo \
  -e AGENT_TAGS=backend \
  hive-agent:latest
```

### Networks

Create `hive-net` bridge network on first start:
```bash
docker network create hive-net 2>/dev/null || true
```

## TUI (User Control Terminal)

Built with `ratatui`.

### Screen Navigation

```mermaid
flowchart LR
    subgraph TUI["TUI Screens"]
        Dashboard[Dashboard]
        Tasks[Tasks]
        Topics[Message Board]
        Agents[Agents]
        Settings[Settings]
    end
    
    Dashboard -->|j/k| Tasks
    Tasks -->|h/l| Dashboard
    Dashboard -->|j/k| Topics
    Topics -->|h/l| Dashboard
    Dashboard -->|j/k| Agents
    Agents -->|h/l| Dashboard
    Dashboard -->|:| Settings
    Settings -->|Esc| Dashboard
```

### Screens

**1. Dashboard (main screen)**
- Agent status (connected, working on task, idle)
- Task queue (next 5 tasks)
- Recent messages
- Quick actions

**2. Tasks**
- List all tasks with status
- Filter by status, tag, assignee
- Create/edit task
- Set dependencies

**3. Message Board**
- List topics
- View topic + comments
- Create topic
- Blocking/non-blocking read controls

**4. Agent View**
- See what each agent is doing
- Push message to agent
- View agent logs

**5. Settings**
- Edit config
- View/manage API keys
- Container management

### Keybindings (vim-style)

| Key | Action |
|-----|--------|
| `Esc` | Back / Cancel |
| `q` | Quit |
| `j/k` | Down/up |
| `h/l` | Left/right (nav) |
| `Enter` | Select |
| `:` | Command palette |
| `g` | Go top |
| `G` | Go bottom |
| `Ctrl+r` | Refresh |
| `Ctrl+c` | Interrupt agent |

## Configuration Management

### Config File Location

- Default: `.hive/config.toml` (relative to project)
- Override: `--config /path/to/config.toml`

### Config Schema

```toml
# .hive/config.toml

[server]
host = "hive-server"
port = 8080

[agents]
count = 2
default_tags = []

[agent.0]
coding_agent = "kilo"
tags = ["backend"]

[agent.1]
coding_agent = "claude" 
tags = ["frontend"]

[app]
start_command = "npm run dev"
test_command = "npm test"
check_command = "npm run check"
restart_command = "npm run restart"
dev_port = 3000

[tools]
parallel = ["test", "check"]
queued = ["start", "restart", "stop", "logs"]

[logging]
level = "info"
```

### Database

SQLite at `.hive/hive.db`. Created by hive-server on first start.

## API Keys

Passed via environment to containers:
- `ANTHROPIC_API_KEY` → hive-agent (for Claude Code)
- `OPENAI_API_KEY` → hive-agent (if using OpenAI models with Kilo)
- `KILO_API_KEY` → hive-agent (if using Kilo's cloud)

Read from:
1. Config file (`[agent.0].api_key`)
2. Environment variables
3. `.env` file in `.hive/`

## Error Handling

- Container startup failures → Show error, offer retry
- Server unreachable → Auto-reconnect with backoff
- Agent disconnected → Mark in TUI, option to restart

## Logging

- `hive-cli` logs to stderr (or file with `--log-file`)
- Container logs accessible via `hive logs [container]`
- All containers log to stdout, collected by Docker

---

## References

### Related Sections

- [Overview](./00-overview.md) - Problem statement
- [Architecture](./01-architecture.md) - System overview
- [Docker](./05-docker.md) - Container specs
- [Configuration](./06-configuration.md) - Config format

### Deep Links

- [Docker management](./02-hive-cli.md#docker-management) - Container lifecycle
- [TUI screens](./02-hive-cli.md#tui-user-control-terminal) - Screen descriptions
- [Config schema](./06-configuration.md#schema) - Config options

### See Also

- [Glossary](./07-glossary.md) - Term definitions
- [Index](./index.md) - File index
