## Workflow

Your job is to work through tasks from the `tt` task tracker, until no pending tasks remain.

Follow this exact sequence:

### Step 1: Determine if there's an active Task
Use MCP tool `tt_current` to get the current task.
If there is a current task, continue to step 2.
If there is no current task, then use MCP tool `tt_advance` to start the next one.
If you need to see a list of tasks, use MCP tool `tt_list`.

### Step 2: Fetch External Task
You should already have the task from step 1 (`tt_current` or `tt_advance`), but if you don't, use the MCP tool `tt_current` to fetch the current task from `tt`. Continue to step 3.
If no tasks remain, report "All tasks complete" and stop.

### Step 3: Create Internal Breakdown
Create an internal todo list using update_todo_list with steps to complete the fetched task.
The todo list should be named: "Task: [external task name]"
That is, the internal todo list should split the task from `tt` into atomic sub-steps to complete that task. Then continue to step 4.

### Step 4: Execute and Monitor
Work through the internal todo list step by step.
After completing each internal step, check if all internal steps are done.

### Step 5: Sync Completion
When all internal todo items are complete:
- Use MCP tool `tt_advance` to mark the task complete and move to the next one
- Clear the internal todo list (or mark as complete)
- Return to Step 2 (`tt_advance` should have returned the new current task)

## Critical Rules
- NEVER mark external tasks complete until internal todo is 100% done
- NEVER skip the fetch step — always verify external state
- If external task disappears or changes, pause and report
- Always keep the `tt` task state in sync with work done
- If the user set a "focus", work through the focused list
- Continue working through the `tt` list until there are no more pending tasks remaining.
