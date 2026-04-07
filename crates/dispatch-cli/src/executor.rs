use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result, bail};
use dispatch_core::{
    BackendInvocation, DispatchStore, EventKind, SessionCaptureStrategy, SessionLocator,
    SessionRef, StepStatus, TaskStatus, list_pending_questions,
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct ExecutionSummary {
    pub task_id: Uuid,
    pub status: String,
    pub exit_code: Option<i32>,
    pub session: Option<SessionRef>,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

pub fn execute_plan(
    store: &DispatchStore,
    task_id: Uuid,
    invocation: &BackendInvocation,
    capture: &SessionCaptureStrategy,
) -> Result<ExecutionSummary> {
    let task = store.load_task(task_id)?;
    let attempt = task.checkpoint.restart_count;
    let stdout_path = task
        .artifacts
        .outputs_dir
        .join(format!("attempt-{attempt:03}.stdout.log"));
    let stderr_path = task
        .artifacts
        .outputs_dir
        .join(format!("attempt-{attempt:03}.stderr.log"));

    store.append_event(
        task_id,
        EventKind::InvocationStarted,
        format!(
            "running `{}` in {}",
            invocation.program,
            invocation.cwd.display()
        ),
    )?;

    let output = run_command(invocation).with_context(|| {
        format!(
            "failed to execute backend command `{}` in {}",
            invocation.program,
            invocation.cwd.display()
        )
    })?;

    let stdout_text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr_text = String::from_utf8_lossy(&output.stderr).into_owned();
    fs::write(&stdout_path, &stdout_text)
        .with_context(|| format!("write stdout log to {}", stdout_path.display()))?;
    fs::write(&stderr_path, &stderr_text)
        .with_context(|| format!("write stderr log to {}", stderr_path.display()))?;

    store.append_event(
        task_id,
        EventKind::OutputSaved,
        format!(
            "saved stdout to {} and stderr to {}",
            stdout_path.display(),
            stderr_path.display()
        ),
    )?;

    let session = resolve_session(
        capture,
        &stdout_text,
        &task.workspace_root,
        Some(task.artifacts.sessions_dir.as_path()),
    )?;
    let success = output.status.success();
    let synced_plan = load_plan_statuses(&task.artifacts.plan_file, &task.plan);
    let pending_questions = list_pending_questions(&task.artifacts.mailbox_dir)?;
    let completion_marker = task.artifacts.mailbox_dir.join(".done");

    store.update_task(task_id, |task| {
        task.session = session.clone().or_else(|| task.session.clone());
        task.checkpoint.last_error = if success {
            None
        } else {
            Some(render_exit_status(&output.status))
        };
        if let Some(plan) = &synced_plan {
            task.plan = plan.clone();
        } else if let Some(step) = task.plan.get_mut(1) {
            step.status = if success {
                StepStatus::Done
            } else {
                StepStatus::Failed
            };
        }
        task.status = if !pending_questions.is_empty() {
            TaskStatus::AwaitingUser
        } else if success && completion_marker.exists() {
            TaskStatus::Completed
        } else if success
            && task
                .plan
                .iter()
                .all(|step| matches!(step.status, StepStatus::Done))
        {
            TaskStatus::Completed
        } else if success {
            TaskStatus::Running
        } else {
            TaskStatus::Failed
        };
    })?;

    store.append_event(
        task_id,
        EventKind::InvocationFinished,
        format!("command exited with {}", render_exit_status(&output.status)),
    )?;
    if success {
        store.append_event(task_id, EventKind::Completed, "task execution completed")?;
    } else {
        store.append_event(task_id, EventKind::Failed, "task execution failed")?;
    }

    Ok(ExecutionSummary {
        task_id,
        status: if success { "completed" } else { "failed" }.into(),
        exit_code: output.status.code(),
        session,
        stdout_path,
        stderr_path,
    })
}

fn run_command(invocation: &BackendInvocation) -> Result<std::process::Output> {
    let mut command = Command::new(&invocation.program);
    command
        .args(&invocation.args)
        .current_dir(&invocation.cwd)
        .stdin(if invocation.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in &invocation.env {
        command.env(key, value);
    }

    let mut child = command.spawn()?;
    if let Some(stdin) = &invocation.stdin {
        use std::io::Write;

        let mut handle = child
            .stdin
            .take()
            .context("failed to acquire child stdin handle")?;
        handle.write_all(stdin.as_bytes())?;
    }

    Ok(child.wait_with_output()?)
}

fn resolve_session(
    capture: &SessionCaptureStrategy,
    stdout_text: &str,
    workspace_root: &Path,
    session_storage: Option<&Path>,
) -> Result<Option<SessionRef>> {
    match capture {
        SessionCaptureStrategy::None => Ok(None),
        SessionCaptureStrategy::Preallocated(session) => Ok(Some(session.clone())),
        SessionCaptureStrategy::StdoutJson { field } => {
            let value = extract_json_field(stdout_text, field);
            match value {
                Some(Value::String(id)) => Ok(Some(SessionRef {
                    backend: dispatch_core::BackendKind::Codex,
                    locator: SessionLocator::Id(id),
                    workspace_root: workspace_root.to_path_buf(),
                    session_storage: session_storage.map(Path::to_path_buf),
                })),
                Some(other) => bail!("session field `{field}` was not a string: {other}"),
                None => Ok(None),
            }
        }
    }
}

fn extract_json_field(stdout_text: &str, field: &str) -> Option<Value> {
    for line in stdout_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Some(found) = find_field_recursive(&value, field) {
                return Some(found.clone());
            }
        }
    }
    None
}

fn find_field_recursive<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(field) {
                return Some(found);
            }
            for nested in map.values() {
                if let Some(found) = find_field_recursive(nested, field) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_field_recursive(item, field)),
        _ => None,
    }
}

fn render_exit_status(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "terminated by signal".into(),
    }
}

fn load_plan_statuses(
    plan_path: &Path,
    fallback: &[dispatch_core::PlanStep],
) -> Option<Vec<dispatch_core::PlanStep>> {
    let text = fs::read_to_string(plan_path).ok()?;
    let mut by_id = std::collections::BTreeMap::new();
    let mut ordered_statuses = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim().strip_prefix("- ").unwrap_or(line.trim());
        let (status, rest) = if let Some(rest) = trimmed.strip_prefix("[ ] ") {
            (StepStatus::Pending, rest)
        } else if let Some(rest) = trimmed.strip_prefix("[>] ") {
            (StepStatus::Running, rest)
        } else if let Some(rest) = trimmed.strip_prefix("[x] ") {
            (StepStatus::Done, rest)
        } else if let Some(rest) = trimmed.strip_prefix("[?] ") {
            (StepStatus::Blocked, rest)
        } else if let Some(rest) = trimmed.strip_prefix("[!] ") {
            (StepStatus::Failed, rest)
        } else {
            continue;
        };
        ordered_statuses.push(status.clone());

        let id = rest
            .rsplit_once("(`")
            .and_then(|(_, suffix)| suffix.strip_suffix("`)"))
            .or_else(|| {
                rest.rsplit_once('(')
                    .and_then(|(_, suffix)| suffix.strip_suffix(')'))
            });
        if let Some(id) = id {
            by_id.insert(id.to_string(), status);
        }
    }

    if by_id.is_empty() && ordered_statuses.is_empty() {
        return None;
    }

    let mut plan = fallback.to_vec();
    let mut ordered = ordered_statuses.into_iter();
    for step in &mut plan {
        if let Some(status) = by_id.get(&step.id) {
            step.status = status.clone();
        } else if let Some(status) = ordered.next() {
            step.status = status;
        }
    }
    Some(plan)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::Path;

    use dispatch_core::{
        BackendInvocation, BackendKind, DispatchStore, ExecutionMode, PlanStep,
        SessionCaptureStrategy, StepStatus, TaskDraft, TaskMode, TaskSource, TaskStatus,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::{execute_plan, extract_json_field, find_field_recursive, resolve_session};

    #[test]
    fn extracts_nested_session_field_from_jsonl() {
        let stdout = r#"{"event":"start"}
{"result":{"session_id":"abc-123","message":"done"}}"#;
        let field = extract_json_field(stdout, "session_id").unwrap();
        assert_eq!(field, json!("abc-123"));
    }

    #[test]
    fn finds_recursive_fields() {
        let value = json!({"a": [{"b": {"session_id": "id-1"}}]});
        let found = find_field_recursive(&value, "session_id").unwrap();
        assert_eq!(found, &json!("id-1"));
    }

    #[test]
    fn resolves_stdout_json_session() {
        let session = resolve_session(
            &SessionCaptureStrategy::StdoutJson {
                field: "session_id".into(),
            },
            "{\"session_id\":\"sess-1\"}",
            Path::new("/tmp/workspace"),
            Some(Path::new("/tmp/sessions")),
        )
        .unwrap()
        .unwrap();

        assert!(matches!(session.backend, BackendKind::Codex));
        assert!(matches!(
            session.locator,
            dispatch_core::SessionLocator::Id(_)
        ));
    }

    #[test]
    fn executes_and_persists_output_artifacts() {
        let root = env::temp_dir().join(format!("dispatch-exec-test-{}", Uuid::new_v4()));
        let store = DispatchStore::new(&root);
        let task = store
            .create_task(TaskDraft {
                title: "Executor smoke".into(),
                prompt: "test".into(),
                task_mode: TaskMode::Plan,
                task_source: TaskSource::InlinePrompt,
                backend: BackendKind::Codex,
                model: None,
                execution_mode: ExecutionMode::Auto,
                preserve_plan_file: false,
                plan_body: None,
                workspace_root: Path::new("/tmp").to_path_buf(),
                plan: vec![
                    PlanStep {
                        id: "plan".into(),
                        title: "Prepare".into(),
                        status: StepStatus::Done,
                        notes: vec![],
                    },
                    PlanStep {
                        id: "run".into(),
                        title: "Execute".into(),
                        status: StepStatus::Running,
                        notes: vec![],
                    },
                    PlanStep {
                        id: "recover".into(),
                        title: "Recover".into(),
                        status: StepStatus::Pending,
                        notes: vec![],
                    },
                ],
            })
            .unwrap();

        let invocation = BackendInvocation {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!(
                    "printf '{{\"session_id\":\"sess-local\"}}\\n' && : > '{}' && cat > '{}' <<'OUT'\ncompleted\nOUT",
                    task.artifacts.mailbox_dir.join(".done").display(),
                    task.artifacts.output_file.display(),
                ),
            ],
            cwd: Path::new("/tmp").to_path_buf(),
            env: Default::default(),
            stdin: None,
        };

        let summary = execute_plan(
            &store,
            task.id,
            &invocation,
            &SessionCaptureStrategy::StdoutJson {
                field: "session_id".into(),
            },
        )
        .unwrap();

        assert_eq!(summary.status, "completed");
        assert!(summary.stdout_path.exists());
        assert!(summary.stderr_path.exists());

        let updated = store.load_task(task.id).unwrap();
        assert!(matches!(updated.status, TaskStatus::Completed));
        assert!(updated.session.is_some());
        assert!(updated.artifacts.output_file.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
