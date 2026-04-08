# Host Integration Contract

This document defines how any calling agent or host environment should integrate with `dispatch-cli`.

It is intentionally host-agnostic.

Examples of possible hosts include:

- a coding-agent CLI with shell access
- a slash-command extension
- a local TUI or editor integration
- an orchestration agent that delegates long-running work

The contract does not assume `pi`, Claude Code, Codex, or any other specific host.

## Product Boundary

`dispatch-cli` is the stable integration surface.

Hosts are expected to:

- decide when dispatch should be used
- invoke `dispatch-cli`
- interpret structured output
- continue the user conversation in their own voice

`dispatch-cli` is expected to:

- perform durable task actions
- persist task state
- manage worker execution and resume
- expose machine-readable operational state

Task artifacts live under the configured task root.
When a backend uses file-backed session paths, that storage is runtime-owned and lives outside the task root.

## Required Host Capabilities

A host integration should be able to:

- execute local shell commands
- pass arguments safely to `dispatch-cli`
- read stdout and stderr
- maintain lightweight conversational memory about the currently relevant task id

Optional but useful capabilities:

- background polling
- task widgets or status bars
- clickable artifact paths
- richer task list UIs

## Invocation Rule

For operational calls, hosts should use `--json`.

Examples:

```text
dispatch --json ready
dispatch --json run --prompt "review auth flow" --mode direct
dispatch --json inspect <task-id>
dispatch --json answer <task-id> --message "Use GitHub OAuth only"
dispatch --json resume <task-id> --message "continue with the user's answer"
```

Hosts should treat the CLI as an API, not as prose.

## Output Contract

Operational commands return a JSON envelope.

Success:

```json
{
  "ok": true,
  "data": {}
}
```

Failure:

```json
{
  "ok": false,
  "error": {
    "message": "..."
  }
}
```

Hosts should:

- branch on `ok`
- use `data` on success
- surface `error.message` on failure

Hosts should not depend on default non-JSON output for automation.

## Command Roles

### Readiness

Use:

- `dispatch --json ready`

Purpose:

- confirm config availability
- inspect installed backends
- warm up dispatch usage in a session

### Task creation

Use:

- `dispatch --json run --prompt "..."`
- `dispatch --json run --from plan.md`

Purpose:

- create a durable task record
- launch a worker turn

### Task inspection

Use:

- `dispatch --json inspect <task-id>`
- `dispatch --json list`
- `dispatch --json status <task-id>`
- `dispatch --json events <task-id>`
- `dispatch --json questions [task-id]`

Purpose:

- inspect the durable state instead of guessing from conversation context

### Mailbox and resume

Use:

- `dispatch --json answer <task-id> --message "..."`
- `dispatch --json resume <task-id> --message "..."`

Purpose:

- answer worker questions
- continue execution after a pause or failure

`resume` means "continue the existing durable task using its stored session reference when available".

Hosts should think in task ids, not in raw backend session handles.

For example:

- `pi` tasks may carry an exact file-backed session path
- `claude` tasks may carry a native session id
- `codex` tasks may carry a captured native session id

The host should not manipulate those handles directly.

### Forking

Some backends support native session forking, but `fork` is not currently part of the stable host-facing CLI surface.

Hosts should treat forking as an internal runtime capability unless and until a dedicated CLI command is added.

## Host Responsibilities

The host owns:

- user-facing conversation
- deciding when to dispatch
- deciding which command to call next
- summarizing task state in natural language
- collecting answers to worker questions

The host should not:

- treat `plan.md` as canonical state
- write mailbox files directly
- rely on hidden in-memory state instead of inspecting task state
- make the CLI responsible for conversation

## Suggested Host Flow

For a new task:

1. interpret the user's request
2. choose whether to dispatch
3. call `dispatch --json run ...`
4. report task id and summary back to the user

For a task update:

1. call `dispatch --json inspect <task-id>`
2. summarize the task, recent events, and pending questions
3. if needed, ask the user a focused follow-up

For a blocked task:

1. call `dispatch --json questions <task-id>`
2. surface the worker question
3. collect the user answer
4. call `dispatch --json answer ...`
5. call `dispatch --json resume ...`

For a retry or continuation:

1. call `dispatch --json inspect <task-id>`
2. confirm that the task should continue rather than be replaced by a new task
3. call `dispatch --json resume <task-id> --message "..."`

## Generic Host Example

Below is a host-neutral example of how a calling agent could drive `dispatch-cli`.

### 1. Warm up

Run:

```text
dispatch --json ready
```

Read:

- `data.default_target`
- `data.installed_backends`

### 2. Start a task

Run:

```text
dispatch --json run --prompt "review auth flow for security issues" --mode direct
```

Read:

- `data.task_id`
- `data.status`

Then store the `task_id` in lightweight host memory for follow-up calls.

### 3. Inspect progress

Run:

```text
dispatch --json inspect <task-id>
```

Read:

- `data.task.status`
- `data.pending_questions`
- `data.recent_events`

If there are no pending questions, summarize the state back to the user.

### 4. Handle a worker question

Run:

```text
dispatch --json questions <task-id>
```

If a question exists:

1. ask the user the question in normal conversation
2. collect the answer
3. run:

```text
dispatch --json answer <task-id> --message "Use GitHub OAuth only"
dispatch --json resume <task-id> --message "continue with the user's answer"
```

### 5. Check completion

Run:

```text
dispatch --json inspect <task-id>
```

If `data.task.status` is `Completed`:

- summarize the result
- optionally read `output.md` if you need richer detail than the task snapshot or events provide

## Artifact Semantics

Hosts should understand these file roles, but usually do not need to manipulate them directly:

- `task.json`
  canonical task snapshot
- `events.jsonl`
  append-only runtime history
- `plan.md`
  worker-owned working artifact
- `output.md`
  worker output
- `context.md`
  recovery artifact
- `mailbox/`
  worker question and answer exchange

The important rule is:

- inspect through CLI first
- read artifacts directly only when needed for debugging or deeper context

## Host Neutrality

This contract is intentionally neutral.

Any host-specific adapter should be a thin layer over this contract.

That means a host adapter may add:

- custom slash commands
- status bar updates
- task selection memory
- richer formatting

But it should not change:

- the CLI command semantics
- the JSON envelope contract
- the artifact semantics

## Principle

Any host can integrate with `dispatch-cli` by following the same pattern:

- call commands
- read structured output
- keep the conversation in the host
