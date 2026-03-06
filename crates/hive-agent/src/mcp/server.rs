//! MCP server using the rmcp crate over TCP transport.

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    schemars, tool, tool_handler, tool_router,
    ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

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
    #[tool(name = "task.get_next", description = "Get the next pending task for the current agent")]
    async fn task_get_next(
        &self,
        Parameters(p): Parameters<TaskGetNextParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({ "tag": p.tag });
        super::tools::tasks::get_next(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Mark the specified task as done.
    #[tool(name = "task.complete", description = "Mark a task as done and get the next task")]
    async fn task_complete(
        &self,
        Parameters(p): Parameters<TaskCompleteParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({ "id": p.id, "result": p.result });
        super::tools::tasks::complete(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Create a new discussion topic on the message board.
    #[tool(name = "topic.create", description = "Create a new discussion topic on the message board")]
    async fn topic_create(
        &self,
        Parameters(p): Parameters<TopicCreateParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "title": p.title,
            "content": p.content,
            "creator_agent_id": p.creator_agent_id,
        });
        super::tools::topics::create(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// List all topics on the message board.
    #[tool(name = "topic.list", description = "List all discussion topics on the message board")]
    async fn topic_list(&self) -> Result<String, String> {
        super::tools::topics::list(&self.state, None).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Get a topic and its comments.
    #[tool(name = "topic.get", description = "Get a discussion topic and all its comments")]
    async fn topic_get(
        &self,
        Parameters(p): Parameters<TopicGetParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({ "id": p.id });
        super::tools::topics::get(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Post a comment on a topic.
    #[tool(name = "topic.comment", description = "Post a comment on a discussion topic")]
    async fn topic_comment(
        &self,
        Parameters(p): Parameters<TopicCommentParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "topic_id": p.topic_id,
            "content": p.content,
            "creator_agent_id": p.creator_agent_id,
        });
        super::tools::topics::comment(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Wait for a topic to receive a minimum number of comments.
    #[tool(name = "topic.wait", description = "Wait until a topic has a minimum number of comments")]
    async fn topic_wait(
        &self,
        Parameters(p): Parameters<TopicWaitParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "id": p.id,
            "min_comments": p.min_comments,
            "timeout_secs": p.timeout_secs,
        });
        super::tools::topics::wait(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Send a push message to another agent.
    #[tool(name = "push.send", description = "Send a direct message to another agent")]
    async fn push_send(
        &self,
        Parameters(p): Parameters<PushSendParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "to_agent_id": p.to_agent_id,
            "content": p.content,
        });
        super::tools::push::send(&self.state, Some(params)).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// List unread push messages for the current agent.
    #[tool(name = "push.list", description = "List unread direct messages for the current agent")]
    async fn push_list(&self) -> Result<String, String> {
        super::tools::push::list(&self.state, None).await
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// Run a command via the app-daemon (build, test, etc.).
    #[tool(name = "app.exec", description = "Run a project command (build, test, run <cmd>) via the app-daemon")]
    async fn app_exec(
        &self,
        Parameters(p): Parameters<AppExecParams>,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "command": p.command,
            "pattern": p.pattern,
        });
        super::tools::app_exec::exec(&self.state, Some(params)).await
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

// ── TCP server ────────────────────────────────────────────────────────────────

/// Start the MCP TCP server. Accepts connections and serves each client in a spawned task.
pub async fn serve(port: u16, state: McpState) -> anyhow::Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!("MCP TCP server listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        let handler = HiveMcpServer::new(state.clone());
        tokio::spawn(async move {
            match handler.serve(stream).await {
                Ok(running) => {
                    info!("MCP client {peer} connected");
                    running.waiting().await.ok();
                    info!("MCP client {peer} disconnected");
                }
                Err(e) => warn!("MCP client {peer} init error: {e}"),
            }
        });
    }
}
