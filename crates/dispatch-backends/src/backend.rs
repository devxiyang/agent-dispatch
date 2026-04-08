use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::path::PathBuf;

use dispatch_core::{
    BackendConfig, BackendInvocation, BackendKind, ExecutionMode, SessionCaptureStrategy,
    SessionLocator, SessionRef,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{BackendError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub native_sessions: bool,
    pub resumable: bool,
    pub forkable: bool,
    pub structured_output: bool,
    pub explicit_session_locator: bool,
    pub supports_auto_mode: bool,
    pub supports_danger_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Availability {
    pub kind: BackendKind,
    pub executable: String,
    pub installed: bool,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct StartSpec {
    pub workspace_root: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
    pub session_dir: Option<PathBuf>,
    pub execution_mode: ExecutionMode,
    pub backend_config: Option<BackendConfig>,
}

#[derive(Debug, Clone)]
pub struct ResumeSpec {
    pub session: SessionRef,
    pub prompt: String,
    pub model: Option<String>,
    pub execution_mode: ExecutionMode,
    pub backend_config: Option<BackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPlan {
    pub invocation: BackendInvocation,
    pub session_capture: SessionCaptureStrategy,
    pub session_hint: Option<SessionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumePlan {
    pub invocation: BackendInvocation,
    pub session: SessionRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkPlan {
    pub invocation: Option<BackendInvocation>,
    pub session: SessionRef,
}

pub trait AgentBackend {
    fn kind(&self) -> BackendKind;
    fn capabilities(&self) -> BackendCapabilities;
    fn detect(&self) -> Availability;
    fn start_plan(&self, spec: &StartSpec) -> Result<StartPlan>;
    fn resume_plan(&self, spec: &ResumeSpec) -> Result<ResumePlan>;
    fn fork_plan(&self, session: &SessionRef) -> Result<ForkPlan>;
}

pub(crate) fn base_invocation(
    program: &str,
    cwd: PathBuf,
    args: Vec<String>,
    stdin: Option<String>,
) -> BackendInvocation {
    BackendInvocation {
        program: program.into(),
        args,
        cwd,
        env: BTreeMap::new(),
        stdin,
    }
}

pub(crate) fn named_session(
    backend: BackendKind,
    workspace_root: PathBuf,
    session_storage: Option<PathBuf>,
) -> SessionRef {
    SessionRef {
        backend,
        locator: SessionLocator::Name(format!("dispatch-{}", Uuid::new_v4())),
        workspace_root,
        session_storage,
    }
}

pub(crate) fn require_session_file(session: &SessionRef) -> Result<PathBuf> {
    match &session.locator {
        SessionLocator::File(path) => Ok(path.clone()),
        other => Err(BackendError::Unsupported(format!(
            "expected file-backed session for {:?}",
            other
        ))),
    }
}

pub(crate) fn invocation_from_backend_config(
    config: &BackendConfig,
    cwd: PathBuf,
    stdin: Option<String>,
    workspace_root: &Path,
    session_file: Option<&Path>,
    _model: Option<&str>,
) -> BackendInvocation {
    let args = config
        .args
        .iter()
        .map(|arg| {
            arg.replace("{workspace}", &workspace_root.display().to_string())
                .replace(
                    "{session_file}",
                    &session_file
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                )
        })
        .collect();
    BackendInvocation {
        program: config.executable.clone(),
        args,
        cwd,
        env: BTreeMap::new(),
        stdin,
    }
}

pub(crate) fn is_command_available(program: &str) -> bool {
    let path = match env::var_os("PATH") {
        Some(path) => path,
        None => return false,
    };

    env::split_paths(&path).any(|dir| executable_exists(&dir, program))
}

fn executable_exists(dir: &Path, program: &str) -> bool {
    let candidate = dir.join(program);
    if candidate.is_file() {
        return true;
    }

    #[cfg(windows)]
    {
        let candidate = dir.join(format!("{program}.exe"));
        if candidate.is_file() {
            return true;
        }
    }

    false
}
