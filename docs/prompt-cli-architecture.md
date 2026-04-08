# Prompt + CLI Architecture

This document defines the intended architecture for `agent-dispatch`.

The project is not a standalone orchestration product and not a traditional workflow engine. It is a prompt-driven dispatch system with a local CLI runtime.

The reference model is the upstream `/dispatch` project:

- prompts define dispatcher and worker behavior
- durable files define the execution contract
- the host agent stays conversational

This repository adds the missing runtime layer as a CLI.

## Core Idea

`agent-dispatch` is:

- a dispatcher prompt contract
- a worker prompt contract
- a CLI that performs durable task actions
- optional host adapters that wrap the CLI
- a task artifact layout that preserves state across turns

In short:

```text
dispatch = prompt + cli + artifacts
```

## Non-Goals

This project is not:

- a standalone chat system
- a daemon-first scheduler
- a UI-first application
- a rigid workflow engine that replaces model judgment

The model should remain responsible for interpreting user intent and deciding how work should proceed.

## Design Principles

### Keep intelligence in prompts

The dispatcher and worker prompts should own:

- intent interpretation
- task decomposition
- plan adaptation
- deciding when to ask questions
- deciding how to continue after answers arrive
- deciding how to summarize results

### Keep side effects in the CLI

The CLI should own:

- creating tasks
- starting workers
- resuming workers
- reading status
- reading events
- reading and answering mailbox questions
- managing config and backend selection

The CLI should not try to replace model reasoning with rigid orchestration logic.

### Keep memory in artifacts

Durable files should preserve enough state to recover work after host restarts, backend failures, or user interruptions.

## Architecture

```text
host coding agent
  -> uses dispatcher prompt
  -> calls dispatch CLI

dispatch CLI
  -> creates and updates task artifacts
  -> starts or resumes backend workers
  -> returns machine-readable state

optional host adapter
  -> wraps the CLI for a specific environment

worker agent
  -> uses worker prompt
  -> reads and updates task artifacts
  -> asks questions through mailbox files
```

The CLI is the stable product surface.

Host adapters are convenience layers, not the architectural center.

See also: [host-integration-contract.md](host-integration-contract.md)
for a generic calling pattern any host can follow.
Adapter-specific notes live in [integrations/README.md](../integrations/README.md).

## Prompt Contract

There are two prompt roles.

### 1. Dispatcher prompt

The dispatcher prompt runs in the host coding agent session.

Its responsibilities are:

- decide whether the user is asking for warmup, config work, new execution, inspection, or resume
- decide which CLI command to run
- decide when to inspect task state or mailbox questions
- decide how to explain progress and results back to the user

The dispatcher prompt should treat CLI output as operational state, not as the final user-facing response.

In practice, the dispatcher prompt should behave like a host-side operator:

- route warmup, config, task, inspection, and resume requests
- choose backend, model, and task mode deliberately
- prefer inspecting durable state over trusting conversational memory
- surface mailbox questions to the user instead of inventing answers
- recover from failures using observed runtime state

### 2. Worker prompt

The worker prompt runs in the backend worker session.

Its responsibilities are:

- read the task plan or task context
- perform the requested work
- update `plan.md` when helpful
- write `output.md` when the task calls for a final artifact
- write `context.md` before stopping on timeout or blockage
- use mailbox files for worker-initiated questions

The worker prompt should be allowed to adapt the plan as needed. The runtime must not assume the original plan is final.

In practice, the worker prompt should preserve execution discipline:

- turn a weak task brief into a usable working plan when needed
- keep progress markers honest when checklist form is used
- use mailbox files only for worker-initiated clarification
- leave behind readable context when blocked or failing
- keep plan state, output artifacts, and completion markers consistent

## CLI Contract

The CLI is the executable surface area that the host prompt can rely on.

It should expose a narrow, stable command set:

- `dispatch ready`
- `dispatch run`
- `dispatch list`
- `dispatch inspect`
- `dispatch status`
- `dispatch events`
- `dispatch questions`
- `dispatch answer`
- `dispatch resume`
- `dispatch config ...`
- `dispatch backends`

Optional convenience commands can exist, but these are the core runtime actions.

Backend-native forking may exist internally, but it is not part of the primary public CLI contract today.

### CLI responsibilities

- create task directories and canonical task records
- persist task metadata
- record append-only events
- launch workers through backend adapters
- capture backend session references
- capture stdout and stderr
- surface pending mailbox questions
- write answers atomically
- resume existing work from persisted state

For backends that support native session continuation, the runtime should preserve the exact session reference needed for later resume.

For bootstrap and first-run setup, the current default target preference is:

- `pi` first
- then `codex`
- then `claude`
- then other discovered targets

### CLI output

Every operational command should support machine-readable output via `--json`.

The host prompt should be able to treat the CLI as a reliable command subsystem, not as prose that needs to be interpreted loosely.

Recommended host pattern:

- use `--json` for operational calls
- prefer `inspect` for single-task inspection
- prefer `list` for multi-task overview
- interpret intent in the host prompt, then call the matching command directly

## Artifact Contract

Each task should live under:

```text
.dispatch/tasks/<task-id>/
  task.json
  events.jsonl
  plan.md
  output.md
  context.md
  mailbox/
  outputs/
```

### Canonical artifacts

These files are runtime truth:

- `task.json`
- `events.jsonl`

`task.json` stores the current durable snapshot.

`events.jsonl` stores the append-only audit trail.

### Model-owned artifacts

These files are worker-owned execution artifacts:

- `plan.md`
- `output.md`
- `context.md`

The runtime may read them, but should avoid treating them as the canonical state machine.

### Mailbox contract

Mailbox files provide a simple worker-to-user handoff:

```text
mailbox/
  001.question
  001.answer
  001.done
  .done
```

Rules:

- questions are worker-initiated
- answers are written atomically
- unanswered questions must survive restarts
- `.done` marks task completion
- ordinary progress updates belong in `plan.md`, not in mailbox files

### Session and output artifacts

- runtime session storage stores backend-native session files outside the task artifact tree when a backend uses file-backed session paths
- `outputs/` stores stdout and stderr for each execution attempt

For file-backed session backends such as `pi`, session storage remains runtime-owned so those files do not leak into the workspace `.dispatch/` tree while still giving the runtime a stable session reference to resume.

This is why `resume` is task-centric instead of session-centric: the host resumes a task id, and the runtime reuses the correct backend session handle underneath.

These artifacts exist to support recovery, debugging, and auditability.

## Model vs Runtime Boundary

This boundary is the most important architectural rule in the project.

### Owned by the model

- understanding the user request
- deciding whether work is direct, plan-driven, or discussion-oriented
- decomposing work
- refining plans
- asking clarifying questions
- deciding how to continue after interruption
- keeping the plan artifact readable and useful to future turns

### Owned by the runtime

- task ids
- directory layout
- canonical status persistence
- event logging
- backend invocation
- session capture
- mailbox atomicity
- output capture
- restart and resume entry points

## Why This Differs From the Reference Project

The upstream `/dispatch` project is primarily a skill and protocol definition.

This repository keeps that prompt-first philosophy, but replaces host-specific tool assumptions with a local CLI runtime.

That means:

- prompts remain the source of intelligence
- the CLI becomes the source of action
- artifacts become the source of durable memory

## Implementation Guidance

When adding new features, prefer these questions:

1. Should this be a prompt rule?
2. Should this be a CLI command or flag?
3. Should this be a durable artifact or event?

Avoid putting model judgment into the runtime unless the logic is required for safety, persistence, or interoperability.

When deciding where a new rule belongs:

- if it changes how the host reasons about what to do next, prefer the dispatcher prompt
- if it changes how a worker records or advances work, prefer the worker prompt
- if it changes persistence, resume fidelity, or machine-readable state, prefer the CLI/runtime

## v1 Direction

For v1, prioritize:

- stable CLI commands
- stable JSON output
- stable artifact layout
- reliable mailbox semantics
- reliable session resume

Do not prioritize:

- a standalone UI
- a daemon architecture
- complex workflow DSLs
- heavyweight orchestration logic inside the runtime
