use std::path::PathBuf;

use dispatch_core::{
    BackendKind, ExecutionMode, SessionCaptureStrategy, SessionLocator, SessionRef,
};
use uuid::Uuid;

use crate::backend::{
    AgentBackend, Availability, BackendCapabilities, ForkPlan, ResumePlan, ResumeSpec, StartPlan,
    StartSpec, base_invocation, invocation_from_backend_config, is_command_available,
    named_session, require_session_file,
};
use crate::error::{BackendError, Result};

#[derive(Debug, Default)]
pub struct CodexBackend;

impl AgentBackend for CodexBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Codex
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_sessions: true,
            resumable: true,
            forkable: true,
            structured_output: true,
            explicit_session_locator: false,
            supports_auto_mode: true,
            supports_danger_mode: true,
        }
    }

    fn detect(&self) -> Availability {
        Availability {
            kind: self.kind(),
            executable: "codex".into(),
            installed: is_command_available("codex"),
            notes: "supports `codex resume` and `codex exec resume`".into(),
        }
    }

    fn start_plan(&self, spec: &StartSpec) -> Result<StartPlan> {
        if let Some(config) = &spec.backend_config {
            let mut invocation = invocation_from_backend_config(
                config,
                spec.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.workspace_root,
                None,
                spec.model.as_deref(),
            );
            apply_codex_execution_mode(&mut invocation.args, &spec.execution_mode);
            if let Some(model) = &spec.model {
                invocation.args.push("--model".into());
                invocation.args.push(model.clone());
            }
            return Ok(StartPlan {
                invocation,
                session_capture: SessionCaptureStrategy::StdoutJson {
                    field: "session_id".into(),
                },
                session_hint: None,
            });
        }
        let mut args = vec![
            "exec".into(),
            "--json".into(),
            "-C".into(),
            spec.workspace_root.display().to_string(),
        ];
        apply_codex_execution_mode(&mut args, &spec.execution_mode);
        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(StartPlan {
            invocation: base_invocation(
                "codex",
                spec.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session_capture: SessionCaptureStrategy::StdoutJson {
                field: "session_id".into(),
            },
            session_hint: None,
        })
    }

    fn resume_plan(&self, spec: &ResumeSpec) -> Result<ResumePlan> {
        if let Some(config) = &spec.backend_config {
            let mut invocation = invocation_from_backend_config(
                config,
                spec.session.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.session.workspace_root,
                None,
                spec.model.as_deref(),
            );
            apply_codex_execution_mode(&mut invocation.args, &spec.execution_mode);
            match &spec.session.locator {
                SessionLocator::Id(id) | SessionLocator::Name(id) => {
                    invocation.args.push(id.clone())
                }
                SessionLocator::MostRecent => invocation.args.push("--last".into()),
                SessionLocator::File(_) => {
                    return Err(BackendError::Unsupported(
                        "codex sessions are id or name based".into(),
                    ));
                }
            }
            if let Some(model) = &spec.model {
                invocation.args.push("--model".into());
                invocation.args.push(model.clone());
            }
            return Ok(ResumePlan {
                invocation,
                session: spec.session.clone(),
            });
        }
        let mut args = vec!["exec".into(), "resume".into(), "--json".into()];
        apply_codex_execution_mode(&mut args, &spec.execution_mode);

        match &spec.session.locator {
            SessionLocator::Id(id) | SessionLocator::Name(id) => args.push(id.clone()),
            SessionLocator::MostRecent => args.push("--last".into()),
            SessionLocator::File(_) => {
                return Err(BackendError::Unsupported(
                    "codex sessions are id or name based".into(),
                ));
            }
        }

        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(ResumePlan {
            invocation: base_invocation(
                "codex",
                spec.session.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session: spec.session.clone(),
        })
    }

    fn fork_plan(&self, session: &SessionRef) -> Result<ForkPlan> {
        let mut args = vec!["fork".into()];
        match &session.locator {
            SessionLocator::Id(id) | SessionLocator::Name(id) => args.push(id.clone()),
            SessionLocator::MostRecent => args.push("--last".into()),
            SessionLocator::File(_) => {
                return Err(BackendError::Unsupported(
                    "codex sessions are id or name based".into(),
                ));
            }
        }

        Ok(ForkPlan {
            invocation: Some(base_invocation(
                "codex",
                session.workspace_root.clone(),
                args,
                None,
            )),
            session: SessionRef {
                backend: BackendKind::Codex,
                locator: SessionLocator::Name(format!("dispatch-fork-{}", Uuid::new_v4())),
                workspace_root: session.workspace_root.clone(),
                session_storage: None,
            },
        })
    }
}

#[derive(Debug, Default)]
pub struct ClaudeBackend;

impl AgentBackend for ClaudeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::ClaudeCode
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_sessions: true,
            resumable: true,
            forkable: true,
            structured_output: true,
            explicit_session_locator: true,
            supports_auto_mode: true,
            supports_danger_mode: true,
        }
    }

    fn detect(&self) -> Availability {
        Availability {
            kind: self.kind(),
            executable: "claude".into(),
            installed: is_command_available("claude"),
            notes: "supports `--continue`, `--resume`, and `--session-id`".into(),
        }
    }

    fn start_plan(&self, spec: &StartSpec) -> Result<StartPlan> {
        let session = SessionRef {
            backend: BackendKind::ClaudeCode,
            locator: SessionLocator::Id(Uuid::new_v4().to_string()),
            workspace_root: spec.workspace_root.clone(),
            session_storage: None,
        };

        if let Some(config) = &spec.backend_config {
            let mut invocation = invocation_from_backend_config(
                config,
                spec.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.workspace_root,
                None,
                spec.model.as_deref(),
            );
            apply_claude_execution_mode(&mut invocation.args, &spec.execution_mode);
            if let SessionLocator::Id(id) = &session.locator {
                invocation.args.push("--session-id".into());
                invocation.args.push(id.clone());
            }
            if let Some(model) = &spec.model {
                invocation.args.push("--model".into());
                invocation.args.push(model.clone());
            }
            return Ok(StartPlan {
                invocation,
                session_capture: SessionCaptureStrategy::Preallocated(session.clone()),
                session_hint: Some(session),
            });
        }

        let mut args = vec![
            "--print".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--session-id".into(),
        ];
        apply_claude_execution_mode(&mut args, &spec.execution_mode);

        if let SessionLocator::Id(id) = &session.locator {
            args.push(id.clone());
        }

        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(StartPlan {
            invocation: base_invocation(
                "claude",
                spec.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session_capture: SessionCaptureStrategy::Preallocated(session.clone()),
            session_hint: Some(session),
        })
    }

    fn resume_plan(&self, spec: &ResumeSpec) -> Result<ResumePlan> {
        if let Some(config) = &spec.backend_config {
            let mut invocation = invocation_from_backend_config(
                config,
                spec.session.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.session.workspace_root,
                None,
                spec.model.as_deref(),
            );
            apply_claude_execution_mode(&mut invocation.args, &spec.execution_mode);
            match &spec.session.locator {
                SessionLocator::Id(id) | SessionLocator::Name(id) => {
                    invocation.args.push("--resume".into());
                    invocation.args.push(id.clone());
                }
                SessionLocator::MostRecent => invocation.args.push("--continue".into()),
                SessionLocator::File(_) => {
                    return Err(BackendError::Unsupported(
                        "claude sessions are id or name based".into(),
                    ));
                }
            }
            if let Some(model) = &spec.model {
                invocation.args.push("--model".into());
                invocation.args.push(model.clone());
            }
            return Ok(ResumePlan {
                invocation,
                session: spec.session.clone(),
            });
        }
        let mut args = vec![
            "--print".into(),
            "--output-format".into(),
            "stream-json".into(),
        ];
        apply_claude_execution_mode(&mut args, &spec.execution_mode);

        match &spec.session.locator {
            SessionLocator::Id(id) | SessionLocator::Name(id) => {
                args.push("--resume".into());
                args.push(id.clone());
            }
            SessionLocator::MostRecent => args.push("--continue".into()),
            SessionLocator::File(_) => {
                return Err(BackendError::Unsupported(
                    "claude sessions are id or name based".into(),
                ));
            }
        }

        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(ResumePlan {
            invocation: base_invocation(
                "claude",
                spec.session.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session: spec.session.clone(),
        })
    }

    fn fork_plan(&self, session: &SessionRef) -> Result<ForkPlan> {
        let child = named_session(
            BackendKind::ClaudeCode,
            session.workspace_root.clone(),
            session.session_storage.clone(),
        );

        let mut args = vec![
            "--print".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--fork-session".into(),
        ];
        apply_claude_execution_mode(&mut args, &ExecutionMode::Auto);

        match &session.locator {
            SessionLocator::Id(id) | SessionLocator::Name(id) => {
                args.push("--resume".into());
                args.push(id.clone());
            }
            SessionLocator::MostRecent => args.push("--continue".into()),
            SessionLocator::File(_) => {
                return Err(BackendError::Unsupported(
                    "claude sessions are id or name based".into(),
                ));
            }
        }

        Ok(ForkPlan {
            invocation: Some(base_invocation(
                "claude",
                session.workspace_root.clone(),
                args,
                None,
            )),
            session: child,
        })
    }
}

#[derive(Debug, Default)]
pub struct PiBackend;

impl AgentBackend for PiBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Pi
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_sessions: true,
            resumable: true,
            forkable: true,
            structured_output: true,
            explicit_session_locator: true,
            supports_auto_mode: false,
            supports_danger_mode: false,
        }
    }

    fn detect(&self) -> Availability {
        Availability {
            kind: self.kind(),
            executable: "pi".into(),
            installed: is_command_available("pi"),
            notes: "supports `--session`, `--continue`, `--resume`, `--fork`, and `--mode json`"
                .into(),
        }
    }

    fn start_plan(&self, spec: &StartSpec) -> Result<StartPlan> {
        let session_dir = spec
            .session_dir
            .clone()
            .ok_or_else(|| BackendError::Unsupported("pi requires a session directory".into()))?;
        let session_path = session_dir.join("session.jsonl");
        let session = SessionRef {
            backend: BackendKind::Pi,
            locator: SessionLocator::File(session_path),
            workspace_root: spec.workspace_root.clone(),
            session_storage: Some(session_dir),
        };

        if let Some(config) = &spec.backend_config {
            let invocation = invocation_from_backend_config(
                config,
                spec.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.workspace_root,
                Some(&require_session_file(&session)?),
                spec.model.as_deref(),
            );
            return Ok(StartPlan {
                invocation,
                session_capture: SessionCaptureStrategy::Preallocated(session.clone()),
                session_hint: Some(session),
            });
        }

        let mut args = vec![
            "--print".into(),
            "--mode".into(),
            "json".into(),
            "--session".into(),
            require_session_file(&session)?.display().to_string(),
        ];
        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(StartPlan {
            invocation: base_invocation(
                "pi",
                spec.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session_capture: SessionCaptureStrategy::Preallocated(session.clone()),
            session_hint: Some(session),
        })
    }

    fn resume_plan(&self, spec: &ResumeSpec) -> Result<ResumePlan> {
        if let Some(config) = &spec.backend_config {
            let invocation = invocation_from_backend_config(
                config,
                spec.session.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.session.workspace_root,
                Some(&require_session_file(&spec.session)?),
                spec.model.as_deref(),
            );
            return Ok(ResumePlan {
                invocation,
                session: spec.session.clone(),
            });
        }
        let mut args = vec![
            "--print".into(),
            "--mode".into(),
            "json".into(),
            "--session".into(),
            require_session_file(&spec.session)?.display().to_string(),
        ];
        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(ResumePlan {
            invocation: base_invocation(
                "pi",
                spec.session.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session: spec.session.clone(),
        })
    }

    fn fork_plan(&self, session: &SessionRef) -> Result<ForkPlan> {
        let fork_path = session
            .session_storage
            .clone()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(format!("fork-{}.jsonl", Uuid::new_v4()));

        Ok(ForkPlan {
            invocation: Some(base_invocation(
                "pi",
                session.workspace_root.clone(),
                vec![
                    "--fork".into(),
                    require_session_file(session)?.display().to_string(),
                    "--session".into(),
                    fork_path.display().to_string(),
                ],
                None,
            )),
            session: SessionRef {
                backend: BackendKind::Pi,
                locator: SessionLocator::File(fork_path),
                workspace_root: session.workspace_root.clone(),
                session_storage: session.session_storage.clone(),
            },
        })
    }
}

#[derive(Debug, Default)]
pub struct CursorAgentBackend;

impl AgentBackend for CursorAgentBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::CursorAgent
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_sessions: false,
            resumable: false,
            forkable: false,
            structured_output: false,
            explicit_session_locator: false,
            supports_auto_mode: true,
            supports_danger_mode: false,
        }
    }

    fn detect(&self) -> Availability {
        Availability {
            kind: self.kind(),
            executable: "agent".into(),
            installed: is_command_available("agent"),
            notes: "command shape is configurable; native session resume still needs on-machine validation".into(),
        }
    }

    fn start_plan(&self, spec: &StartSpec) -> Result<StartPlan> {
        if let Some(config) = &spec.backend_config {
            let mut invocation = invocation_from_backend_config(
                config,
                spec.workspace_root.clone(),
                Some(spec.prompt.clone()),
                &spec.workspace_root,
                None,
                spec.model.as_deref(),
            );
            apply_cursor_execution_mode(&mut invocation.args, &spec.execution_mode);
            if let Some(model) = &spec.model {
                invocation.args.push("--model".into());
                invocation.args.push(model.clone());
            }
            return Ok(StartPlan {
                invocation,
                session_capture: SessionCaptureStrategy::None,
                session_hint: None,
            });
        }
        let mut args = vec![
            "-p".into(),
            "--workspace".into(),
            spec.workspace_root.display().to_string(),
        ];
        apply_cursor_execution_mode(&mut args, &spec.execution_mode);
        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        Ok(StartPlan {
            invocation: base_invocation(
                "agent",
                spec.workspace_root.clone(),
                args,
                Some(spec.prompt.clone()),
            ),
            session_capture: SessionCaptureStrategy::None,
            session_hint: None,
        })
    }

    fn resume_plan(&self, _spec: &ResumeSpec) -> Result<ResumePlan> {
        Err(BackendError::Unsupported(
            "cursor agent resume semantics are not validated yet; use external task checkpoints"
                .into(),
        ))
    }

    fn fork_plan(&self, _session: &SessionRef) -> Result<ForkPlan> {
        Err(BackendError::Unsupported(
            "cursor agent does not have a validated native fork flow yet".into(),
        ))
    }
}

fn apply_codex_execution_mode(args: &mut Vec<String>, mode: &ExecutionMode) {
    match mode {
        ExecutionMode::Standard => {}
        ExecutionMode::Auto => args.push("--full-auto".into()),
        ExecutionMode::Danger => args.push("--dangerously-bypass-approvals-and-sandbox".into()),
    }
}

fn apply_claude_execution_mode(args: &mut Vec<String>, mode: &ExecutionMode) {
    match mode {
        ExecutionMode::Standard => {}
        ExecutionMode::Auto => {
            args.push("--permission-mode".into());
            args.push("auto".into());
        }
        ExecutionMode::Danger => args.push("--dangerously-skip-permissions".into()),
    }
}

fn apply_cursor_execution_mode(args: &mut Vec<String>, mode: &ExecutionMode) {
    match mode {
        ExecutionMode::Standard => {}
        ExecutionMode::Auto | ExecutionMode::Danger => args.push("--force".into()),
    }
}
