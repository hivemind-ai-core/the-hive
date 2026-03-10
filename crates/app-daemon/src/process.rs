//! Process lifecycle management for long-running commands (e.g. dev servers).

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::info;

/// Fixed-size ring buffer for captured output lines.
pub struct OutputBuffer {
    lines: VecDeque<String>,
    max_lines: usize,
}

impl OutputBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(max_lines.min(1024)),
            max_lines,
        }
    }

    pub fn push(&mut self, line: String) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    /// Return the last `n` lines (or all if `n` is None).
    pub fn tail(&self, n: Option<usize>) -> Vec<&str> {
        match n {
            Some(n) => self
                .lines
                .iter()
                .rev()
                .take(n)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|s| s.as_str())
                .collect(),
            None => self.lines.iter().map(|s| s.as_str()).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

/// A tracked long-running process.
pub struct TrackedProcess {
    pub pid: u32,
    pub command: String,
    pub started_at: Instant,
    pub output: Arc<Mutex<OutputBuffer>>,
    pub stdin: Option<ChildStdin>,
    child_handle: JoinHandle<Option<i32>>,
}

impl TrackedProcess {
    pub fn is_running(&self) -> bool {
        !self.child_handle.is_finished()
    }
}

/// Info returned after starting a process.
#[derive(Debug, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub command: String,
}

/// Status of a tracked process slot.
#[derive(Debug, Serialize)]
pub struct ProcessStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub command: Option<String>,
    pub uptime_secs: Option<u64>,
}

pub type ProcessManager = HashMap<String, TrackedProcess>;
pub type ProcessManagerHandle = Arc<Mutex<ProcessManager>>;

pub fn new_handle() -> ProcessManagerHandle {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Start a process in the given slot, killing any existing process first.
pub async fn start(
    handle: &ProcessManagerHandle,
    slot: &str,
    cmd: &str,
) -> Result<ProcessInfo, String> {
    // Phase 1: remove old process under lock.
    let old = {
        let mut mgr = handle.lock().await;
        mgr.remove(slot)
    };

    // Phase 2: kill old process (no lock held).
    if let Some(old) = old {
        kill_process(old).await;
    }

    // Phase 3: spawn new process.
    let tracked = spawn_tracked(cmd).await?;
    let info = ProcessInfo {
        pid: tracked.pid,
        command: tracked.command.clone(),
    };

    // Phase 4: insert under lock.
    {
        let mut mgr = handle.lock().await;
        mgr.insert(slot.to_string(), tracked);
    }

    info!("Started process in slot '{slot}': {cmd} (pid {})", info.pid);
    Ok(info)
}

/// Stop a process in the given slot.
pub async fn stop(handle: &ProcessManagerHandle, slot: &str) -> Result<(), String> {
    let proc = {
        let mut mgr = handle.lock().await;
        mgr.remove(slot)
    };

    match proc {
        Some(p) => {
            let pid = p.pid;
            kill_process(p).await;
            info!("Stopped process in slot '{slot}' (pid {pid})");
            Ok(())
        }
        None => Err(format!("no process running in slot '{slot}'")),
    }
}

/// Get the status of a process slot.
pub async fn status(handle: &ProcessManagerHandle, slot: &str) -> ProcessStatus {
    let mgr = handle.lock().await;
    match mgr.get(slot) {
        Some(p) => ProcessStatus {
            running: p.is_running(),
            pid: Some(p.pid),
            command: Some(p.command.clone()),
            uptime_secs: Some(p.started_at.elapsed().as_secs()),
        },
        None => ProcessStatus {
            running: false,
            pid: None,
            command: None,
            uptime_secs: None,
        },
    }
}

/// Get output logs from a process slot.
pub async fn get_logs(
    handle: &ProcessManagerHandle,
    slot: &str,
    tail_n: Option<usize>,
) -> Result<(String, usize), String> {
    let mgr = handle.lock().await;
    match mgr.get(slot) {
        Some(p) => {
            let buf = p.output.lock().await;
            let lines = buf.tail(tail_n);
            let count = lines.len();
            Ok((lines.join("\n"), count))
        }
        None => Err(format!("no process in slot '{slot}'")),
    }
}

/// Send input to a running process's stdin.
pub async fn send_stdin(
    handle: &ProcessManagerHandle,
    slot: &str,
    input: &str,
) -> Result<(), String> {
    let mut mgr = handle.lock().await;
    match mgr.get_mut(slot) {
        Some(p) if p.is_running() => match p.stdin.as_mut() {
            Some(stdin) => stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|e| format!("stdin write failed: {e}")),
            None => Err("stdin not available".to_string()),
        },
        Some(_) => Err("process is not running".to_string()),
        None => Err(format!("no process in slot '{slot}'")),
    }
}

/// Kill all tracked processes (for shutdown).
pub async fn kill_all(handle: &ProcessManagerHandle) {
    let procs: Vec<TrackedProcess> = {
        let mut mgr = handle.lock().await;
        mgr.drain().map(|(_, p)| p).collect()
    };
    for p in procs {
        kill_process(p).await;
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

async fn spawn_tracked(cmd: &str) -> Result<TrackedProcess, String> {
    use tokio::process::Command;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;

    let pid = child.id().ok_or("process exited immediately")?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let output = Arc::new(Mutex::new(OutputBuffer::new(10_000)));

    spawn_reader(stdout, Arc::clone(&output));
    spawn_reader(stderr, Arc::clone(&output));

    let child_handle = tokio::spawn(async move { child.wait().await.ok().and_then(|s| s.code()) });

    Ok(TrackedProcess {
        pid,
        command: cmd.to_string(),
        started_at: Instant::now(),
        output,
        stdin,
        child_handle,
    })
}

fn spawn_reader<R: AsyncRead + Unpin + Send + 'static>(stream: R, buffer: Arc<Mutex<OutputBuffer>>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            buffer.lock().await.push(line);
        }
    });
}

async fn kill_process(proc: TrackedProcess) {
    let pid = proc.pid as i32;

    // Send SIGTERM.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Wait up to 5 seconds for clean exit.
    tokio::select! {
        _ = proc.child_handle => {}
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            // SIGKILL if still alive.
            unsafe { libc::kill(pid, libc::SIGKILL); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_buffer_push_and_tail() {
        let mut buf = OutputBuffer::new(5);
        for i in 0..5 {
            buf.push(format!("line {i}"));
        }
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.tail(None), vec!["line 0", "line 1", "line 2", "line 3", "line 4"]);
        assert_eq!(buf.tail(Some(2)), vec!["line 3", "line 4"]);
    }

    #[test]
    fn output_buffer_ring_eviction() {
        let mut buf = OutputBuffer::new(3);
        for i in 0..6 {
            buf.push(format!("line {i}"));
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.tail(None), vec!["line 3", "line 4", "line 5"]);
    }

    #[test]
    fn output_buffer_tail_larger_than_size() {
        let mut buf = OutputBuffer::new(10);
        buf.push("only".to_string());
        assert_eq!(buf.tail(Some(100)), vec!["only"]);
    }

    #[test]
    fn output_buffer_empty() {
        let buf = OutputBuffer::new(10);
        assert_eq!(buf.len(), 0);
        assert!(buf.tail(None).is_empty());
        assert!(buf.tail(Some(5)).is_empty());
    }

    #[tokio::test]
    async fn start_and_status() {
        let handle = new_handle();
        let info = start(&handle, "test", "echo hello && sleep 60").await.unwrap();
        assert!(info.pid > 0);

        let st = status(&handle, "test").await;
        assert!(st.running);
        assert_eq!(st.pid, Some(info.pid));

        // Clean up.
        stop(&handle, "test").await.unwrap();
    }

    #[tokio::test]
    async fn start_replaces_existing() {
        let handle = new_handle();
        let info1 = start(&handle, "test", "sleep 60").await.unwrap();
        let info2 = start(&handle, "test", "sleep 60").await.unwrap();
        assert_ne!(info1.pid, info2.pid);

        stop(&handle, "test").await.unwrap();
    }

    #[tokio::test]
    async fn stop_nonexistent_errors() {
        let handle = new_handle();
        let result = stop(&handle, "nope").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn logs_capture_output() {
        let handle = new_handle();
        start(&handle, "test", "echo hello; echo world").await.unwrap();

        // Wait for process to finish and reader to flush.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let (output, count) = get_logs(&handle, "test", None).await.unwrap();
        assert!(count >= 2, "expected at least 2 lines, got {count}");
        assert!(output.contains("hello"));
        assert!(output.contains("world"));

        // Clean up (process may have exited already).
        let _ = stop(&handle, "test").await;
    }

    #[tokio::test]
    async fn status_not_running_after_exit() {
        let handle = new_handle();
        start(&handle, "test", "echo done").await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let st = status(&handle, "test").await;
        assert!(!st.running);

        let _ = stop(&handle, "test").await;
    }

    #[tokio::test]
    async fn kill_all_cleans_up() {
        let handle = new_handle();
        start(&handle, "a", "sleep 60").await.unwrap();
        start(&handle, "b", "sleep 60").await.unwrap();
        kill_all(&handle).await;

        let mgr = handle.lock().await;
        assert!(mgr.is_empty());
    }

    #[test]
    fn output_buffer_tail_with_limit() {
        let mut buf = OutputBuffer::new(20);
        for i in 0..10 {
            buf.push(format!("line {i}"));
        }
        let last5 = buf.tail(Some(5));
        assert_eq!(last5, vec!["line 5", "line 6", "line 7", "line 8", "line 9"]);
    }

    #[tokio::test]
    async fn start_stop_exits() {
        let handle = new_handle();
        let info = start(&handle, "test", "sleep 999").await.unwrap();
        assert!(info.pid > 0);

        // Verify it's running.
        let st = status(&handle, "test").await;
        assert!(st.running);

        // Stop it.
        stop(&handle, "test").await.unwrap();

        // After stop, the slot should be removed.
        let st = status(&handle, "test").await;
        assert!(!st.running);
        assert!(st.pid.is_none());
    }

    #[tokio::test]
    async fn kill_process_completes() {
        let handle = new_handle();
        start(&handle, "test", "sleep 999").await.unwrap();

        let st = status(&handle, "test").await;
        assert!(st.running);

        // Remove from manager and kill directly.
        let proc = {
            let mut mgr = handle.lock().await;
            mgr.remove("test").unwrap()
        };

        // The child_handle should still be alive.
        assert!(!proc.child_handle.is_finished());

        // Kill via stop (re-insert first).
        {
            let mut mgr = handle.lock().await;
            mgr.insert("test".to_string(), proc);
        }
        stop(&handle, "test").await.unwrap();

        // Slot is gone.
        let mgr = handle.lock().await;
        assert!(!mgr.contains_key("test"));
    }
}
