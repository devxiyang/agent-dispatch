use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn setup_env() -> (TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("root");
    fs::create_dir_all(home.join(".dispatch")).unwrap();
    fs::create_dir_all(&root).unwrap();
    let config = r#"
default: fake-pi
backends:
  pi:
    executable: /bin/sh
    args:
      - -c
      - |
        prompt="$(cat)"
        session="$1"
        taskdir="$(dirname "$(dirname "$session")")"
        mkdir -p "$taskdir/mailbox"
        if printf '%s' "$prompt" | grep -q 'NEED-ANSWER-E2E'; then
          if [ -f "$taskdir/mailbox/001.answer" ]; then
            perl -0pi -e 's/\[\?\]/[x]/g; s/\[>\]/[x]/g; s/\[ \]/[x]/g' "$taskdir/plan.md"
            printf 'answer: ' > "$taskdir/output.md"
            cat "$taskdir/mailbox/001.answer" >> "$taskdir/output.md"
            printf '\n' >> "$taskdir/output.md"
            : > "$taskdir/mailbox/001.done"
            : > "$taskdir/mailbox/.done"
          else
            perl -0pi -e 's/\[>\]/[?]/g' "$taskdir/plan.md"
            printf 'What should the worker do next?\n' > "$taskdir/mailbox/001.question"
          fi
        else
          perl -0pi -e 's/\[ \]/[x]/g; s/\[>\]/[x]/g' "$taskdir/plan.md"
          printf 'fake summary\n' > "$taskdir/output.md"
          : > "$taskdir/mailbox/.done"
        fi
        printf '{"ok":true}\n'
      - dispatch-sh
      - "{session_file}"
models:
  fake-pi:
    backend: pi
    model: null
aliases: {}
"#;
    fs::write(home.join(".dispatch/config.yaml"), config.trim_start()).unwrap();
    (temp, home, root)
}

fn dispatch_cmd(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("dispatch-cli").unwrap();
    cmd.env("HOME", home);
    cmd
}

#[test]
fn template_can_be_written_to_file() {
    let (temp, home, _root) = setup_env();
    let output = temp.path().join("plan-template.md");
    dispatch_cmd(&home)
        .args([
            "--root",
            temp.path().join("task-store").to_str().unwrap(),
            "template",
            "--kind",
            "audit",
            "--output",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();

    let body = fs::read_to_string(output).unwrap();
    assert!(body.contains("# Audit"));
    assert!(body.contains("## Plan"));
}

#[test]
fn ready_reports_config_and_installed_backends() {
    let (_temp, home, root) = setup_env();

    let output = dispatch_cmd(&home)
        .args(["--root", root.to_str().unwrap(), "ready"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let payload: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(payload["default_target"], "fake-pi");
    assert_eq!(payload["backend_count"], 1);
    assert_eq!(payload["model_count"], 1);
}

#[test]
fn route_classifies_warmup_config_and_task_requests() {
    let (_temp, home, root) = setup_env();

    let warmup: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args(["--root", root.to_str().unwrap(), "route", "--prompt", ""])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_eq!(warmup["kind"], "Warmup");

    let config: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args([
                "--root",
                root.to_str().unwrap(),
                "route",
                "--prompt",
                "set default to fake-pi",
            ])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_eq!(config["kind"], "ConfigRequest");
    assert_eq!(config["suggested_cli_args"][1], "set-default");

    let task: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args([
                "--root",
                root.to_str().unwrap(),
                "route",
                "--prompt",
                "fix the README typo",
            ])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_eq!(task["kind"], "TaskRequest");
    assert_eq!(task["suggested_mode"], "direct");
}

#[test]
fn direct_prompt_file_mode_completes() {
    let (_temp, home, root) = setup_env();
    let prompt_file = root.parent().unwrap().join("prompt.md");
    fs::write(
        &prompt_file,
        "Reply with exactly DIRECT-E2E and nothing else.\n",
    )
    .unwrap();

    let output = dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "--from",
            prompt_file.to_str().unwrap(),
            "--mode",
            "direct",
            "--model",
            "fake-pi",
            "--workspace",
            ".",
            "--foreground",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let task_id = value["task_id"].as_str().unwrap();
    let task: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args(["--root", root.to_str().unwrap(), "status", task_id])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();

    assert_eq!(task["task_mode"], "Direct");
    assert_eq!(task["task_source"], "PromptFile");
    assert_eq!(task["status"], "Completed");
}

#[test]
fn plan_file_mode_preserves_user_plan_and_completes() {
    let (_temp, home, root) = setup_env();
    let plan_file = root.parent().unwrap().join("plan.md");
    fs::write(
        &plan_file,
        "# User Plan\n\n## Goal\n\nShip a deterministic response.\n\n## Plan\n\n- [ ] Reply with exactly PLAN-E2E and nothing else.\n- [ ] Write summary to `.dispatch/tasks/<task-id>/output.md`\n",
    )
    .unwrap();

    let output = dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "--from",
            plan_file.to_str().unwrap(),
            "--mode",
            "plan",
            "--model",
            "fake-pi",
            "--workspace",
            ".",
            "--foreground",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let task_id = value["task_id"].as_str().unwrap();
    let task_dir = root.join("tasks").join(task_id);
    let task: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(task_dir.join("task.json")).unwrap()).unwrap();
    let plan = fs::read_to_string(task_dir.join("plan.md")).unwrap();

    assert_eq!(task["task_mode"], "Plan");
    assert_eq!(task["task_source"], "PlanFile");
    assert_eq!(task["preserve_plan_file"], true);
    assert!(plan.contains("- [x] Reply with exactly PLAN-E2E and nothing else."));
}

#[test]
fn discuss_mode_creates_draft_and_waits_for_user() {
    let (_temp, home, root) = setup_env();

    let output = dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "--prompt",
            "Help me decide whether this should be direct or plan mode.",
            "--mode",
            "discuss",
            "--model",
            "fake-pi",
            "--workspace",
            ".",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let task_id = value["task_id"].as_str().unwrap();
    assert_eq!(value["status"], "awaiting_user");

    let task_dir = root.join("tasks").join(task_id);
    let task: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(task_dir.join("task.json")).unwrap()).unwrap();
    let draft = fs::read_to_string(task_dir.join("output.md")).unwrap();
    assert_eq!(task["status"], "AwaitingUser");
    assert!(draft.contains("## Open Questions"));
}

#[test]
fn background_dispatch_completes_and_updates_status() {
    let (_temp, home, root) = setup_env();

    let output = dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "--prompt",
            "Reply with exactly BACKGROUND-E2E and nothing else.",
            "--model",
            "fake-pi",
            "--workspace",
            ".",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let task_id = value["task_id"].as_str().unwrap().to_string();
    assert_eq!(value["status"], "dispatched");

    let mut status = serde_json::Value::Null;
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(150));
        status = serde_json::from_slice(
            &dispatch_cmd(&home)
                .args(["--root", root.to_str().unwrap(), "status", &task_id])
                .assert()
                .success()
                .get_output()
                .stdout,
        )
        .unwrap();
        if status["status"] == "Completed" {
            break;
        }
    }

    assert_eq!(status["status"], "Completed");
}

#[test]
fn config_commands_persist_explicit_backend_model_and_alias_mappings() {
    let (_temp, home, root) = setup_env();

    dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "config",
            "add-backend",
            "custom-pi",
            "/bin/sh",
            "--arg=-c",
            "--arg=printf '{\"ok\":true}\\n'",
        ])
        .assert()
        .success();

    dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "config",
            "add-model",
            "review-model",
            "--backend",
            "custom-pi",
            "--scoped-model",
            "pi-reviewer",
        ])
        .assert()
        .success();

    dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "config",
            "add-alias",
            "reviewer",
            "--model",
            "review-model",
            "--prompt",
            "focus on risks",
        ])
        .assert()
        .success();

    dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "config",
            "set-default",
            "reviewer",
        ])
        .assert()
        .success();

    let config_output = dispatch_cmd(&home)
        .args(["--root", root.to_str().unwrap(), "config", "show"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let config: serde_yaml::Value = serde_yaml::from_slice(&config_output).unwrap();

    assert_eq!(config["default"], "reviewer");
    assert_eq!(config["backends"]["custom-pi"]["executable"], "/bin/sh");
    assert_eq!(config["models"]["review-model"]["backend"], "custom-pi");
    assert_eq!(config["models"]["review-model"]["model"], "pi-reviewer");
    assert_eq!(config["aliases"]["reviewer"]["model"], "review-model");
    assert_eq!(config["aliases"]["reviewer"]["prompt"], "focus on risks");
}

#[test]
fn mailbox_question_answer_and_resume_roundtrip_completes() {
    let (_temp, home, root) = setup_env();

    let output = dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "--prompt",
            "NEED-ANSWER-E2E",
            "--mode",
            "plan",
            "--model",
            "fake-pi",
            "--workspace",
            ".",
            "--foreground",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let task_id = value["task_id"].as_str().unwrap();
    let task_dir = root.join("tasks").join(task_id);

    let initial_status: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args(["--root", root.to_str().unwrap(), "status", task_id])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_eq!(initial_status["status"], "AwaitingUser");

    let questions: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args(["--root", root.to_str().unwrap(), "questions", task_id])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_eq!(questions[0][0], task_id);
    assert_eq!(questions[0][1][0]["sequence"], "001");

    dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "answer",
            task_id,
            "--message",
            "Finish the task",
        ])
        .assert()
        .success();

    dispatch_cmd(&home)
        .args([
            "--root",
            root.to_str().unwrap(),
            "resume",
            task_id,
            "--message",
            "Continue with the mailbox answer",
            "--foreground",
        ])
        .assert()
        .success();

    let final_status: serde_json::Value = serde_json::from_slice(
        &dispatch_cmd(&home)
            .args(["--root", root.to_str().unwrap(), "status", task_id])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    let output_body = fs::read_to_string(task_dir.join("output.md")).unwrap();

    assert_eq!(final_status["status"], "Completed");
    assert!(output_body.contains("answer: Finish the task"));
    assert!(task_dir.join("mailbox/.done").exists());
}
