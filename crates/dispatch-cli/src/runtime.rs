use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use dispatch_backends::{ResumeSpec, StartSpec, backend_for};
use dispatch_core::{
    BackendConfig, BackendKind, DispatchConfig, DispatchStore, EventKind, ExecutionMode,
    ModelConfig, PlanStep, StepStatus, TaskDraft, TaskMode, TaskRecord, TaskSource, TaskStatus,
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
        steps: Vec<PlanStep>,
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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub enum RouteKind {
    Warmup,
    ConfigRequest,
    TaskRequest,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct RouteSummary {
    pub kind: RouteKind,
    pub suggested_mode: Option<String>,
    pub suggested_cli_args: Option<Vec<String>>,
    pub reason: String,
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

pub fn route_request(prompt: &str) -> RouteSummary {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return RouteSummary {
            kind: RouteKind::Warmup,
            suggested_mode: None,
            suggested_cli_args: Some(vec!["ready".into()]),
            reason: "empty request; load config and report readiness".into(),
        };
    }

    if let Some(args) = parse_config_request(trimmed) {
        return RouteSummary {
            kind: RouteKind::ConfigRequest,
            suggested_mode: None,
            suggested_cli_args: Some(args),
            reason: "request matched an explicit config operation".into(),
        };
    }

    let suggested_mode = suggest_mode_for_prompt(trimmed);
    RouteSummary {
        kind: RouteKind::TaskRequest,
        suggested_mode: Some(task_mode_name(&suggested_mode).into()),
        suggested_cli_args: None,
        reason: match suggested_mode {
            TaskMode::Discuss => {
                "request appears exploratory or asks for clarification before execution".into()
            }
            TaskMode::Direct => {
                "request appears concrete and small enough for direct execution".into()
            }
            TaskMode::Plan => {
                "request appears substantial enough to benefit from a persisted plan".into()
            }
        },
    }
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

    let default = if models.contains_key("pi-default") {
        "pi-default".into()
    } else if models.contains_key("sonnet") {
        "sonnet".into()
    } else if let Some(first) = models.keys().next() {
        first.clone()
    } else {
        "pi-default".into()
    };

    DispatchConfig {
        default,
        backends,
        models,
        aliases,
    }
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
    let (prompt, task_source, preserve_plan_file, plan_body, plan) = match input {
        DispatchInput::InlinePrompt { prompt } => {
            let plan = derive_plan(task_mode.clone(), &prompt);
            (prompt, TaskSource::InlinePrompt, false, None, plan)
        }
        DispatchInput::PromptFile { _path: _, prompt } => {
            let plan = derive_plan(task_mode.clone(), &prompt);
            (prompt, TaskSource::PromptFile, false, None, plan)
        }
        DispatchInput::PlanFile {
            _path: _,
            prompt,
            plan_body,
            steps,
        } => (prompt, TaskSource::PlanFile, true, Some(plan_body), steps),
    };
    Ok(store.create_task(TaskDraft {
        title,
        prompt,
        task_mode,
        task_source,
        backend: resolved.backend.clone(),
        model: resolved.model.clone(),
        execution_mode,
        preserve_plan_file,
        plan_body,
        workspace_root: workspace,
        plan,
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
        session_dir: task.artifacts.sessions_dir.clone(),
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
            if let Some(step) = record.plan.get_mut(0) {
                step.status = StepStatus::Blocked;
                step.notes.push("waiting for user clarification".into());
            }
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
        if let Some(step) = record.plan.get_mut(1) {
            step.status = StepStatus::Running;
        }
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
        if let Some(step) = record.plan.get_mut(2) {
            step.status = StepStatus::Running;
        }
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

fn derive_plan(task_mode: TaskMode, prompt: &str) -> Vec<PlanStep> {
    let compact = prompt.trim();
    match task_mode {
        TaskMode::Direct => vec![
            PlanStep {
                id: "execute".into(),
                title: format!(
                    "Execute the requested work directly: {}",
                    truncate(compact, 72)
                ),
                status: StepStatus::Pending,
                notes: vec![],
            },
            PlanStep {
                id: "summarize".into(),
                title: "Write any requested result or summary to the task output artifact".into(),
                status: StepStatus::Pending,
                notes: vec![],
            },
        ],
        TaskMode::Plan | TaskMode::Discuss => vec![
            PlanStep {
                id: "clarify".into(),
                title: format!("Clarify the request details for: {}", truncate(compact, 72)),
                status: StepStatus::Pending,
                notes: vec![],
            },
            PlanStep {
                id: "plan".into(),
                title: format!(
                    "Propose the execution approach for: {}",
                    truncate(compact, 72)
                ),
                status: StepStatus::Pending,
                notes: vec![],
            },
            PlanStep {
                id: "next".into(),
                title: "Capture next actions in the task output artifact".into(),
                status: StepStatus::Pending,
                notes: vec![],
            },
        ],
    }
}

fn truncate(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        input.to_string()
    } else {
        format!("{}...", &input[..max_len.saturating_sub(3)])
    }
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
        let steps = parse_plan_steps(&body)?;
        let prompt = extract_goal_from_plan(&body)
            .unwrap_or_else(|| format!("Execute plan from {}", path.display()));
        return Ok(DispatchInput::PlanFile {
            _path: path,
            prompt,
            plan_body: body,
            steps,
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
            DispatchInput::InlinePrompt { prompt } => suggest_mode_for_prompt(prompt),
        },
    }
}

fn suggest_mode_for_prompt(prompt: &str) -> TaskMode {
    let lower = prompt.trim().to_lowercase();
    let discuss_markers = [
        "help me decide",
        "let's discuss",
        "lets discuss",
        "should we",
        "should i",
        "how should",
        "brainstorm",
        "think through",
        "explore options",
        "not sure",
        "unclear",
    ];
    if discuss_markers.iter().any(|marker| lower.contains(marker)) {
        return TaskMode::Discuss;
    }

    let direct_verbs = [
        "fix",
        "add",
        "update",
        "write",
        "rename",
        "remove",
        "change",
        "translate",
        "summarize",
    ];
    let word_count = lower.split_whitespace().count();
    let sentence_count = prompt.matches('.').count() + prompt.matches('\n').count();
    if word_count <= 18
        && sentence_count <= 1
        && direct_verbs.iter().any(|verb| lower.starts_with(verb))
    {
        return TaskMode::Direct;
    }

    TaskMode::Plan
}

fn parse_config_request(prompt: &str) -> Option<Vec<String>> {
    let trimmed = prompt.trim();
    let lower = trimmed.to_lowercase();

    if matches!(lower.as_str(), "show config" | "config show") {
        return Some(vec!["config".into(), "show".into()]);
    }
    if matches!(lower.as_str(), "bootstrap config" | "config bootstrap") {
        return Some(vec!["config".into(), "bootstrap".into()]);
    }
    if let Some(value) = lower
        .strip_prefix("set default to ")
        .or_else(|| lower.strip_prefix("use default "))
    {
        let original = trimmed[trimmed.len() - value.len()..].trim().to_string();
        return Some(vec!["config".into(), "set-default".into(), original]);
    }
    if let Some(name) = lower
        .strip_prefix("remove model ")
        .or_else(|| lower.strip_prefix("delete model "))
    {
        return Some(vec![
            "config".into(),
            "remove-model".into(),
            name.trim().into(),
        ]);
    }
    if let Some(name) = lower
        .strip_prefix("remove alias ")
        .or_else(|| lower.strip_prefix("delete alias "))
    {
        return Some(vec![
            "config".into(),
            "remove-alias".into(),
            name.trim().into(),
        ]);
    }
    if let Some(name) = lower
        .strip_prefix("remove backend ")
        .or_else(|| lower.strip_prefix("delete backend "))
    {
        return Some(vec![
            "config".into(),
            "remove-backend".into(),
            name.trim().into(),
        ]);
    }

    None
}

fn task_mode_name(mode: &TaskMode) -> &'static str {
    match mode {
        TaskMode::Direct => "direct",
        TaskMode::Plan => "plan",
        TaskMode::Discuss => "discuss",
    }
}

fn parse_plan_steps(body: &str) -> Result<Vec<PlanStep>> {
    let mut steps = Vec::new();
    for (index, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            steps.push(plan_step_from_line(index, rest, StepStatus::Pending));
        } else if let Some(rest) = trimmed.strip_prefix("- [>] ") {
            steps.push(plan_step_from_line(index, rest, StepStatus::Running));
        } else if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            steps.push(plan_step_from_line(index, rest, StepStatus::Done));
        } else if let Some(rest) = trimmed.strip_prefix("- [?] ") {
            steps.push(plan_step_from_line(index, rest, StepStatus::Blocked));
        } else if let Some(rest) = trimmed.strip_prefix("- [!] ") {
            steps.push(plan_step_from_line(index, rest, StepStatus::Failed));
        }
    }

    if steps.is_empty() {
        bail!("plan file contains no checklist items");
    }
    Ok(steps)
}

fn plan_step_from_line(index: usize, text: &str, status: StepStatus) -> PlanStep {
    PlanStep {
        id: format!("step-{:03}", index + 1),
        title: text.trim().to_string(),
        status,
        notes: vec![],
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
    use std::fs;

    use dispatch_core::{BackendConfig, DispatchConfig, ModelConfig, TaskMode};

    use super::{
        DispatchInput, RequestedTaskMode, RouteKind, bootstrap_config, generate_template,
        load_dispatch_input, resolve_target, resolve_task_mode, route_request,
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
            steps: vec![],
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
    fn auto_mode_uses_discuss_for_exploratory_inline_prompts() {
        let input = DispatchInput::InlinePrompt {
            prompt: "Help me decide whether this should be a direct task or a larger plan".into(),
        };
        assert!(matches!(
            resolve_task_mode(RequestedTaskMode::Auto, &input),
            TaskMode::Discuss
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
            DispatchInput::PlanFile { steps, .. } => assert_eq!(steps.len(), 2),
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
    }

    #[test]
    fn route_request_classifies_warmup_config_and_task_requests() {
        let warmup = route_request("");
        assert_eq!(warmup.kind, RouteKind::Warmup);

        let config = route_request("set default to sonnet");
        assert_eq!(config.kind, RouteKind::ConfigRequest);
        assert_eq!(
            config.suggested_cli_args,
            Some(vec!["config".into(), "set-default".into(), "sonnet".into()])
        );

        let task = route_request("fix the README typo");
        assert_eq!(task.kind, RouteKind::TaskRequest);
        assert_eq!(task.suggested_mode.as_deref(), Some("direct"));
    }
}
