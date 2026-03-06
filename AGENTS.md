# Agent Rules for tt Task Tracker

## Required Rules

```
## Task Tracking (tt)

You MUST use the tt task tracker for all work.

### Every Session:
1. Get task: `get_current_task` → none? `advance_task` starts first available one → returns task
2. Work - create internal todo
3. Done? `advance_task` → returns next task

### Blocked?
- Waiting: `edit_task(id=X, action="block")`
- Ready: `edit_task(id=X, action="unblock")`

### Essential Tools (tt namespace)
| Tool | Use |
|------|-----|
| `get_current_task` | Active task |
| `advance_task` | Complete + start next, start first if no active task |
| `edit_task(action="block")` | Block |
| `edit_task(action="unblock")` | Unblock |
| `list_tasks(status="pending", limit=1)` | Next pending task |
| `list_tasks(active=true)` | All active tasks (pending or in_progress) |

### Key Rules:
- ALWAYS get task from tt
- ALWAYS create internal todo breakdown
- ALWAYS start the first task with `advance_task`
- NEVER skip sub-step creation
- NEVER complete manually → use `advance_task`
```

