mod executor;
mod prompt_builder;
mod runtime;

use std::path::PathBuf;
use std::process;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use dispatch_core::{
    DispatchConfig, DispatchStore, EventKind, ExecutionMode, PendingQuestion, TaskRecord,
    TaskStatus, list_pending_questions, write_answer_atomic,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "dispatch")]
#[command(about = "Persistent task dispatch for coding-agent CLIs")]
struct Cli {
    #[arg(long, global = true, default_value = ".dispatch")]
    root: PathBuf,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init {
        #[arg(long)]
        bootstrap_config: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Template {
        #[arg(long, value_enum, default_value = "generic")]
        kind: TemplateKindArg,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Ready,
    Backends,
    Run {
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        from: Option<PathBuf>,
        #[arg(
            long = "mode",
            visible_alias = "task-mode",
            value_enum,
            default_value = "auto"
        )]
        mode: TaskModeArg,
        #[arg(long, value_enum)]
        backend: Option<BackendArg>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum, default_value = "auto")]
        execution_mode: ExecutionModeArg,
        #[arg(long)]
        foreground: bool,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
    },
    List,
    Inspect {
        task_id: Uuid,
        #[arg(long, default_value_t = 10)]
        event_limit: usize,
    },
    Status {
        task_id: Uuid,
    },
    Resume {
        task_id: Uuid,
        #[arg(long)]
        message: String,
        #[arg(long, value_enum)]
        execution_mode: Option<ExecutionModeArg>,
        #[arg(long)]
        foreground: bool,
    },
    Events {
        task_id: Uuid,
    },
    Questions {
        task_id: Option<Uuid>,
    },
    Answer {
        task_id: Uuid,
        #[arg(long)]
        message: String,
    },
    #[command(hide = true)]
    Execute {
        task_id: Uuid,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    Show,
    Bootstrap,
    SetDefault {
        value: String,
    },
    AddBackend {
        name: String,
        executable: String,
        #[arg(long = "arg")]
        args: Vec<String>,
    },
    RemoveBackend {
        name: String,
    },
    AddModel {
        name: String,
        #[arg(long)]
        backend: String,
        #[arg(long = "scoped-model")]
        scoped_model: Option<String>,
    },
    RemoveModel {
        name: String,
    },
    AddAlias {
        name: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        prompt: Option<String>,
    },
    RemoveAlias {
        name: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum BackendArg {
    Codex,
    Claude,
    Pi,
    CursorAgent,
}

#[derive(Debug, Clone, ValueEnum)]
enum TaskModeArg {
    Auto,
    Direct,
    Plan,
    Discuss,
}

#[derive(Debug, Clone, ValueEnum)]
enum TemplateKindArg {
    Generic,
    Feature,
    Bugfix,
    Refactor,
    Audit,
    Research,
}

#[derive(Debug, Clone, ValueEnum)]
enum ExecutionModeArg {
    Standard,
    Auto,
    Danger,
}

impl From<ExecutionModeArg> for ExecutionMode {
    fn from(value: ExecutionModeArg) -> Self {
        match value {
            ExecutionModeArg::Standard => ExecutionMode::Standard,
            ExecutionModeArg::Auto => ExecutionMode::Auto,
            ExecutionModeArg::Danger => ExecutionMode::Danger,
        }
    }
}

impl From<TaskModeArg> for runtime::RequestedTaskMode {
    fn from(value: TaskModeArg) -> Self {
        match value {
            TaskModeArg::Auto => runtime::RequestedTaskMode::Auto,
            TaskModeArg::Direct => runtime::RequestedTaskMode::Direct,
            TaskModeArg::Plan => runtime::RequestedTaskMode::Plan,
            TaskModeArg::Discuss => runtime::RequestedTaskMode::Discuss,
        }
    }
}

impl From<TemplateKindArg> for runtime::TemplateKind {
    fn from(value: TemplateKindArg) -> Self {
        match value {
            TemplateKindArg::Generic => runtime::TemplateKind::Generic,
            TemplateKindArg::Feature => runtime::TemplateKind::Feature,
            TemplateKindArg::Bugfix => runtime::TemplateKind::Bugfix,
            TemplateKindArg::Refactor => runtime::TemplateKind::Refactor,
            TemplateKindArg::Audit => runtime::TemplateKind::Audit,
            TemplateKindArg::Research => runtime::TemplateKind::Research,
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonEnvelope<T> {
    ok: bool,
    data: T,
}

#[derive(Debug, Serialize)]
struct JsonErrorEnvelope {
    ok: bool,
    error: JsonError,
}

#[derive(Debug, Serialize)]
struct JsonError {
    message: String,
}

#[derive(Debug, Serialize)]
struct BackendSummary {
    kind: String,
    executable: String,
    installed: bool,
    notes: String,
    capabilities: dispatch_backends::BackendCapabilities,
}

#[derive(Debug, Serialize)]
struct TaskListItem {
    task_id: Uuid,
    title: String,
    status: TaskStatus,
    backend: String,
    model: Option<String>,
    updated_at: String,
    pending_question_count: usize,
}

#[derive(Debug, Serialize)]
struct QuestionsSummary {
    task_id: Uuid,
    task_title: String,
    status: TaskStatus,
    questions: Vec<PendingQuestion>,
}

#[derive(Debug, Serialize)]
struct InspectSummary {
    task: TaskRecord,
    pending_questions: Vec<PendingQuestion>,
    recent_events: Vec<dispatch_core::EventRecord>,
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(error) = run(cli) {
        if json {
            let payload = JsonErrorEnvelope {
                ok: false,
                error: JsonError {
                    message: error.to_string(),
                },
            };
            let _ = println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
                    "{\"ok\":false,\"error\":{\"message\":\"serialization error\"}}".into()
                })
            );
        } else {
            eprintln!("{error:#}");
        }
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let store = DispatchStore::new(cli.root);

    match cli.command {
        Commands::Init { bootstrap_config } => {
            store.init()?;
            if bootstrap_config {
                let config = runtime::bootstrap_config();
                config.save()?;
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: serde_json::json!({
                            "root": store.root(),
                            "config_path": DispatchConfig::config_path(),
                            "bootstrapped_config": true,
                        }),
                    })?;
                } else {
                    println!(
                        "initialized {} and bootstrapped {}",
                        store.root().display(),
                        DispatchConfig::config_path().display()
                    );
                }
            } else {
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: serde_json::json!({
                            "root": store.root(),
                            "bootstrapped_config": false,
                        }),
                    })?;
                } else {
                    println!("initialized {}", store.root().display());
                }
            }
        }
        Commands::Config { command } => match command {
            ConfigCommands::Show => {
                let config = runtime::ensure_config()?;
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &config,
                    })?;
                } else {
                    println!("{}", serde_yaml::to_string(&config)?);
                }
            }
            ConfigCommands::Bootstrap => {
                let config = runtime::bootstrap_config();
                config.save()?;
                let payload = serde_json::json!({
                    "config_path": DispatchConfig::config_path(),
                    "default_target": config.default,
                });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!("bootstrapped {}", DispatchConfig::config_path().display());
                }
            }
            ConfigCommands::SetDefault { value } => {
                let mut config = runtime::ensure_config()?;
                config.set_default(value.clone());
                config.save()?;
                let payload = serde_json::json!({ "default": value });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!("default set to {}", payload["default"].as_str().unwrap());
                }
            }
            ConfigCommands::AddBackend {
                name,
                executable,
                args,
            } => {
                let mut config = runtime::ensure_config()?;
                config.upsert_backend(name.clone(), executable, args);
                config.save()?;
                let payload = serde_json::json!({ "backend": name });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!("backend {} saved", payload["backend"].as_str().unwrap());
                }
            }
            ConfigCommands::RemoveBackend { name } => {
                let mut config = runtime::ensure_config()?;
                let removed = config.remove_backend(&name).is_some();
                config.save()?;
                let payload = serde_json::json!({ "backend": name, "removed": removed });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!(
                        "{} {}",
                        if removed {
                            "removed backend"
                        } else {
                            "backend not found"
                        },
                        payload["backend"].as_str().unwrap()
                    );
                }
            }
            ConfigCommands::AddModel {
                name,
                backend,
                scoped_model,
            } => {
                let mut config = runtime::ensure_config()?;
                config.upsert_model(name.clone(), backend.clone(), scoped_model.clone());
                config.save()?;
                let payload = serde_json::json!({
                    "model": name,
                    "backend": backend,
                    "scoped_model": scoped_model,
                });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!(
                        "model {} saved -> backend={} scoped_model={}",
                        payload["model"].as_str().unwrap(),
                        payload["backend"].as_str().unwrap(),
                        payload["scoped_model"].as_str().unwrap_or("null")
                    );
                }
            }
            ConfigCommands::RemoveModel { name } => {
                let mut config = runtime::ensure_config()?;
                let removed = config.remove_model(&name).is_some();
                config.save()?;
                let payload = serde_json::json!({ "model": name, "removed": removed });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!(
                        "{} {}",
                        if removed {
                            "removed model"
                        } else {
                            "model not found"
                        },
                        payload["model"].as_str().unwrap()
                    );
                }
            }
            ConfigCommands::AddAlias {
                name,
                model,
                prompt,
            } => {
                let mut config = runtime::ensure_config()?;
                config.upsert_alias(name.clone(), model.clone(), prompt);
                config.save()?;
                let payload = serde_json::json!({ "alias": name, "model": model });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!(
                        "alias {} -> {} saved",
                        payload["alias"].as_str().unwrap(),
                        payload["model"].as_str().unwrap()
                    );
                }
            }
            ConfigCommands::RemoveAlias { name } => {
                let mut config = runtime::ensure_config()?;
                let removed = config.remove_alias(&name).is_some();
                config.save()?;
                let payload = serde_json::json!({ "alias": name, "removed": removed });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!(
                        "{} {}",
                        if removed {
                            "removed alias"
                        } else {
                            "alias not found"
                        },
                        payload["alias"].as_str().unwrap()
                    );
                }
            }
        },
        Commands::Template { kind, output } => {
            let body = runtime::generate_template(kind.into());
            if let Some(output) = output {
                std::fs::write(&output, body)?;
                let payload = serde_json::json!({ "output_path": output, "written": true });
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: &payload,
                    })?;
                } else {
                    println!("{}", payload["output_path"].as_str().unwrap());
                }
            } else {
                if cli.json {
                    print_json(&JsonEnvelope {
                        ok: true,
                        data: serde_json::json!({ "body": body }),
                    })?;
                } else {
                    println!("{body}");
                }
            }
        }
        Commands::Ready => {
            let summary = runtime::readiness_summary()?;
            print_payload(cli.json, &summary, &summary)?;
        }
        Commands::Backends => {
            let mut summaries = Vec::new();
            for backend in dispatch_backends::all_backends() {
                let availability = backend.detect();
                let capabilities = backend.capabilities();
                summaries.push(BackendSummary {
                    kind: availability.kind.as_str().into(),
                    executable: availability.executable,
                    installed: availability.installed,
                    notes: availability.notes,
                    capabilities,
                });
            }
            if cli.json {
                print_json(&JsonEnvelope {
                    ok: true,
                    data: &summaries,
                })?;
            } else {
                for summary in summaries {
                    println!(
                        "{} installed={} native_sessions={} resumable={} forkable={} auto={} danger={} notes={}",
                        summary.kind,
                        summary.installed,
                        summary.capabilities.native_sessions,
                        summary.capabilities.resumable,
                        summary.capabilities.forkable,
                        summary.capabilities.supports_auto_mode,
                        summary.capabilities.supports_danger_mode,
                        summary.notes
                    );
                }
            }
        }
        Commands::Run {
            title,
            prompt,
            from,
            mode,
            backend,
            model,
            execution_mode,
            foreground,
            workspace,
        } => {
            let input = match (prompt, from) {
                (Some(prompt), None) => runtime::DispatchInput::InlinePrompt { prompt },
                (None, Some(path)) => runtime::load_dispatch_input(path, mode.clone().into())?,
                (Some(_), Some(_)) => anyhow::bail!("use either --prompt or --from, not both"),
                (None, None) => anyhow::bail!("one of --prompt or --from is required"),
            };
            let title = title.unwrap_or_else(|| match &input {
                runtime::DispatchInput::InlinePrompt { prompt } => derive_title(prompt),
                runtime::DispatchInput::PromptFile { prompt, .. } => derive_title(prompt),
                runtime::DispatchInput::PlanFile { prompt, .. } => derive_title(prompt),
            });
            let summary = runtime::dispatch_start(
                &store,
                title,
                input,
                workspace,
                execution_mode.into(),
                mode.into(),
                backend.map(map_backend),
                model,
                foreground,
            )?;
            let payload = serde_json::json!({
                "task_id": summary.task_id,
                "status": summary.status,
            });
            print_payload(cli.json, &payload, &payload)?;
        }
        Commands::List => {
            let mut tasks = store
                .list_task_ids()?
                .into_iter()
                .map(|task_id| {
                    let task = store.load_task(task_id)?;
                    let pending = list_pending_questions(&task.artifacts.mailbox_dir)?;
                    Ok(TaskListItem {
                        task_id: task.id,
                        title: task.title,
                        status: task.status,
                        backend: task.backend.as_str().into(),
                        model: task.model,
                        updated_at: task.updated_at.to_rfc3339(),
                        pending_question_count: pending.len(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            print_payload(cli.json, &tasks, &tasks)?;
        }
        Commands::Inspect {
            task_id,
            event_limit,
        } => {
            let task = store.load_task(task_id)?;
            let pending_questions = list_pending_questions(&task.artifacts.mailbox_dir)?;
            let mut recent_events = store.read_events(task_id)?;
            if recent_events.len() > event_limit {
                recent_events = recent_events
                    .into_iter()
                    .rev()
                    .take(event_limit)
                    .collect::<Vec<_>>();
                recent_events.reverse();
            }
            let payload = InspectSummary {
                task,
                pending_questions,
                recent_events,
            };
            print_payload(cli.json, &payload, &payload)?;
        }
        Commands::Status { task_id } => {
            let task = store.load_task(task_id)?;
            print_payload(cli.json, &task, &task)?;
        }
        Commands::Resume {
            task_id,
            message,
            execution_mode,
            foreground,
        } => {
            let summary = runtime::dispatch_resume(
                &store,
                task_id,
                message,
                execution_mode.map(Into::into),
                foreground,
            )?;
            let payload = serde_json::json!({
                "task_id": summary.task_id,
                "status": summary.status,
            });
            print_payload(cli.json, &payload, &payload)?;
        }
        Commands::Events { task_id } => {
            let events = store.read_events(task_id)?;
            print_payload(cli.json, &events, &events)?;
        }
        Commands::Questions { task_id } => {
            let mut all_questions = Vec::new();
            let mut legacy_questions = Vec::new();
            if let Some(task_id) = task_id {
                let task = store.load_task(task_id)?;
                let questions = list_pending_questions(&task.artifacts.mailbox_dir)?;
                all_questions.push(QuestionsSummary {
                    task_id,
                    task_title: task.title,
                    status: task.status,
                    questions: questions.clone(),
                });
                legacy_questions.push((task_id, questions));
            } else {
                for task_id in store.list_task_ids()? {
                    let task = store.load_task(task_id)?;
                    let pending = list_pending_questions(&task.artifacts.mailbox_dir)?;
                    if !pending.is_empty() {
                        all_questions.push(QuestionsSummary {
                            task_id,
                            task_title: task.title,
                            status: task.status,
                            questions: pending.clone(),
                        });
                        legacy_questions.push((task_id, pending));
                    }
                }
            }
            if cli.json {
                print_json(&JsonEnvelope {
                    ok: true,
                    data: &all_questions,
                })?;
            } else {
                print_json(&legacy_questions)?;
            }
        }
        Commands::Answer { task_id, message } => {
            let task = store.load_task(task_id)?;
            let pending = list_pending_questions(&task.artifacts.mailbox_dir)?;
            let question = pending
                .first()
                .ok_or_else(|| anyhow::anyhow!("no unanswered mailbox question found for task"))?;
            write_answer_atomic(&question.answer_path, &message)?;
            store.set_status(task_id, TaskStatus::Running, "answer written to mailbox")?;
            store.append_event(
                task_id,
                EventKind::UserAnswered,
                format!("answered question {}", question.sequence),
            )?;
            let payload = serde_json::json!({
                "task_id": task_id,
                "answered": question.sequence,
                "answer_path": question.answer_path,
            });
            print_payload(cli.json, &payload, &payload)?;
        }
        Commands::Execute { task_id } => {
            let task = store.load_task(task_id)?;
            let invocation = task
                .checkpoint
                .last_invocation
                .clone()
                .ok_or_else(|| anyhow::anyhow!("task has no saved invocation"))?;
            let summary = executor::execute_plan(
                &store,
                task_id,
                &invocation,
                &task.checkpoint.session_capture,
            )?;
            print_payload(cli.json, &summary, &summary)?;
        }
    }

    Ok(())
}

fn print_json<T: Serialize>(data: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(data)?);
    Ok(())
}

fn print_payload<TText: Serialize, TJson: Serialize>(
    as_json_envelope: bool,
    text_payload: &TText,
    json_payload: &TJson,
) -> Result<()> {
    if as_json_envelope {
        let envelope = JsonEnvelope {
            ok: true,
            data: json_payload,
        };
        print_json(&envelope)
    } else {
        print_json(text_payload)
    }
}

fn derive_title(prompt: &str) -> String {
    let compact = prompt.replace('\n', " ").trim().to_string();
    if compact.len() <= 72 {
        compact
    } else {
        format!("{}...", &compact[..69])
    }
}

fn map_backend(value: BackendArg) -> dispatch_core::BackendKind {
    match value {
        BackendArg::Codex => dispatch_core::BackendKind::Codex,
        BackendArg::Claude => dispatch_core::BackendKind::ClaudeCode,
        BackendArg::Pi => dispatch_core::BackendKind::Pi,
        BackendArg::CursorAgent => dispatch_core::BackendKind::CursorAgent,
    }
}
