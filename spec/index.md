# Specification Index

| File | Description | Tags |
|------|-------------|------|
| [00-overview.md](./00-overview.md) | Problem statement, raison d'être, how the solution solves problems | overview,problem,justification,benefits |
| [01-architecture.md](./01-architecture.md) | System architecture, components, data flow, database schema, execution model | architecture,components,data-flow,schema |
| [02-hive-cli.md](./02-hive-cli.md) | CLI commands, Docker management, TUI screens, initialization flow | cli,tui,commands,docker |
| [03-hive-server.md](./03-hive-server.md) | Server API (tasks, message board, push), state machines, algorithms | server,api,tasks,messages,push |
| [04-hive-agent.md](./04-hive-agent.md) | Agent execution, MCP tools, session resumption, main loop | agent,mcp,execution,session |
| [05-docker.md](./05-docker.md) | Container specs, Dockerfiles, networking, volumes, app-daemon | docker,containers,networking,volumes |
| [06-configuration.md](./06-configuration.md) | Config format, environment variables, directory structure, validation | config,settings,environment |
| [07-glossary.md](./07-glossary.md) | Definitions of terms, acronyms, and file paths | glossary,terms,definitions,acronyms |
| [README.md](./README.md) | Human-facing entry point with table of contents | readme,toc,overview |

## Quick Reference

### Looking for...

- **How agents coordinate?** → [03-hive-server.md](./03-hive-server.md) (tasks, message board, push)
- **How agents run code?** → [04-hive-agent.md](./04-hive-agent.md) (execution, MCP)
- **How to use the CLI?** → [02-hive-cli.md](./02-hive-cli.md)
- **Docker setup?** → [05-docker.md](./05-docker.md)
- **Configuration options?** → [06-configuration.md](./06-configuration.md)
- **Why does this exist?** → [00-overview.md](./00-overview.md)
- **What does X mean?** → [07-glossary.md](./07-glossary.md)

### Tags

```
agents       - agent-related content
api          - API specifications
architecture - system design
cli          - command-line interface
commands     - CLI commands
components   - system components
config       - configuration
data-flow    - data flow diagrams
database     - database schema
definitions  - glossary terms
docker       - containerization
environment  - environment variables
execution    - agent execution
glossary      - terminology
justification - why this exists
mcp          - Model Context Protocol
message-board - communication
network      - networking
overview     - high-level summary
problem      - problem statement
push         - push messaging
schema        - data models
server       - hive-server
session      - session management
settings     - configuration
tags         - filtering/tags
tasks        - task tracker
tui          - terminal UI
volumes       - Docker volumes
```
