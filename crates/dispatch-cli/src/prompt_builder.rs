use dispatch_core::TaskRecord;
use std::sync::OnceLock;

const WORKER_TEMPLATE_MARKDOWN: &str = include_str!("../../../prompts/worker-template.md");

pub fn build_worker_prompt(
    task: &TaskRecord,
    user_request: &str,
    resume_note: Option<&str>,
) -> String {
    let mut prompt = load_worker_template().replace("{task_id}", &task.id.to_string());
    prompt.push_str("\n\nContext:\n");
    prompt.push_str(&format!("- Outcome: {}.\n", user_request.trim()));
    prompt.push_str(&format!(
        "- References: read {}, adapt it into a working plan if needed, then write requested results to {} if needed.\n",
        task.artifacts.plan_file.display(),
        task.artifacts.output_file.display()
    ));
    prompt.push_str(&format!(
        "- Mailbox: use {} for worker-initiated questions only.\n",
        task.artifacts.mailbox_dir.display()
    ));
    prompt.push_str(&format!(
        "- Recovery: if blocked, save working context to {} before stopping.\n",
        task.artifacts.context_file.display()
    ));
    prompt.push_str(
        "- Constraints: update only plan.md, output.md, mailbox/, and context.md. Never edit task.json or events.jsonl.\n",
    );
    if let Some(note) = resume_note {
        prompt.push_str(&format!("- Resume note: {}.\n", note.trim()));
    }
    prompt.push_str("\nFinal completion instruction:\n");
    prompt.push_str(&format!(
        "- When all work is complete, write the completion marker at {}.\n",
        task.artifacts.mailbox_dir.join(".done").display()
    ));
    prompt
}

fn load_worker_template() -> &'static str {
    static TEMPLATE: OnceLock<String> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| {
            extract_first_text_block(WORKER_TEMPLATE_MARKDOWN)
                .unwrap_or_else(|| WORKER_TEMPLATE_MARKDOWN.to_string())
        })
        .as_str()
}

fn extract_first_text_block(markdown: &str) -> Option<String> {
    let mut start = None;
    let lines = markdown.lines().collect::<Vec<_>>();
    for (index, line) in markdown.lines().enumerate() {
        if start.is_none() && line.trim() == "```text" {
            start = Some(index + 1);
            continue;
        }
        if let Some(block_start) = start
            && line.trim() == "```"
        {
            return Some(lines[block_start..index].join("\n"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::env;

    use dispatch_core::{
        ArtifactPaths, BackendKind, ExecutionMode, RuntimeCheckpoint, TaskMode, TaskRecord,
        TaskSource, TaskStatus, now,
    };
    use uuid::Uuid;

    use super::{build_worker_prompt, load_worker_template};

    #[test]
    fn worker_prompt_comes_from_prompt_asset_and_uses_mailbox() {
        let task_id = Uuid::new_v4();
        let task_root = env::temp_dir().join(format!("dispatch-test/{task_id}"));
        let workspace = env::temp_dir().join(format!("dispatch-workspace/{task_id}"));
        let task = TaskRecord {
            id: task_id,
            title: "Test".into(),
            prompt: "Review auth".into(),
            task_mode: TaskMode::Plan,
            task_source: TaskSource::InlinePrompt,
            backend: BackendKind::Pi,
            model: Some("pi-default".into()),
            execution_mode: ExecutionMode::Auto,
            workspace_root: workspace,
            created_at: now(),
            updated_at: now(),
            status: TaskStatus::Pending,
            session: None,
            checkpoint: RuntimeCheckpoint::default(),
            artifacts: ArtifactPaths::new(task_root),
        };

        let prompt = build_worker_prompt(&task, "Review auth flows", Some("continue"));
        assert!(load_worker_template().contains("You have a plan file"));
        assert!(prompt.contains("mailbox"));
        assert!(prompt.contains("Review auth flows"));
        assert!(prompt.contains("Never edit task.json"));
        assert!(prompt.contains("Resume note: continue."));
    }
}
