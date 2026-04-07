use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::{DispatchError, Result};
use crate::model::{ArtifactPaths, EventKind, EventRecord, TaskDraft, TaskRecord, TaskStatus, now};

#[derive(Debug, Clone)]
pub struct DispatchStore {
    root: PathBuf,
}

impl DispatchStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn init(&self) -> Result<()> {
        self.mkdir(&self.root)?;
        self.mkdir(&self.tasks_root())?;
        Ok(())
    }

    pub fn create_task(&self, draft: TaskDraft) -> Result<TaskRecord> {
        self.init()?;

        let id = Uuid::new_v4();
        let task_root = self.task_dir(id);
        self.mkdir(&task_root)?;
        let artifacts = ArtifactPaths::new(task_root.clone());
        self.mkdir(&artifacts.mailbox_dir)?;
        self.mkdir(&artifacts.sessions_dir)?;
        self.mkdir(&artifacts.outputs_dir)?;

        let now = now();
        let task = TaskRecord {
            id,
            title: draft.title,
            prompt: draft.prompt,
            task_mode: draft.task_mode,
            task_source: draft.task_source,
            backend: draft.backend,
            model: draft.model,
            execution_mode: draft.execution_mode,
            preserve_plan_file: draft.preserve_plan_file,
            workspace_root: draft.workspace_root,
            created_at: now,
            updated_at: now,
            status: TaskStatus::Pending,
            plan: draft.plan,
            session: None,
            checkpoint: Default::default(),
            artifacts,
        };

        self.save_task(&task)?;
        if let Some(plan_body) = draft.plan_body {
            fs::write(&task.artifacts.plan_file, plan_body).map_err(|source| {
                DispatchError::Io {
                    path: task.artifacts.plan_file.clone(),
                    source,
                }
            })?;
        }
        self.append_event(id, EventKind::Created, "task created")?;
        Ok(task)
    }

    pub fn load_task(&self, id: Uuid) -> Result<TaskRecord> {
        let path = self.task_file(id);
        let bytes = fs::read(&path).map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => DispatchError::TaskNotFound(id.to_string()),
            _ => DispatchError::Io { path, source },
        })?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save_task(&self, task: &TaskRecord) -> Result<()> {
        self.write_json(&task.artifacts.task_file, task)?;
        self.write_plan(task)?;
        Ok(())
    }

    pub fn set_status(
        &self,
        id: Uuid,
        status: TaskStatus,
        message: impl Into<String>,
    ) -> Result<TaskRecord> {
        let mut task = self.load_task(id)?;
        task.status = status;
        task.updated_at = now();
        self.save_task(&task)?;

        let kind = match task.status {
            TaskStatus::Completed => EventKind::Completed,
            TaskStatus::Failed => EventKind::Failed,
            _ => EventKind::StatusChanged,
        };
        self.append_event(id, kind, message)?;
        Ok(task)
    }

    pub fn update_task<F>(&self, id: Uuid, mutator: F) -> Result<TaskRecord>
    where
        F: FnOnce(&mut TaskRecord),
    {
        let mut task = self.load_task(id)?;
        mutator(&mut task);
        task.updated_at = now();
        self.save_task(&task)?;
        Ok(task)
    }

    pub fn append_event(
        &self,
        id: Uuid,
        kind: EventKind,
        message: impl Into<String>,
    ) -> Result<EventRecord> {
        let path = self.events_file(id);
        let sequence = self.read_event_count(id)? + 1;
        let event = EventRecord::new(sequence, kind, message);
        let line = serde_json::to_string(&event)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| DispatchError::Io {
                path: path.clone(),
                source,
            })?;
        writeln!(file, "{line}").map_err(|source| DispatchError::Io { path, source })?;
        Ok(event)
    }

    pub fn read_events(&self, id: Uuid) -> Result<Vec<EventRecord>> {
        let path = self.events_file(id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&path).map_err(|source| DispatchError::Io {
            path: path.clone(),
            source,
        })?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|source| DispatchError::Io {
                path: path.clone(),
                source,
            })?;
            events.push(serde_json::from_str(&line)?);
        }
        Ok(events)
    }

    pub fn list_task_ids(&self) -> Result<Vec<Uuid>> {
        let root = self.tasks_root();
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::new();
        for entry in fs::read_dir(&root).map_err(|source| DispatchError::Io {
            path: root.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| DispatchError::Io {
                path: root.clone(),
                source,
            })?;
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(id) = Uuid::parse_str(name) {
                    ids.push(id);
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn write_plan(&self, task: &TaskRecord) -> Result<()> {
        let mut body = String::new();
        if task.preserve_plan_file && task.artifacts.plan_file.exists() {
            return Ok(());
        }
        body.push_str(&format!("# {}\n\n", task.title));
        body.push_str(&format!("- id: `{}`\n", task.id));
        body.push_str(&format!("- status: `{:?}`\n", task.status));
        body.push_str(&format!("- mode: `{:?}`\n", task.task_mode));
        body.push_str(&format!("- source: `{:?}`\n", task.task_source));
        body.push_str(&format!("- backend: `{}`\n", task.backend.as_str()));
        if let Some(model) = &task.model {
            body.push_str(&format!("- model: `{model}`\n"));
        }
        body.push_str(&format!("- execution-mode: `{:?}`\n", task.execution_mode));
        body.push_str(&format!(
            "- workspace: `{}`\n\n",
            task.workspace_root.display()
        ));
        body.push_str("## Prompt\n\n");
        body.push_str(&task.prompt);
        body.push_str("\n\n## Plan\n\n");

        for step in &task.plan {
            body.push_str(&format!(
                "{} {} (`{}`)\n",
                step.status.markdown_checkbox(),
                step.title,
                step.id
            ));
            for note in &step.notes {
                body.push_str(&format!("  - {note}\n"));
            }
        }

        if !task.artifacts.output_file.as_path().exists() {
            body.push_str("\n## Output\n\n");
            body.push_str(&format!(
                "Write any requested summary or report to `{}`.\n",
                task.artifacts.output_file.display()
            ));
        }

        fs::write(&task.artifacts.plan_file, body).map_err(|source| DispatchError::Io {
            path: task.artifacts.plan_file.clone(),
            source,
        })?;
        Ok(())
    }

    fn write_json<T: serde::Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        fs::write(path, bytes).map_err(|source| DispatchError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    fn read_event_count(&self, id: Uuid) -> Result<u64> {
        Ok(self.read_events(id)?.len() as u64)
    }

    fn tasks_root(&self) -> PathBuf {
        self.root.join("tasks")
    }

    fn task_dir(&self, id: Uuid) -> PathBuf {
        self.tasks_root().join(id.to_string())
    }

    fn task_file(&self, id: Uuid) -> PathBuf {
        self.task_dir(id).join("task.json")
    }

    fn events_file(&self, id: Uuid) -> PathBuf {
        self.task_dir(id).join("events.jsonl")
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).map_err(|source| DispatchError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    use super::DispatchStore;
    use crate::model::{
        BackendKind, ExecutionMode, PlanStep, StepStatus, TaskDraft, TaskMode, TaskSource,
    };

    #[test]
    fn creates_persistent_task_layout() {
        let root = env::temp_dir().join(format!("dispatch-core-test-{}", uuid::Uuid::new_v4()));
        let store = DispatchStore::new(&root);
        let task = store
            .create_task(TaskDraft {
                title: "Review auth module".into(),
                prompt: "Inspect auth flows and list findings".into(),
                task_mode: TaskMode::Plan,
                task_source: TaskSource::InlinePrompt,
                backend: BackendKind::Codex,
                model: Some("gpt-5.3-codex".into()),
                execution_mode: ExecutionMode::Auto,
                preserve_plan_file: false,
                plan_body: None,
                workspace_root: PathBuf::from("/tmp/workspace"),
                plan: vec![PlanStep {
                    id: "review".into(),
                    title: "Review the auth module".into(),
                    status: StepStatus::Pending,
                    notes: vec!["focus on session persistence".into()],
                }],
            })
            .unwrap();

        assert!(task.artifacts.task_file.exists());
        assert!(task.artifacts.plan_file.exists());
        assert!(task.artifacts.mailbox_dir.exists());
        assert_eq!(store.read_events(task.id).unwrap().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }
}
