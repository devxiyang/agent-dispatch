use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use dispatch_backends::{ResumeSpec, StartSpec, backend_for};
use dispatch_core::{
    BackendConfig, BackendKind, DispatchConfig, DispatchStore, EventKind, ExecutionMode,
    ModelConfig, TaskDraft, TaskMode, TaskRecord, TaskSource, TaskStatus,
};
use uuid::Uuid;

use crate::prompt_builder::build_worker_prompt;

#[derive(Debug, Clone)]
pub struct DispatchSummary {
    pub task_id: Uuid,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub backend_key: String,
    pub backend: BackendKind,
    pub model: Option<String>,
    pub alias_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DispatchInput {
    InlinePrompt {
        prompt: String,
    },
    PromptFile {
        _path: PathBuf,
        prompt: String,
    },
    PlanFile {
        _path: PathBuf,
        prompt: String,
        plan_body: String,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum RequestedTaskMode {
    Auto,
    Direct,
    Plan,
    Discuss,
}

#[derive(Debug, Clone, Copy)]
pub enum TemplateKind {
    Generic,
    Feature,
    Bugfix,
    Refactor,
    Audit,
    Research,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReadySummary {
    pub config_path: String,
    pub default_target: String,
    pub backend_count: usize,
    pub model_count: usize,
    pub alias_count: usize,
    pub installed_backends: Vec<String>,
}

pub fn ensure_config() -> Result<DispatchConfig> {
    if let Some(config) = DispatchConfig::load_if_exists()? {
        return Ok(config);
    }
    let config = bootstrap_config();
    config.save()?;
    Ok(config)
}

pub fn readiness_summary() -> Result<ReadySummary> {
    let config = ensure_config()?;
    let installed_backends = dispatch_backends::all_backends()
        .into_iter()
        .filter_map(|backend| {
            let availability = backend.detect();
            availability
                .installed
                .then(|| availability.kind.as_str().to_string())
        })
        .collect();
    Ok(ReadySummary {
        config_path: DispatchConfig::config_path().display().to_string(),
        default_target: config.default,
        backend_count: config.backends.len(),
        model_count: config.models.len(),
        alias_count: config.aliases.len(),
        installed_backends,
    })
}

pub fn bootstrap_config() -> DispatchConfig {
    let mut backends = BTreeMap::new();
    let mut models: BTreeMap<String, ModelConfig> = BTreeMap::new();
    let aliases = BTreeMap::new();

    let detected = dispatch_backends::all_backends();
    for backend in detected {
        let availability = backend.detect();
        if !availability.installed {
            continue;
        }
        match availability.kind {
            BackendKind::Codex => {
                backends.insert(
                    "codex".into(),
                    BackendConfig {
                        executable: "codex".into(),
                        args: vec![
                            "exec".into(),
                            "--json".into(),
                            "-C".into(),
                            "{workspace}".into(),
                        ],
                    },
                );
                for model in [
                    "gpt-5.4-mini",
                    "gpt-5.4",
                    "gpt-5.3-codex",
                    "gpt-5.3-codex-spark",
                    "gpt-5.2",
                ] {
                    models.insert(
                        model.into(),
                        ModelConfig {
                            backend: "codex".into(),
                            model: Some(model.into()),
                        },
                    );
                }
            }
            BackendKind::ClaudeCode => {
                backends.insert(
                    "claude".into(),
                    BackendConfig {
                        executable: "claude".into(),
                        args: vec![
                            "--print".into(),
                            "--output-format".into(),
                            "stream-json".into(),
                            "--permission-mode".into(),
                            "auto".into(),
                        ],
                    },
                );
                for model in ["opus", "sonnet", "haiku"] {
                    models.insert(
                        model.into(),
                        ModelConfig {
                            backend: "claude".into(),
                            model: Some(model.into()),
                        },
                    );
                }
            }
            BackendKind::Pi => {
                backends.insert(
                    "pi".into(),
                    BackendConfig {
                        executable: "pi".into(),
                        args: vec![
                            "--print".into(),
                            "--mode".into(),
                            "json".into(),
                            "--session".into(),
                            "{session_file}".into(),
                        ],
                    },
                );
                models.insert(
                    "pi-default".into(),
                    ModelConfig {
                        backend: "pi".into(),
                        model: None,
                    },
                );
            }
            BackendKind::CursorAgent => {
                backends.insert(
                    "cursor-agent".into(),
                    BackendConfig {
                        executable: "agent".into(),
                        args: vec![
                            "-p".into(),
                            "--force".into(),
                            "--workspace".into(),
                            "{workspace}".into(),
                        ],
                    },
                );
                models.insert(
                    "cursor-default".into(),
                    ModelConfig {
                        backend: "cursor-agent".into(),
                        model: None,
                    },
                );
            }
            BackendKind::Generic => {}
        }
    }

    let default = choose_default_target(&models);

    DispatchConfig {
        default,
        backends,
        models,
        aliases,
    }
}

fn choose_default_target(models: &BTreeMap<String, ModelConfig>) -> String {
    for preferred in [
        "pi-default",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark",
        "gpt-5.2",
        "sonnet",
        "opus",
        "haiku",
        "cursor-default",
    ] {
        if models.contains_key(preferred) {
            return preferred.into();
        }
    }

    models
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "pi-default".into())
}

pub fn resolve_target(
    config: &DispatchConfig,
    backend_override: Option<BackendKind>,
    model_override: Option<String>,
) -> Result<ResolvedTarget> {
    if let Some(backend) = backend_override {
        return Ok(ResolvedTarget {
            backend_key: backend_name_for_kind(&backend).into(),
            backend,
            model: model_override,
            alias_prompt: None,
        });
    }

    if let Some(model_name) = model_override {
        if let Some(alias) = config.aliases.get(&model_name) {
            let resolved_model = resolve_model(config, &alias.model)?;
            return Ok(ResolvedTarget {
                backend_key: alias_backend_key(config, &alias.model)?,
                backend: resolved_model.backend,
                model: resolved_model.model,
                alias_prompt: alias.prompt.clone(),
            });
        }
        let resolved_model = resolve_model(config, &model_name)?;
        return Ok(ResolvedTarget {
            backend_key: model_backend_key(config, &model_name)?,
            backend: resolved_model.backend,
            model: resolved_model.model,
            alias_prompt: None,
        });
    }

    if let Some(alias) = config.aliases.get(&config.default) {
        let resolved_model = resolve_model(config, &alias.model)?;
        return Ok(ResolvedTarget {
            backend_key: alias_backend_key(config, &alias.model)?,
            backend: resolved_model.backend,
            model: resolved_model.model,
            alias_prompt: alias.prompt.clone(),
        });
    }

    let resolved_model = resolve_model(config, &config.default)?;
    Ok(ResolvedTarget {
        backend_key: model_backend_key(config, &config.default)?,
        backend: resolved_model.backend,
        model: resolved_model.model,
        alias_prompt: None,
    })
}

pub fn create_task_from_request(
    store: &DispatchStore,
    title: String,
    input: DispatchInput,
    workspace: PathBuf,
    execution_mode: ExecutionMode,
    task_mode: TaskMode,
    resolved: &ResolvedTarget,
) -> Result<TaskRecord> {
    let (prompt, task_source, plan_body) = match input {
        DispatchInput::InlinePrompt { prompt } => (prompt, TaskSource::InlinePrompt, None),
        DispatchInput::PromptFile { _path: _, prompt } => (prompt, TaskSource::PromptFile, None),
        DispatchInput::PlanFile {
            _path: _,
            prompt,
            plan_body,
        } => (prompt, TaskSource::PlanFile, Some(plan_body)),
    };
    Ok(store.create_task(TaskDraft {
        title,
        prompt,
        task_mode,
        task_source,
        backend: resolved.backend.clone(),
        model: resolved.model.clone(),
        execution_mode,
        plan_body,
        workspace_root: workspace,
    })?)
}

pub fn prepare_start(
    task: &TaskRecord,
    resolved: &ResolvedTarget,
) -> Result<dispatch_backends::StartPlan> {
    let backend_impl = backend_for(&task.backend);
    let mut prompt = build_worker_prompt(task, &task.prompt, None);
    if let Some(prefix) = &resolved.alias_prompt {
        prompt = format!("{prefix}\n\n{prompt}");
    }
    let config = ensure_config()?;
    let backend_config = config.backends.get(&resolved.backend_key).cloned();
    Ok(backend_impl.start_plan(&StartSpec {
        workspace_root: task.workspace_root.clone(),
        prompt,
        model: resolved.model.clone(),
        session_dir: Some(ensure_session_storage_dir(task.id)?),
        execution_mode: task.execution_mode.clone(),
        backend_config,
    })?)
}

pub fn prepare_resume(task: &TaskRecord, message: &str) -> Result<dispatch_backends::ResumePlan> {
    let session = task
        .session
        .clone()
        .context("task has no resumable session reference")?;
    let prompt = build_worker_prompt(task, &task.prompt, Some(message));
    let config = ensure_config()?;
    let backend_key = backend_name_for_kind(&task.backend).to_string();
    let backend_config = config.backends.get(&backend_key).cloned();
    Ok(backend_for(&task.backend).resume_plan(&ResumeSpec {
        session,
        prompt,
        model: task.model.clone(),
        execution_mode: task.execution_mode.clone(),
        backend_config,
    })?)
}

pub fn spawn_background_execution(store: &DispatchStore, task_id: Uuid) -> Result<()> {
    let current_exe = env::current_exe().context("resolve current executable")?;
    let mut child = Command::new(current_exe);
    child
        .arg("--root")
        .arg(store.root())
        .arg("execute")
        .arg(task_id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        child.process_group(0);
    }

    child.spawn().context("spawn background execution")?;
    Ok(())
}

pub fn dispatch_start(
    store: &DispatchStore,
    title: String,
    input: DispatchInput,
    workspace: PathBuf,
    execution_mode: ExecutionMode,
    requested_mode: RequestedTaskMode,
    backend_override: Option<BackendKind>,
    model_override: Option<String>,
    foreground: bool,
) -> Result<DispatchSummary> {
    store.init()?;
    let config = ensure_config()?;
    let resolved = resolve_target(&config, backend_override, model_override)?;
    let task_mode = resolve_task_mode(requested_mode, &input);
    let task = create_task_from_request(
        store,
        title,
        input,
        workspace,
        execution_mode,
        task_mode.clone(),
        &resolved,
    )?;

    if matches!(task_mode, TaskMode::Discuss) {
        let draft = build_discussion_draft(&task);
        fs::write(&task.artifacts.output_file, draft).with_context(|| {
            format!(
                "write discussion draft to {}",
                task.artifacts.output_file.display()
            )
        })?;
        store.update_task(task.id, |record| {
            record.status = TaskStatus::AwaitingUser;
        })?;
        store.append_event(
            task.id,
            EventKind::WaitingForUser,
            "discussion draft created",
        )?;
        return Ok(DispatchSummary {
            task_id: task.id,
            status: "awaiting_user",
        });
    }

    let plan = prepare_start(&task, &resolved)?;
    let session_hint = plan.session_hint.clone();

    store.update_task(task.id, |record| {
        record.status = TaskStatus::Running;
        record.session = session_hint.clone();
        record.checkpoint.last_invocation = Some(plan.invocation.clone());
        record.checkpoint.session_capture = plan.session_capture.clone();
    })?;
    store.append_event(
        task.id,
        EventKind::SessionPrepared,
        "worker invocation prepared",
    )?;

    if foreground {
        crate::executor::execute_plan(store, task.id, &plan.invocation, &plan.session_capture)?;
        Ok(DispatchSummary {
            task_id: task.id,
            status: "completed",
        })
    } else {
        spawn_background_execution(store, task.id)?;
        Ok(DispatchSummary {
            task_id: task.id,
            status: "dispatched",
        })
    }
}

pub fn dispatch_resume(
    store: &DispatchStore,
    task_id: Uuid,
    message: String,
    execution_mode_override: Option<ExecutionMode>,
    foreground: bool,
) -> Result<DispatchSummary> {
    let _task = store.load_task(task_id)?;
    if let Some(mode) = execution_mode_override {
        store.update_task(task_id, |record| {
            record.execution_mode = mode;
        })?;
    }
    let refreshed = store.load_task(task_id)?;
    let plan = prepare_resume(&refreshed, &message)?;

    store.update_task(task_id, |record| {
        record.status = TaskStatus::Running;
        record.session = Some(plan.session.clone());
        record.checkpoint.restart_count += 1;
        record.checkpoint.last_invocation = Some(plan.invocation.clone());
    })?;
    store.append_event(
        task_id,
        EventKind::SessionResumed,
        "session resume prepared",
    )?;

    if foreground {
        crate::executor::execute_plan(
            store,
            task_id,
            &plan.invocation,
            &dispatch_core::SessionCaptureStrategy::Preallocated(plan.session),
        )?;
        Ok(DispatchSummary {
            task_id,
            status: "completed",
        })
    } else {
        spawn_background_execution(store, task_id)?;
        Ok(DispatchSummary {
            task_id,
            status: "dispatched",
        })
    }
}

struct ResolvedModelConfig {
    backend: BackendKind,
    model: Option<String>,
}

fn resolve_model(config: &DispatchConfig, model_name: &str) -> Result<ResolvedModelConfig> {
    let model = config
        .models
        .get(model_name)
        .with_context(|| format!("model `{model_name}` is not defined in config"))?;
    Ok(ResolvedModelConfig {
        backend: parse_backend_name(&model.backend)?,
        model: model
            .model
            .clone()
            .or_else(|| (!model_name.ends_with("-default")).then(|| model_name.to_string())),
    })
}

fn model_backend_key(config: &DispatchConfig, model_name: &str) -> Result<String> {
    config
        .models
        .get(model_name)
        .map(|model| model.backend.clone())
        .with_context(|| format!("model `{model_name}` is not defined in config"))
}

fn alias_backend_key(config: &DispatchConfig, model_name: &str) -> Result<String> {
    model_backend_key(config, model_name)
}

fn parse_backend_name(value: &str) -> Result<BackendKind> {
    match value {
        "codex" => Ok(BackendKind::Codex),
        "claude" | "claude-code" => Ok(BackendKind::ClaudeCode),
        "pi" => Ok(BackendKind::Pi),
        "cursor" | "cursor-agent" | "agent" => Ok(BackendKind::CursorAgent),
        other => bail!("unknown backend `{other}`"),
    }
}

fn backend_name_for_kind(kind: &BackendKind) -> &'static str {
    match kind {
        BackendKind::Codex => "codex",
        BackendKind::ClaudeCode => "claude",
        BackendKind::Pi => "pi",
        BackendKind::CursorAgent => "cursor-agent",
        BackendKind::Generic => "generic",
    }
}

pub(crate) fn session_storage_dir_for_task(task_id: Uuid) -> PathBuf {
    DispatchConfig::session_storage_root().join(task_id.to_string())
}

fn ensure_session_storage_dir(task_id: Uuid) -> Result<PathBuf> {
    let path = session_storage_dir_for_task(task_id);
    fs::create_dir_all(&path)
        .with_context(|| format!("create session storage directory {}", path.display()))?;
    Ok(path)
}

pub fn load_dispatch_input(
    path: PathBuf,
    requested_mode: RequestedTaskMode,
) -> Result<DispatchInput> {
    let body =
        fs::read_to_string(&path).with_context(|| format!("read input file {}", path.display()))?;
    let inferred_plan = matches!(requested_mode, RequestedTaskMode::Plan)
        || (matches!(requested_mode, RequestedTaskMode::Auto)
            && body.lines().any(is_checklist_line));

    if inferred_plan {
        let prompt = extract_goal_from_plan(&body)
            .unwrap_or_else(|| format!("Execute plan from {}", path.display()));
        return Ok(DispatchInput::PlanFile {
            _path: path,
            prompt,
            plan_body: body,
        });
    }

    Ok(DispatchInput::PromptFile {
        _path: path,
        prompt: body,
    })
}

pub fn generate_template(kind: TemplateKind) -> String {
    match kind {
        TemplateKind::Generic => generic_template("Task Title"),
        TemplateKind::Feature => generic_template("Feature Implementation"),
        TemplateKind::Bugfix => generic_template("Bug Fix"),
        TemplateKind::Refactor => generic_template("Refactor"),
        TemplateKind::Audit => generic_template("Audit"),
        TemplateKind::Research => generic_template("Research Task"),
    }
}

fn generic_template(title: &str) -> String {
    format!(
        "# {title}\n\n## Goal\n\n- \n\n## Constraints\n\n- \n\n## References\n\n- \n\n## Plan\n\n- [ ] \n- [ ] \n- [ ] Write summary to `.dispatch/tasks/<task-id>/output.md`\n"
    )
}

fn build_discussion_draft(task: &TaskRecord) -> String {
    format!(
        "# Discussion Draft\n\n## Requested Outcome\n\n{}\n\n## Open Questions\n\n- What outcome should be considered complete?\n- What constraints or non-goals matter?\n- Should this be handled as direct execution or a plan-driven task?\n\n## Suggested Next Step\n\n- If the work is already clear, re-run dispatch with `--mode direct` or `--mode plan`.\n- If you want to author the task yourself, generate a template and fill in `plan.md`.\n",
        task.prompt.trim()
    )
}

fn resolve_task_mode(requested_mode: RequestedTaskMode, input: &DispatchInput) -> TaskMode {
    match requested_mode {
        RequestedTaskMode::Direct => TaskMode::Direct,
        RequestedTaskMode::Plan => TaskMode::Plan,
        RequestedTaskMode::Discuss => TaskMode::Discuss,
        RequestedTaskMode::Auto => match input {
            DispatchInput::PlanFile { .. } => TaskMode::Plan,
            DispatchInput::PromptFile { .. } => TaskMode::Direct,
            DispatchInput::InlinePrompt { .. } => TaskMode::Direct,
        },
    }
}

fn extract_goal_from_plan(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("- ["))
        .map(ToOwned::to_owned)
}

fn is_checklist_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("- [ ] ")
        || trimmed.starts_with("- [>] ")
        || trimmed.starts_with("- [x] ")
        || trimmed.starts_with("- [?] ")
        || trimmed.starts_with("- [!] ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use dispatch_core::{BackendConfig, DispatchConfig, ModelConfig, TaskMode};

    use super::{
        DispatchInput, RequestedTaskMode, bootstrap_config, choose_default_target,
        generate_template, load_dispatch_input, resolve_target, resolve_task_mode,
    };

    #[test]
    fn auto_mode_prefers_direct_for_prompt_files_and_plan_for_plan_files() {
        let prompt_input = DispatchInput::PromptFile {
            _path: "prompt.md".into(),
            prompt: "do the work".into(),
        };
        let plan_input = DispatchInput::PlanFile {
            _path: "plan.md".into(),
            prompt: "execute plan".into(),
            plan_body: "- [ ] step".into(),
        };

        assert!(matches!(
            resolve_task_mode(RequestedTaskMode::Auto, &prompt_input),
            TaskMode::Direct
        ));
        assert!(matches!(
            resolve_task_mode(RequestedTaskMode::Auto, &plan_input),
            TaskMode::Plan
        ));
    }

    #[test]
    fn auto_mode_defaults_inline_prompts_to_direct_for_host_control() {
        let input = DispatchInput::InlinePrompt {
            prompt: "Help me decide whether this should be a direct task or a larger plan".into(),
        };
        assert!(matches!(
            resolve_task_mode(RequestedTaskMode::Auto, &input),
            TaskMode::Direct
        ));
    }

    #[test]
    fn load_dispatch_input_detects_plan_files_from_checklists() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("plan.md");
        fs::write(
            &path,
            "# Title\n\n## Goal\n\nShip it.\n\n## Plan\n\n- [ ] Step one\n- [ ] Step two\n",
        )
        .unwrap();

        let input = load_dispatch_input(path, RequestedTaskMode::Auto).unwrap();
        match input {
            DispatchInput::PlanFile { .. } => {}
            other => panic!("expected plan input, got {other:?}"),
        }
    }

    #[test]
    fn resolve_target_requires_explicit_model_mapping_without_backend_override() {
        let config = DispatchConfig {
            default: "missing".into(),
            backends: [(
                "pi".into(),
                BackendConfig {
                    executable: "pi".into(),
                    args: vec!["--session".into(), "{session_file}".into()],
                },
            )]
            .into_iter()
            .collect(),
            models: [(
                "pi-default".into(),
                ModelConfig {
                    backend: "pi".into(),
                    model: None,
                },
            )]
            .into_iter()
            .collect(),
            aliases: Default::default(),
        };

        let error = resolve_target(&config, None, Some("unknown-model".into())).unwrap_err();
        assert!(error.to_string().contains("unknown-model"));
    }

    #[test]
    fn template_generation_includes_plan_section() {
        let body = generate_template(super::TemplateKind::Audit);
        assert!(body.contains("# Audit"));
        assert!(body.contains("## Plan"));
    }

    #[test]
    fn bootstrap_config_uses_explicit_backend_and_model_scoping() {
        let config = bootstrap_config();
        if let Some(model) = config.models.get("pi-default") {
            assert_eq!(model.backend, "pi");
            assert!(model.model.is_none());
        }
        if let Some(backend) = config.backends.get("pi") {
            assert!(
                backend
                    .args
                    .iter()
                    .any(|arg| arg.contains("{session_file}"))
            );
        }
    }

    #[test]
    fn choose_default_target_prefers_pi_when_available() {
        let models = BTreeMap::from([
            (
                "pi-default".into(),
                ModelConfig {
                    backend: "pi".into(),
                    model: None,
                },
            ),
            (
                "gpt-5.4".into(),
                ModelConfig {
                    backend: "codex".into(),
                    model: Some("gpt-5.4".into()),
                },
            ),
            (
                "sonnet".into(),
                ModelConfig {
                    backend: "claude".into(),
                    model: Some("sonnet".into()),
                },
            ),
        ]);

        assert_eq!(choose_default_target(&models), "pi-default");
    }

    #[test]
    fn choose_default_target_prefers_codex_before_claude_when_pi_is_missing() {
        let models = BTreeMap::from([
            (
                "gpt-5.4".into(),
                ModelConfig {
                    backend: "codex".into(),
                    model: Some("gpt-5.4".into()),
                },
            ),
            (
                "sonnet".into(),
                ModelConfig {
                    backend: "claude".into(),
                    model: Some("sonnet".into()),
                },
            ),
        ]);

        assert_eq!(choose_default_target(&models), "gpt-5.4");
    }

    #[test]
    fn choose_default_target_falls_back_to_pi_when_it_is_all_we_have() {
        let models = BTreeMap::from([(
            "pi-default".into(),
            ModelConfig {
                backend: "pi".into(),
                model: None,
            },
        )]);

        assert_eq!(choose_default_target(&models), "pi-default");
    }
}
