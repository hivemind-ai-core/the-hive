//! MCP server using the rmcp crate over Streamable HTTP transport.

use std::sync::Arc;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

use crate::client::{ClientCmd, PendingRequests};

// ── Shared state passed to every handler instance ─────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub agent_id: String,
    pub cmd_tx: UnboundedSender<ClientCmd>,
    pub pending: PendingRequests,
    pub app_daemon_url: String,
    pub http: reqwest::Client,
}

// ── Parameter structs for tools that accept arguments ────────────────────────

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct TaskGetNextParams {
    tag: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskCompleteParams {
    id: String,
    result: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskCreateParams {
    title: String,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct TaskListParams {
    /// Filter by status: "pending", "`in_progress`", "done", or "cancelled"
    status: Option<String>,
    /// Filter by tag (exact match)
    tag: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskGetParams {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskUpdateParams {
    id: String,
    description: Option<String>,
    tags: Option<Vec<String>>,
    /// New status: "pending", "`in_progress`", "done", or "cancelled"
    status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SubtaskSpec {
    title: String,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskSplitParams {
    /// ID of the task to split (must be your currently assigned task)
    id: String,
    /// Ordered list of subtasks; they will be chained so each depends on the previous
    subtasks: Vec<SubtaskSpec>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskSetDependencyParams {
    /// The task that must wait
    task_id: String,
    /// The task that must complete first
    depends_on_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TopicCreateParams {
    title: String,
    content: String,
    creator_agent_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TopicGetParams {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TopicCommentParams {
    topic_id: String,
    content: String,
    creator_agent_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TopicWaitParams {
    id: String,
    min_comments: Option<u64>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PushSendParams {
    to_agent_id: String,
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AppExecParams {
    command: String,
    pattern: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AppDevParams {
    /// Action to perform: start, stop, restart, status, logs, or stdin
    action: String,
    /// For "logs" action: number of tail lines to return
    tail: Option<u64>,
    /// For "stdin" action: text to send to the process
    input: Option<String>,
}

// ── MCP server handler ────────────────────────────────────────────────────────

#[derive(Clone)]
struct HiveMcpServer {
    state: McpState,
    tool_router: ToolRouter<HiveMcpServer>,
}

impl HiveMcpServer {
    fn new(state: McpState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl HiveMcpServer {
    /// Get the next pending task. The task is assigned to the current agent.
    #[tool(
        name = "task.get_next",
        description = "Get the next pending task for the current agent"
    )]
    async fn task_get_next(
        &self,
        Parameters(p): Parameters<TaskGetNextParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({ "tag": p.tag });
        super::tools::tasks::get_next(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Mark the specified task as done.
    #[tool(
        name = "task.complete",
        description = "Mark a task as done and get the next task"
    )]
    async fn task_complete(
        &self,
        Parameters(p): Parameters<TaskCompleteParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({ "id": p.id, "result": p.result });
        super::tools::tasks::complete(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Create a new task. Use this to add work items for yourself or other agents.
    #[tool(
        name = "task.create",
        description = "Create a new pending task. Optionally set description and tags to route it to a specific agent."
    )]
    async fn task_create(
        &self,
        Parameters(p): Parameters<TaskCreateParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "title": p.title,
            "description": p.description,
            "tags": p.tags,
        });
        super::tools::tasks::create(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// List tasks, optionally filtered by status or tag.
    #[tool(
        name = "task.list",
        description = "Browse all tasks. Filter by status (pending/in_progress/done/cancelled) or tag."
    )]
    async fn task_list(&self, Parameters(p): Parameters<TaskListParams>) -> Result<String, String> {
        let params = serde_json::json!({ "status": p.status, "tag": p.tag });
        super::tools::tasks::list(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Get a specific task by ID.
    #[tool(name = "task.get", description = "Fetch a specific task by its ID.")]
    async fn task_get(&self, Parameters(p): Parameters<TaskGetParams>) -> Result<String, String> {
        let params = serde_json::json!({ "id": p.id });
        super::tools::tasks::get(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Update a task's description, tags, or status.
    #[tool(
        name = "task.update",
        description = "Update a task's description, tags, or status. Use status=pending to un-claim a task."
    )]
    async fn task_update(
        &self,
        Parameters(p): Parameters<TaskUpdateParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "id": p.id,
            "description": p.description,
            "tags": p.tags,
            "status": p.status,
        });
        super::tools::tasks::update(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Split your current task into ordered subtasks. The original task is cancelled.
    #[tool(
        name = "task.split",
        description = "Break a task into ordered subtasks. Each subtask depends on the previous. The original task is cancelled and subtasks are dispatched in sequence."
    )]
    async fn task_split(
        &self,
        Parameters(p): Parameters<TaskSplitParams>,
    ) -> Result<String, String> {
        let subtasks: Vec<serde_json::Value> = p
            .subtasks
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "title": s.title,
                    "description": s.description,
                    "tags": s.tags,
                })
            })
            .collect();
        let params = serde_json::json!({ "id": p.id, "subtasks": subtasks });
        super::tools::tasks::split(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Declare that one task must complete before another can start.
    #[tool(
        name = "task.set_dependency",
        description = "Make task_id wait for depends_on_id to complete before it can be dispatched."
    )]
    async fn task_set_dependency(
        &self,
        Parameters(p): Parameters<TaskSetDependencyParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "task_id": p.task_id,
            "depends_on_id": p.depends_on_id,
        });
        super::tools::tasks::set_dependency(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Create a new discussion topic on the message board.
    #[tool(
        name = "topic.create",
        description = "Create a new discussion topic on the message board"
    )]
    async fn topic_create(
        &self,
        Parameters(p): Parameters<TopicCreateParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "title": p.title,
            "content": p.content,
            "creator_agent_id": p.creator_agent_id,
        });
        super::tools::topics::create(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// List all topics on the message board.
    #[tool(
        name = "topic.list",
        description = "List all discussion topics on the message board"
    )]
    async fn topic_list(&self) -> Result<String, String> {
        super::tools::topics::list(&self.state, None)
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Get a topic and its comments.
    #[tool(
        name = "topic.get",
        description = "Get a discussion topic and all its comments"
    )]
    async fn topic_get(&self, Parameters(p): Parameters<TopicGetParams>) -> Result<String, String> {
        let params = serde_json::json!({ "id": p.id });
        super::tools::topics::get(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Post a comment on a topic.
    #[tool(
        name = "topic.comment",
        description = "Post a comment on a discussion topic"
    )]
    async fn topic_comment(
        &self,
        Parameters(p): Parameters<TopicCommentParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "topic_id": p.topic_id,
            "content": p.content,
            "creator_agent_id": p.creator_agent_id,
        });
        super::tools::topics::comment(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Wait for a topic to receive a minimum number of comments.
    #[tool(
        name = "topic.wait",
        description = "Wait until a topic has a minimum number of comments"
    )]
    async fn topic_wait(
        &self,
        Parameters(p): Parameters<TopicWaitParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "id": p.id,
            "min_comments": p.min_comments,
            "timeout_secs": p.timeout_secs,
        });
        super::tools::topics::wait(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// List all agents registered with the hive.
    #[tool(
        name = "agent.list",
        description = "List all agents known to the hive. Use this to discover agent IDs for push.send or @mention in topic comments."
    )]
    async fn agent_list(&self) -> Result<String, String> {
        super::tools::agents::list(&self.state, None)
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Send a push message to another agent.
    #[tool(
        name = "push.send",
        description = "Send a direct message to another agent"
    )]
    async fn push_send(&self, Parameters(p): Parameters<PushSendParams>) -> Result<String, String> {
        let params = serde_json::json!({
            "to_agent_id": p.to_agent_id,
            "content": p.content,
        });
        super::tools::push::send(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// List unread push messages for the current agent.
    #[tool(
        name = "push.list",
        description = "List unread direct messages for the current agent"
    )]
    async fn push_list(&self) -> Result<String, String> {
        super::tools::push::list(&self.state, None)
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Run a command via the app-daemon (build, test, etc.).
    #[tool(
        name = "app.exec",
        description = "Run a project command (build, test, run <cmd>) via the app-daemon"
    )]
    async fn app_exec(&self, Parameters(p): Parameters<AppExecParams>) -> Result<String, String> {
        let params = serde_json::json!({
            "command": p.command,
            "pattern": p.pattern,
        });
        super::tools::app_exec::exec(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Manage the dev server lifecycle (start, stop, restart, status, logs, stdin).
    #[tool(
        name = "app.dev",
        description = "Dev server lifecycle: start, stop, restart, status, logs, stdin. Use action='start' to launch, 'logs' to read output, 'stdin' to send input."
    )]
    async fn app_dev(&self, Parameters(p): Parameters<AppDevParams>) -> Result<String, String> {
        let params = serde_json::json!({
            "action": p.action,
            "tail": p.tail,
            "input": p.input,
        });
        super::tools::app_dev::dev(&self.state, Some(params))
            .await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }
}

#[tool_handler]
impl ServerHandler for HiveMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "hive-agent-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Hive Agent MCP server. Use these tools to coordinate work, \
                 manage tasks, communicate with other agents, and run project commands.",
            )
    }
}

// ── Streamable HTTP server ────────────────────────────────────────────────────

/// Start the MCP Streamable HTTP server on `http://127.0.0.1:{port}/mcp`.
pub async fn serve(port: u16, state: McpState) -> anyhow::Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("MCP HTTP server listening on http://{addr}/mcp");

    let mcp_service = StreamableHttpService::<HiveMcpServer, LocalSessionManager>::new(
        move || Ok(HiveMcpServer::new(state.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", mcp_service);
    axum::serve(listener, router).await?;
    Ok(())
}
