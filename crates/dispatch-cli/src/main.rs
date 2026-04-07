mod executor;
mod prompt_builder;
mod runtime;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use dispatch_core::{
    DispatchConfig, DispatchStore, EventKind, ExecutionMode, TaskStatus, list_pending_questions,
    write_answer_atomic,
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "dispatch")]
#[command(about = "Persistent task dispatch for coding-agent CLIs")]
struct Cli {
    #[arg(long, global = true, default_value = ".dispatch")]
    root: PathBuf,
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
    Route {
        #[arg(long)]
        prompt: String,
    },
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = DispatchStore::new(cli.root);

    match cli.command {
        Commands::Init { bootstrap_config } => {
            store.init()?;
            if bootstrap_config {
                let config = runtime::bootstrap_config();
                config.save()?;
                println!(
                    "initialized {} and bootstrapped {}",
                    store.root().display(),
                    DispatchConfig::config_path().display()
                );
            } else {
                println!("initialized {}", store.root().display());
            }
        }
        Commands::Config { command } => match command {
            ConfigCommands::Show => {
                let config = runtime::ensure_config()?;
                println!("{}", serde_yaml::to_string(&config)?);
            }
            ConfigCommands::Bootstrap => {
                let config = runtime::bootstrap_config();
                config.save()?;
                println!("bootstrapped {}", DispatchConfig::config_path().display());
            }
            ConfigCommands::SetDefault { value } => {
                let mut config = runtime::ensure_config()?;
                config.set_default(value.clone());
                config.save()?;
                println!("default set to {}", value);
            }
            ConfigCommands::AddBackend {
                name,
                executable,
                args,
            } => {
                let mut config = runtime::ensure_config()?;
                config.upsert_backend(name.clone(), executable, args);
                config.save()?;
                println!("backend {} saved", name);
            }
            ConfigCommands::RemoveBackend { name } => {
                let mut config = runtime::ensure_config()?;
                let removed = config.remove_backend(&name).is_some();
                config.save()?;
                println!(
                    "{} {}",
                    if removed {
                        "removed backend"
                    } else {
                        "backend not found"
                    },
                    name
                );
            }
            ConfigCommands::AddModel {
                name,
                backend,
                scoped_model,
            } => {
                let mut config = runtime::ensure_config()?;
                config.upsert_model(name.clone(), backend.clone(), scoped_model.clone());
                config.save()?;
                println!(
                    "model {} saved -> backend={} scoped_model={}",
                    name,
                    backend,
                    scoped_model.as_deref().unwrap_or("null")
                );
            }
            ConfigCommands::RemoveModel { name } => {
                let mut config = runtime::ensure_config()?;
                let removed = config.remove_model(&name).is_some();
                config.save()?;
                println!(
                    "{} {}",
                    if removed {
                        "removed model"
                    } else {
                        "model not found"
                    },
                    name
                );
            }
            ConfigCommands::AddAlias {
                name,
                model,
                prompt,
            } => {
                let mut config = runtime::ensure_config()?;
                config.upsert_alias(name.clone(), model.clone(), prompt);
                config.save()?;
                println!("alias {} -> {} saved", name, model);
            }
            ConfigCommands::RemoveAlias { name } => {
                let mut config = runtime::ensure_config()?;
                let removed = config.remove_alias(&name).is_some();
                config.save()?;
                println!(
                    "{} {}",
                    if removed {
                        "removed alias"
                    } else {
                        "alias not found"
                    },
                    name
                );
            }
        },
        Commands::Template { kind, output } => {
            let body = runtime::generate_template(kind.into());
            if let Some(output) = output {
                std::fs::write(&output, body)?;
                println!("{}", output.display());
            } else {
                println!("{body}");
            }
        }
        Commands::Ready => {
            let summary = runtime::readiness_summary()?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::Route { prompt } => {
            let summary = runtime::route_request(&prompt);
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::Backends => {
            for backend in dispatch_backends::all_backends() {
                let availability = backend.detect();
                let capabilities = backend.capabilities();
                println!(
                    "{} installed={} native_sessions={} resumable={} forkable={} auto={} danger={} notes={}",
                    availability.kind.as_str(),
                    availability.installed,
                    capabilities.native_sessions,
                    capabilities.resumable,
                    capabilities.forkable,
                    capabilities.supports_auto_mode,
                    capabilities.supports_danger_mode,
                    availability.notes
                );
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
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": summary.task_id,
                    "status": summary.status,
                }))?
            );
        }
        Commands::Status { task_id } => {
            let task = store.load_task(task_id)?;
            println!("{}", serde_json::to_string_pretty(&task)?);
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
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": summary.task_id,
                    "status": summary.status,
                }))?
            );
        }
        Commands::Events { task_id } => {
            let events = store.read_events(task_id)?;
            println!("{}", serde_json::to_string_pretty(&events)?);
        }
        Commands::Questions { task_id } => {
            let mut all_questions = Vec::new();
            if let Some(task_id) = task_id {
                let task = store.load_task(task_id)?;
                all_questions.push((
                    task_id,
                    list_pending_questions(&task.artifacts.mailbox_dir)?,
                ));
            } else {
                for task_id in store.list_task_ids()? {
                    let task = store.load_task(task_id)?;
                    let pending = list_pending_questions(&task.artifacts.mailbox_dir)?;
                    if !pending.is_empty() {
                        all_questions.push((task_id, pending));
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&all_questions)?);
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
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": task_id,
                    "answered": question.sequence,
                    "answer_path": question.answer_path,
                }))?
            );
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
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
    }

    Ok(())
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
