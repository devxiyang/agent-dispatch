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
            workspace_root: draft.workspace_root,
            created_at: now,
            updated_at: now,
            status: TaskStatus::Pending,
            session: None,
            checkpoint: Default::default(),
            artifacts,
        };

        self.save_task(&task)?;
        self.write_initial_plan_artifact(&task, draft.plan_body.as_deref())?;
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

    fn write_initial_plan_artifact(
        &self,
        task: &TaskRecord,
        plan_body: Option<&str>,
    ) -> Result<()> {
        let body = plan_body
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.default_plan_artifact(task));
        fs::write(&task.artifacts.plan_file, body).map_err(|source| DispatchError::Io {
            path: task.artifacts.plan_file.clone(),
            source,
        })?;
        Ok(())
    }

    fn default_plan_artifact(&self, task: &TaskRecord) -> String {
        let mut body = String::new();
        body.push_str(&format!("# {}\n\n", task.title));
        body.push_str("## Task\n\n");
        body.push_str(&task.prompt);
        body.push_str("\n\n## Runtime Context\n\n");
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
            "- workspace: `{}`\n",
            task.workspace_root.display()
        ));
        body.push_str("\n## Working Plan\n\n");
        body.push_str(
            "- Convert this artifact into a working checklist if the task benefits from one.\n",
        );
        body.push_str("- Update this file as you make progress.\n");
        body.push_str("- Ask the user questions through the mailbox when blocked.\n");
        body.push_str("\n## Output\n\n");
        body.push_str(&format!(
            "Write any requested summary or report to `{}`.\n",
            task.artifacts.output_file.display()
        ));
        body
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
    use super::DispatchStore;
    use crate::model::{BackendKind, ExecutionMode, TaskDraft, TaskMode, TaskSource};
    use std::env;
    use std::fs;

    #[test]
    fn creates_persistent_task_layout() {
        let root = env::temp_dir().join(format!("dispatch-core-test-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
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
                plan_body: None,
                workspace_root: workspace,
            })
            .unwrap();

        assert!(task.artifacts.task_file.exists());
        assert!(task.artifacts.plan_file.exists());
        assert!(task.artifacts.mailbox_dir.exists());
        assert_eq!(store.read_events(task.id).unwrap().len(), 1);
        let plan = fs::read_to_string(task.artifacts.plan_file).unwrap();
        assert!(plan.contains("## Working Plan"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_updates_do_not_overwrite_worker_owned_plan_artifact() {
        let root = env::temp_dir().join(format!("dispatch-core-test-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        let store = DispatchStore::new(&root);
        let task = store
            .create_task(TaskDraft {
                title: "Worker owned plan".into(),
                prompt: "Use the artifact as a working notebook".into(),
                task_mode: TaskMode::Plan,
                task_source: TaskSource::PlanFile,
                backend: BackendKind::Pi,
                model: Some("pi-default".into()),
                execution_mode: ExecutionMode::Auto,
                plan_body: Some("# User Plan\n\n- [ ] initial step\n".into()),
                workspace_root: workspace,
            })
            .unwrap();

        fs::write(
            &task.artifacts.plan_file,
            "# Worker Revised Plan\n\n- [x] updated step\n",
        )
        .unwrap();
        store
            .set_status(task.id, crate::model::TaskStatus::Running, "worker resumed")
            .unwrap();

        let plan = fs::read_to_string(&task.artifacts.plan_file).unwrap();
        assert!(plan.contains("# Worker Revised Plan"));
        assert!(plan.contains("- [x] updated step"));

        fs::remove_dir_all(root).unwrap();
    }
}
