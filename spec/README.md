# The Hive - Specification

The Hive is a swarm orchestration system for AI coding agents. This directory contains the technical specification.

## What is The Hive?

The Hive coordinates multiple AI coding agents (Kilo, Claude Code) to work together on a shared codebase. It provides:

- **Task coordination** - Centralized task tracker with dependencies
- **Communication** - Message board + push messaging for inter-agent communication  
- **Isolation** - Each agent runs in its own Docker container
- **Efficiency** - Shared development environment for tests/linting
- **Control** - TUI for monitoring and interaction

## Quick Links

| I want to... | Read this |
|--------------|-----------|
| Understand why this exists | [Overview](./00-overview.md) |
| See how the system fits together | [Architecture](./01-architecture.md) |
| Use the CLI or TUI | [hive-cli](./02-hive-cli.md) |
| Understand the server API | [hive-server](./03-hive-server.md) |
| See how agents execute code | [hive-agent](./04-hive-agent.md) |
| Set up Docker containers | [Docker](./05-docker.md) |
| Configure the project | [Configuration](./06-configuration.md) |
| Look up a term | [Glossary](./07-glossary.md) |

## Table of Contents

1. [Overview](./00-overview.md) - Problem statement, rationale, solution
2. [Architecture](./01-architecture.md) - System design, components, data flow
3. [hive-cli](./02-hive-cli.md) - CLI commands, Docker management, TUI
4. [hive-server](./03-hive-server.md) - Server API, task tracker, message board
5. [hive-agent](./04-hive-agent.md) - Agent execution, MCP tools, sessions
6. [Docker](./05-docker.md) - Container specs, networking, volumes
7. [Configuration](./06-configuration.md) - Config format, environment
8. [Glossary](./07-glossary.md) - Term definitions

## Key Concepts

### Task Tracker
Agents claim tasks from a shared queue. Tasks have:
- Status: pending → in-progress → done (or blocked/cancelled)
- Tags for filtering (e.g., "backend", "frontend")
- Dependencies (DAG - tasks must respect dependency order)

### Message Board
Pull-based communication:
- Create topics with initial content
- Add comments
- Block waiting for new content (with timeout)
- Non-blocking "new since X" queries

### Push Messages
Fire-and-forget messages between agents:
- Sent to recipient's next turn
- Simple text format: `[sender]: message`

### Single-Turn Execution
Each task runs in a fresh coding agent session. Within a task, session resumption provides continuity. This gives:
- Clean task boundaries
- No context leaking
- Predictable resource usage

## File Structure

```
spec/
├── README.md          # This file
├── index.md           # Machine-readable index
├── 00-overview.md     # Problem & solution
├── 01-architecture.md # System architecture
├── 02-hive-cli.md     # CLI & TUI
├── 03-hive-server.md # Server API
├── 04-hive-agent.md   # Agent execution
├── 05-docker.md       # Docker containers
├── 06-configuration.md # Configuration
└── 07-glossary.md    # Definitions
```

## Reading Order

**If you're new to The Hive:**

1. Start with [Overview](./00-overview.md) to understand the problem
2. Read [Architecture](./01-architecture.md) for the big picture
3. Dive into your area of interest

**If you're implementing something:**

- CLI → [02-hive-cli.md](./02-hive-cli.md)
- Server → [03-hive-server.md](./03-hive-server.md)
- Agent → [04-hive-agent.md](./04-hive-agent.md)
- DevOps → [05-docker.md](./05-docker.md) + [06-configuration.md](./06-configuration.md)

## Contributing

This specification is the source of truth. Before implementing:
1. Check if the spec covers your case
2. If not, propose an addition
3. Update the spec first, then implement

## License

See project root for license information.
