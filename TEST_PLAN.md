# Manual Test Plan

Agents running:
- `agent-1` — Kilo
- `agent-2` — Kilo
- `agent-claude` — Claude Code

All tests use the WebSocket server at `ws://localhost:8080`.
CLI shortcuts use `wscat -c ws://localhost:8080` or the `hive ui` TUI.
Helper: `HIVE=ws://localhost:8080`

---

## T1 — Task assignment: kilo-tagged task

**Steps:**
1. Create a task with tag `kilo-only` assigned via `task.create`
2. Observe that it is dispatched to agent-1 or agent-2 (Kilo agents)
3. Confirm the agent picks it up (status → `in-progress`)
4. Confirm it eventually reaches `done`

**Expected:** Task dispatched to a Kilo agent and completed.

**Result:** KILO_TASK.txt created with content `42`. Task dispatched to `agent-1`.

**Status:** [x] PASS  [ ] FAIL

---

## T2 — Task assignment: claude-tagged task

**Steps:**
1. Create a task with tag `claude-only` assigned via `task.create`
2. Observe that it is dispatched to `agent-claude`
3. Confirm `agent-claude` picks it up (status → `in-progress`)
4. Confirm it eventually reaches `done`

**Expected:** Task dispatched to Claude agent and completed.

**Result:** Task dispatched to `agent-claude`. CLAUDE_TASK.txt created with content `Claude was here`. Note: Claude required credentials fix before it could execute (`.credentials.json` must be copied to `/home/ubuntu/.claude/` in container).

**Status:** [x] PASS  [ ] FAIL

---

## T3 — Task assignment: untagged task

**Steps:**
1. Create a task with no tags
2. Observe that it is dispatched to any available agent (Kilo or Claude)
3. Confirm the agent picks it up and completes it

**Expected:** Task picked up by whichever agent polls next and completed.

**Result:** UNTAGGED_TASK.txt created with `2026-03-10`. Picked up by `agent-claude`.

**Status:** [x] PASS  [ ] FAIL

---

## T4 — Push message to Kilo

**Steps:**
1. Send a `push.send` message to `agent-1` with content "Hello Kilo, please create a file called HELLO_KILO.txt with content 'Kilo was here'"
2. Observe `agent-1` receives `push.notify`
3. Confirm `HELLO_KILO.txt` appears in the project directory

**Expected:** Kilo receives the push, creates the file.

**Result:** HELLO_KILO.txt created with content `Kilo was here`.

**Status:** [x] PASS  [ ] FAIL

---

## T5 — Push message to Claude

**Steps:**
1. Send a `push.send` message to `agent-claude` with content "Hello Claude, please create a file called HELLO_CLAUDE.txt with content 'Claude was here'"
2. Observe `agent-claude` receives `push.notify`
3. Confirm `HELLO_CLAUDE.txt` appears in the project directory

**Expected:** Claude receives the push, creates the file.

**Result:** CLAUDE_TASK.txt created with content `Claude was here`.

**Status:** [x] PASS  [ ] FAIL

---

## T6 — Create topic + comments (human-created)

**Steps:**
1. Create a topic titled "Integration Test Topic" with content "This topic tests the message board"
2. Add a comment: "Comment from the operator"
3. Verify the topic and comment are retrievable via `board.get` / `board.list`

**Expected:** Topic created, comment attached, both readable.

**Result:** "Integration Test Topic" created, operator comment "Comment from the operator — this board is working!" attached and readable.

**Status:** [x] PASS  [ ] FAIL

---

## T7 — Kilo reads topic, adds comment, creates own topic

**Steps:**
1. Send a push to `agent-1`: "Read the topic titled 'Integration Test Topic' from the message board, add a comment 'Kilo read this topic', then create a new topic titled 'Kilo Topic' with content 'Created by Kilo'"
2. Wait for agent to complete
3. Verify comment by Kilo appears on Integration Test Topic
4. Verify 'Kilo Topic' exists on the board

**Expected:** Kilo comments and creates a topic.

**Result:** Comment "Kilo read this topic" added to Integration Test Topic. "Kilo Topic" created on the board.

**Status:** [x] PASS  [ ] FAIL

---

## T8 — Claude reads topic, adds comment, creates own topic

**Steps:**
1. Send a push to `agent-claude`: "Read the topic titled 'Integration Test Topic' from the message board, add a comment 'Claude read this topic', then create a new topic titled 'Claude Topic' with content 'Created by Claude'"
2. Wait for agent to complete
3. Verify comment by Claude appears on Integration Test Topic
4. Verify 'Claude Topic' exists on the board

**Expected:** Claude comments and creates a topic.

**Result:** Comment "Claude read this topic" added to Integration Test Topic. "Claude Topic" created on the board.

**Status:** [x] PASS  [ ] FAIL

---

## T9 — @mention Kilo via message board

**Steps:**
1. Post a message board comment (on any topic) containing `@agent-1 please acknowledge by writing the word ACKNOWLEDGED to a file called ACK_KILO.txt`
2. Verify `agent-1` receives a `push.notify` triggered by the @mention
3. Confirm `ACK_KILO.txt` appears in the project

**Expected:** @mention triggers push to Kilo; Kilo acts on it.

**Result:** ACK_KILO.txt created with content `ACKNOWLEDGED`.

**Status:** [x] PASS  [ ] FAIL

---

## T10 — @mention Claude via message board

**Steps:**
1. Post a message board comment containing `@agent-claude please acknowledge by writing the word ACKNOWLEDGED to a file called ACK_CLAUDE.txt`
2. Verify `agent-claude` receives a `push.notify` triggered by the @mention
3. Confirm `ACK_CLAUDE.txt` appears in the project

**Expected:** @mention triggers push to Claude; Claude acts on it.

**Result:** ACK_CLAUDE.txt created with content `ACKNOWLEDGED`.

**Status:** [x] PASS  [ ] FAIL

---

## Results Summary

| Test | Description | Result |
|------|-------------|--------|
| T1 | Kilo-tagged task dispatched & completed | PASS ✓ |
| T2 | Claude-tagged task dispatched & completed | PASS ✓ |
| T3 | Untagged task dispatched & completed | PASS ✓ |
| T4 | Push message to Kilo | PASS ✓ |
| T5 | Push message to Claude | PASS ✓ |
| T6 | Topic + comment created by operator | PASS ✓ |
| T7 | Kilo reads topic, comments, creates topic | PASS ✓ |
| T8 | Claude reads topic, comments, creates topic | PASS ✓ |
| T9 | @mention Kilo via message board | PASS ✓ |
| T10 | @mention Claude via message board | PASS ✓ |

## Notes

- **Claude agent credentials**: `hive auth sync` copies `~/.claude.json` (config only). The actual OAuth credentials are in `~/.claude/.credentials.json`. For Claude agents to work, `~/.claude/.credentials.json` must be manually copied to the container at `/home/ubuntu/.claude/.credentials.json`. The `hive auth sync` flow should be updated to also copy `~/.claude/` (or at least `.credentials.json`).
- **Container user mismatch**: The Dockerfile uses `USER agent` (home: `/home/agent`) but the runtime UID override maps to `ubuntu` (home: `/home/ubuntu`). The credential mount in containers.rs targets `/home/agent/.claude` which is unreachable by the running process.
