use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackendKind {
    Codex,
    ClaudeCode,
    Pi,
    CursorAgent,
    Generic,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::Pi => "pi",
            Self::CursorAgent => "cursor-agent",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionMode {
    Standard,
    Auto,
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskMode {
    Direct,
    Plan,
    Discuss,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskSource {
    InlinePrompt,
    PromptFile,
    PlanFile,
    Template,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    AwaitingUser,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionLocator {
    Id(String),
    Name(String),
    File(PathBuf),
    MostRecent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRef {
    pub backend: BackendKind,
    pub locator: SessionLocator,
    pub workspace_root: PathBuf,
    pub session_storage: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionCaptureStrategy {
    None,
    StdoutJson { field: String },
    Preallocated(SessionRef),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCheckpoint {
    pub last_invocation: Option<BackendInvocation>,
    pub session_capture: SessionCaptureStrategy,
    pub restart_count: u32,
    pub last_error: Option<String>,
}

impl Default for RuntimeCheckpoint {
    fn default() -> Self {
        Self {
            last_invocation: None,
            session_capture: SessionCaptureStrategy::None,
            restart_count: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactPaths {
    pub root: PathBuf,
    pub task_file: PathBuf,
    pub events_file: PathBuf,
    pub plan_file: PathBuf,
    pub output_file: PathBuf,
    pub context_file: PathBuf,
    pub mailbox_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub outputs_dir: PathBuf,
}

impl ArtifactPaths {
    pub fn new(root: PathBuf) -> Self {
        Self {
            task_file: root.join("task.json"),
            events_file: root.join("events.jsonl"),
            plan_file: root.join("plan.md"),
            output_file: root.join("output.md"),
            context_file: root.join("context.md"),
            mailbox_dir: root.join("mailbox"),
            sessions_dir: root.join("sessions"),
            outputs_dir: root.join("outputs"),
            root,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: Uuid,
    pub title: String,
    pub prompt: String,
    pub task_mode: TaskMode,
    pub task_source: TaskSource,
    pub backend: BackendKind,
    pub model: Option<String>,
    pub execution_mode: ExecutionMode,
    pub workspace_root: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: TaskStatus,
    pub session: Option<SessionRef>,
    pub checkpoint: RuntimeCheckpoint,
    pub artifacts: ArtifactPaths,
}

#[derive(Debug, Clone)]
pub struct TaskDraft {
    pub title: String,
    pub prompt: String,
    pub task_mode: TaskMode,
    pub task_source: TaskSource,
    pub backend: BackendKind,
    pub model: Option<String>,
    pub execution_mode: ExecutionMode,
    pub plan_body: Option<String>,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    Created,
    SessionPrepared,
    SessionResumed,
    SessionForked,
    InvocationStarted,
    InvocationFinished,
    WaitingForUser,
    UserAnswered,
    StatusChanged,
    CheckpointSaved,
    OutputSaved,
    Completed,
    Failed,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub kind: EventKind,
    pub message: String,
}

impl EventRecord {
    pub fn new(sequence: u64, kind: EventKind, message: impl Into<String>) -> Self {
        Self {
            sequence,
            timestamp: Utc::now(),
            kind,
            message: message.into(),
        }
    }
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}
