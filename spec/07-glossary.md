# Glossary

## Terms

### Agent
A single AI coding assistant running inside a `hive-agent` container. Can be configured to use Kilo or Claude Code as the underlying coding agent.

### app-container
The Docker container that runs the shared development environment. Contains pre-installed tools (Node, Rust, Python, Go) and runs the `app-daemon`.

### app-daemon
A simple HTTP server running inside the `app-container` that executes development commands (test, lint, start dev server, etc.) on behalf of agents.

### Blocking Read
A read operation that waits for new content to arrive (or a timeout) before returning. Used in the message board for agents that need to wait for responses from other agents. See [`topic_wait`](./03-hive-server.md#topic_wait).

### coding-agent
The underlying AI tool that actually writes code: either Kilo or Claude Code. `hive-agent` wraps this and adds coordination capabilities.

### Container
A Docker container - an isolated execution environment.

### Control Plane
The centralized coordination logic. In The Hive, this is `hive-server`. Compare to "data plane" which handles actual task execution.

### DAG
Directed Acyclic Graph. Used for task dependencies - tasks form a DAG where edges represent "depends on" relationships. The graph must be acyclic (no circular dependencies).

### Dependency
A relationship between tasks where task B depends on task A. Task B cannot be worked on until task A is complete.

### Docker Network
A Docker bridge network that allows containers to communicate. The Hive creates a `hive-net` network.

### get-next
The operation where an agent asks for the next available task. Returns a task that:
- Has no unmet dependencies
- Is pending (not assigned)
- Matches the agent's tags (if specified)

### hive-agent
The Rust binary that runs inside each agent container. It manages the coding agent subprocess, runs the MCP server, and communicates with `hive-server`.

### hive-cli
The user-facing Rust CLI and TUI application. Used to start/stop containers, configure the project, and interact with the swarm.

### hive-server
The Rust binary that runs the coordination control plane. Exposes the WebSocket API for task tracking, message board, and push messaging.

### MCP
Model Context Protocol. An open standard for connecting AI tools to external data sources. The Hive exposes coordination tools via MCP.

### Message Board
A pull-based communication system where agents can create topics, add comments, and read new content. Supports blocking and non-blocking reads.

### Non-blocking Read
A read operation that returns immediately with whatever content is available (or nothing). Compare to "blocking read".

### Overlay Mount
Mounting a directory as read-only to hide it from processes inside a container. Used to hide `.hive/` from agents.

### Position
The order of tasks in the queue. Tasks are sorted by position, respecting dependencies.

### Push Message
A direct, fire-and-forget message from one agent to another. Delivered at the start of the recipient's next turn.

### Session
A coding agent's conversation state. Can be resumed between invocations using a session ID. Sessions provide continuity within a task.

### Session Resumption
The technique of continuing a coding agent's session across multiple invocations. Used to maintain context within a task without running the agent forever.

### Single-turn Execution
Running a coding agent for one prompt/response cycle, then exiting. Combined with session resumption for clean task boundaries + continuity.

### Split
An operation that replaces a task with multiple ordered subtasks. Example: task "Refactor auth" → ["Update login flow", "Update password reset", "Add tests"].

### Status
The current state of a task. One of: `pending`, `in-progress`, `done`, `blocked`, `cancelled`.

### Swarm
A collection of multiple agents working together under coordination. Named "The Hive" as a metaphor.

### Tag
A label attached to tasks and agents for filtering. Agents can request tasks matching specific tags (e.g., "backend", "frontend", "urgent").

### Task
A unit of work in the task tracker. Has title, description, status, tags, and optional dependencies.

### TUI
Terminal User Interface. A text-based UI (like `ratatui`) for interacting with The Hive.

### UCT
User Control Terminal. The TUI component of `hive-cli`.

### Volume Mount
Mapping a host directory into a Docker container. The Hive uses delegated volume mounts for performance.

## Acronyms

| Acronym | Full Form |
|---------|-----------|
| API | Application Programming Interface |
| CLI | Command Line Interface |
| DAG | Directed Acyclic Graph |
| HTTP | HyperText Transfer Protocol |
| MCP | Model Context Protocol |
| SQLite | Structured Query Language (embedded database) |
| TUI | Terminal User Interface |
| WS | WebSocket |

## File Paths

| Path | Description |
|------|-------------|
| `.hive/` | Project config and data directory |
| `.hive/config.toml` | Project configuration file |
| `.hive/hive.db` | SQLite database |
| `.hive/agents/{id}/session` | Session file for resuming coding agent |
| `/app/` | Project code inside containers |
| `/data/` | Data directory inside hive-server container |

## Config Keys

| Key | Description |
|-----|-------------|
| `coding_agent` | Which coding agent to use: "kilo" or "claude" |
| `tags` | Array of tags for task filtering |
| `start_command` | Command to start dev server |
| `test_command` | Command to run tests |
| `check_command` | Command to run lint/type check |

---

## References

### Related Sections

- [Overview](./00-overview.md) - Problem statement
- [Architecture](./01-architecture.md) - System overview
- [Index](./index.md) - File index

### Where Terms Are Used

- Tasks: [03-hive-server.md](./03-hive-server.md)
- MCP: [04-hive-agent.md](./04-hive-agent.md)
- Docker: [05-docker.md](./05-docker.md)
- Configuration: [06-configuration.md](./06-configuration.md)
