# App Daemon — Manual Test Plan

End-to-end verification of app-daemon process management and MCP tool integration.

## Prerequisites

```bash
cargo build --release -p app-daemon -p hive-agent
```

Start the app-daemon (or via Docker):

```bash
# Direct:
HIVE_APP_DAEMON_PORT=8081 ./target/release/app-daemon

# Docker:
cd docker && docker compose build && docker compose up
```

## 1. Health Check

```bash
curl http://localhost:8081/health
```

Expected: `{"status":"ok"}`

## 2. Start Dev Server

```bash
curl -X POST http://localhost:8081/dev/start
```

Expected: `{"status":"ok","pid":<N>,"command":"..."}` where pid > 0

## 3. Check Status (Running)

```bash
curl http://localhost:8081/dev/status
```

Expected: `{"running":true,"pid":<N>,"command":"...","uptime_secs":<N>}`

## 4. Get Logs

```bash
curl http://localhost:8081/dev/logs
```

Expected: `{"output":"...","line_count":<N>}` with process output

## 5. Get Logs with Tail

```bash
curl 'http://localhost:8081/dev/logs?tail=5'
```

Expected: `{"output":"...","line_count":<N>}` where line_count <= 5

## 6. Send Stdin

```bash
curl -X POST http://localhost:8081/dev/stdin \
  -H 'Content-Type: application/json' \
  -d '{"input":"hello\n"}'
```

Expected: `{"status":"ok"}` (or error if process doesn't accept stdin)

## 7. Stop Dev Server

```bash
curl -X POST http://localhost:8081/dev/stop
```

Expected: `{"status":"ok"}`

## 8. Status After Stop

```bash
curl http://localhost:8081/dev/status
```

Expected: `{"running":false,"pid":null,"command":null,"uptime_secs":null}`

## 9. Restart

Start the dev server, then restart:

```bash
curl -X POST http://localhost:8081/dev/start
# Note the pid from response

curl -X POST http://localhost:8081/dev/restart
# Verify new pid differs from the first
```

Expected: second response has different pid than the first

## 10. Exec (Synchronous Command)

```bash
curl -X POST http://localhost:8081/exec \
  -H 'Content-Type: application/json' \
  -d '{"command":"test"}'
```

Expected: `{"status":"ok","output":"...","exit_code":0}`

## 11. MCP Tool Integration

When an agent connects to the hive-server, verify:

1. `app.dev` appears in the MCP tool list
2. Tool schema includes: `action` (required), `tail` (optional), `input` (optional)
3. Calling `app.dev` with `{"action":"status"}` returns dev server status

## Cleanup

```bash
curl -X POST http://localhost:8081/dev/stop
```

Stop the app-daemon process (Ctrl+C triggers graceful shutdown which kills all tracked processes).
