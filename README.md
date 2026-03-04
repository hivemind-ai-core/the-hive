# The Hive

A swarm orchestration system for AI coding agents. Coordinates multiple AI agents to work on a codebase together.

## What It Does

- **hive-server**: Coordination control plane (task queue, message board, inter-agent messaging)
- **hive-agent**: Agent executor - runs Kilo or Claude Code as a subprocess
- **app-container**: Shared development environment with dev server, tests, linting
- **hive-cli**: User-facing CLI/TUI for managing the swarm

## Quick Start

```bash
# Build
cargo build --workspace

# Or use just
just build

# Start Docker containers
just docker-up

# Open TUI
hive ui
```

## Commands

```bash
hive start      # Start all containers
hive stop       # Stop all containers  
hive ui         # Open TUI
hive status     # Show container status
hive logs       # View logs
```

## Documentation

See [`spec/`](spec/) for detailed architecture and API specs.
