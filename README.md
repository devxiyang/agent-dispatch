# agent-dispatch

Rust workspace for a durable task dispatcher that can drive multiple coding-agent CLIs:

- `codex`
- `claude code`
- `pi`
- `cursor agent` (`agent`)

The design target is not just "spawn a worker", but "persist enough state to recover and continue work later".

The project should be read as a general-purpose prompt + CLI tool:

- prompts define host and worker behavior
- `dispatch-cli` exposes the durable command surface
- optional host adapters can wrap the CLI for specific environments

## Goals

- Persist task state to disk under `.dispatch/tasks/<task-id>/`
- Persist backend session references so the same agent session can be resumed
- Keep an append-only event log for auditing and recovery
- Support backend-specific native session resume when available
- Fall back to external checkpoints when a backend has no validated native resume
- Persist execution permission mode explicitly per task

## Workspace

- `crates/dispatch-core`
  Durable task model, artifact layout, event log, and plan rendering.
- `crates/dispatch-backends`
  Backend traits plus adapters for `codex`, `claude`, `pi`, and `cursor-agent`.
- `crates/dispatch-cli`
  CLI for `init`, `run`, `status`, `resume`, and `events`.
- `integrations/`
  Optional host adapters. These are examples and convenience layers over `dispatch-cli`, not the core product boundary.
- `prompts`
  First-class dispatcher and worker prompt assets derived from the reference project.
- `docs`
  Flow specs and mapping notes that bring the full reference process into this repo.
  Includes a host-neutral integration contract for any calling agent.

## Persistence Model

Each task gets a dedicated directory:

```text
<cwd>/.dispatch/
  tasks/
    <task-id>/
      task.json
      plan.md
      output.md
      context.md
      events.jsonl
      mailbox/
      sessions/
      outputs/
```

`task.json` is the canonical state. `events.jsonl` is append-only. `plan.md` is a worker-owned working artifact, not the runtime source of truth.

`mailbox/` is the worker-to-user handoff channel:

```text
mailbox/
  001.question
  001.answer
  001.done
  .done
```

`outputs/` stores captured stdout/stderr for each execution attempt:

```text
outputs/
  attempt-000.stdout.log
  attempt-000.stderr.log
  attempt-001.stdout.log
  attempt-001.stderr.log
```

## Backend Notes

- `codex`
  Uses native session resume. `auto` maps to `--full-auto`; `danger` maps to `--dangerously-bypass-approvals-and-sandbox`.
- `claude`
  Uses a preallocated `--session-id` so the dispatcher can persist the session reference before execution. `auto` maps to `--permission-mode auto`; `danger` maps to `--dangerously-skip-permissions`.
- `pi`
  Uses a file-backed session in the task's `sessions/` directory via `--session <path>`. The current adapter records execution mode but does not add extra flags because the CLI help does not expose a separate approval-bypass switch.
- `cursor-agent`
  Included as a first-class backend target, but native resume is still marked unvalidated until the local CLI can be checked. `auto` and `danger` currently both map to `--force`. External task checkpoints still work.

## CLI Flow

Most operational commands support `--json` so a host prompt can treat `dispatch` as a stable command subsystem instead of scraping prose.

- `dispatch init`
  Create the root task store.
- `dispatch template`
  Generate a `plan.md` template that the user can fill in directly.
- `dispatch ready`
  Load config and report dispatcher readiness without starting a task.
- `dispatch route --prompt "..."`
  Advisory classifier for warm-up, config request, or task request. Hosts may use it as a suggestion helper instead of treating it as an authoritative router.
- `dispatch config show`
  Inspect the current explicit backend/model/alias mapping.
- `dispatch config add-backend|remove-backend`
  Manage backend command definitions.
- `dispatch config add-model|remove-model`
  Manage named model entries with explicit backend binding and optional scoped model id.
- `dispatch config add-alias|remove-alias`
  Manage user-facing aliases that point to a model and can prepend prompt guidance.
- `dispatch config set-default`
  Change the default model or alias.
- `dispatch backends`
  Show installed status and capability flags for each backend.
- `dispatch run`
  Persist a task from an inline prompt, `prompt.md`, or `plan.md`, then execute it by default.
- `dispatch list`
  Show recent tasks in a compact summary view.
- `dispatch inspect`
  Show task state, pending mailbox questions, and recent events in one call.
- `dispatch resume`
  Reuse the persisted session reference and execute the next turn by default.
- `dispatch status`
  Show the canonical task record.
- `dispatch events`
  Show the append-only event log.
- `dispatch questions`
  Show pending mailbox questions across tasks or for a specific task.
- `dispatch answer`
  Write an answer into the task mailbox.

`run` now supports three task modes:

- `--mode direct`
  Push a clear prompt straight to the worker.
- `--mode plan`
  Use or generate a checklist-driven `plan.md`.
- `--mode discuss`
  Produce a discussion draft and wait for user clarification.

When `--mode auto` is used:

- `plan.md` inputs route to `plan`
- `prompt.md` inputs route to `direct`
- inline prompts default to `direct` so the host prompt remains in control

Input sources:

- `--prompt "..."`
  Inline prompt
- `--from prompt.md`
  Prompt file
- `--from plan.md`
  User-authored plan file

Default task root is always `cwd/.dispatch` unless `--root` is set explicitly.

## Host Integrations

`dispatch-cli` is the primary tool surface.

Host integrations are optional wrappers that help a specific coding agent invoke the CLI more ergonomically.

The host-neutral integration contract is documented at [host-integration-contract.md](docs/host-integration-contract.md).
Adapter-specific guidance lives in [integrations/README.md](integrations/README.md).

Minimal generic flow:

```text
dispatch --json ready
dispatch --json run --prompt "review auth flow" --mode direct
dispatch --json inspect <task-id>
dispatch --json questions <task-id>
dispatch --json answer <task-id> --message "..."
dispatch --json resume <task-id> --message "continue with the user's answer"
```

The repository currently includes one adapter example for `pi`.

### Pi Host Extension

The repository now includes a `pi` host extension at `integrations/pi-dispatch-host/index.ts`.

Development load:

```bash
pi -e /path/to/agent-dispatch/integrations/pi-dispatch-host/index.ts
```

Global install:

```bash
mkdir -p ~/.pi/agent/extensions/dispatch
cp /path/to/agent-dispatch/integrations/pi-dispatch-host/index.ts ~/.pi/agent/extensions/dispatch/index.ts
```

The extension resolves the dispatcher in this order:

1. `DISPATCH_BIN`
2. `DISPATCH_WORKSPACE`
3. repo-local `target/debug/dispatch-cli`
4. `cargo run -p dispatch-cli`
5. `dispatch-cli` on `PATH`

For a copied global extension, set one of these first:

```bash
export DISPATCH_WORKSPACE=/path/to/agent-dispatch
# or
export DISPATCH_BIN=/absolute/path/to/dispatch-cli
```

Supported commands inside `pi`:

- `/dispatch <prompt>`
- `/dispatch --backend codex --model gpt-5.3-codex --mode direct <prompt>`
- `/dispatch --from plan.md --mode plan`
- `/dispatch template --kind audit --output plan.md`
- `/dispatch ready`
- `/dispatch config show`
- `/dispatch set default to sonnet`
- `/dispatch list`
- `/dispatch inspect <task-id>`
- `/dispatch status [task-id]`
- `/dispatch questions [task-id]`
- `/dispatch events [task-id]`
- `/dispatch answer <task-id> <message...>`
- `/dispatch resume <task-id> <message...>`
- `/dispatch backends`

The extension persists the last selected task in the `pi` session via `appendEntry()`, restores it on session start/tree navigation, and updates `ctx.ui.setStatus()` plus a small widget with the current task state.
It also routes empty requests to readiness checks and can translate simple config requests such as `set default to sonnet` into the corresponding `dispatch config ...` command.

Nothing in the core runtime depends on `pi`. Other hosts can integrate by invoking `dispatch-cli` with `--json` and treating it as a command subsystem.

## Test Coverage

The repository includes both unit tests and end-to-end tests:

- mailbox persistence and atomic answers
- runtime mode resolution and file-source parsing
- executor output capture and session extraction
- prompt-file direct execution
- user-authored `plan.md` execution
- discuss-mode draft creation
- background execution
- mailbox question -> answer -> resume roundtrip
- readiness reporting and request routing
- config mutation commands for explicit backend/model/alias mappings

## Prompt and Flow Assets

The complete reference behavior from the upstream `dispatch` project is now landed in repository assets:

- [prompts/dispatcher-system.md](prompts/dispatcher-system.md)
- [prompts/worker-template.md](prompts/worker-template.md)
- [docs/dispatch-flows.md](docs/dispatch-flows.md)
- [docs/dispatch-reference-map.md](docs/dispatch-reference-map.md)
- [docs/host-integration-contract.md](docs/host-integration-contract.md)

These files are now part of runtime behavior:

- worker prompts are rendered from [worker-template.md](prompts/worker-template.md), not from an inline hardcoded string
- mailbox flow and recovery semantics are exercised by unit tests and e2e tests
- config is editable through explicit CLI commands instead of relying on heuristic model-family routing
