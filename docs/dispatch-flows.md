# Dispatch Flows

This file lands the full operational flow from the reference dispatch project into `agent-dispatch`.

## Source Reference

Primary source:

- `skills/dispatch/SKILL.md` in the upstream `dispatch` reference repo

Supporting sources:

- `skills/dispatch/references/first-run-setup.md` in the upstream reference repo
- `skills/dispatch/references/config-modification.md` in the upstream reference repo
- `skills/dispatch/references/proactive-recovery.md` in the upstream reference repo
- `skills/dispatch/references/ipc-protocol.md` in the upstream reference repo

## First-Run Setup Flow

Trigger:

- no dispatch config exists

Steps:

1. detect installed CLIs
2. discover models
3. choose a sensible default
4. generate config
5. continue with the original request

CLI detection targets:

- `agent`
- `claude`
- `codex`
- `pi`

Model discovery policy:

- prefer native discovery commands when available
- use backend preference rules when multiple CLIs expose overlapping models
- group all discovered models into the generated config

## Config Modification Flow

Trigger:

- user requests config changes instead of task execution

Supported operations:

- add model
- remove model
- change default
- add alias
- repair backend mapping

Rule:

- stop after config is updated
- do not continue into task dispatch automatically unless the user asked for both

## Task Dispatch Flow

1. resolve backend and model
2. parse directives
3. create task plan
4. persist task state
5. build backend invocation
6. launch worker
7. capture stdout and stderr
8. capture or persist session reference
9. report task id and summary
10. return control immediately

## Mailbox Flow

Mailbox traffic is worker-initiated only.

Directory:

```text
.dispatch/tasks/<task-id>/mailbox/
  001.question
  001.answer
  001.done
  .done
```

Rules:

- dispatcher must not write unsolicited mailbox files
- answers are atomic writes
- unanswered questions must be detected on restart

## Fallback Recovery Flow

Trigger:

- worker timed out waiting for mailbox answer
- worker exited after marking `[?]`

Steps:

1. read the blocked step
2. read `context.md` if present
3. ask the user for the missing input
4. start a new worker
5. inject the saved context and answer
6. continue from the blocked step

## Proactive Recovery Flow

Trigger:

- worker failed to start
- CLI disappeared
- auth failed
- quota failed

Steps:

1. verify current CLI availability
2. determine which alternatives still exist
3. present a compatible replacement
4. update config if the replacement should persist
5. re-dispatch

## Progress Reporting Flow

When the user asks for status:

1. read canonical task state
2. read plan markers
3. read recent events
4. check whether there is a pending mailbox question
5. summarize only what matters

## Parallel and Sequential Work

Parallel:

- separate task ids
- separate session refs
- separate outputs

Sequential:

- task B waits until task A completes
- do not merge dependent tasks unless the dependency is trivial

## What Must Be Durable

At minimum:

- task metadata
- backend selection
- execution mode
- session reference
- last invocation
- event log
- stdout/stderr artifacts

This is the durable minimum needed to recover after host restarts.
