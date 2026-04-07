# Dispatch Reference Map

This file maps the original reference implementation to the new repository structure.

## Reference Project

Reference root:

- `/Users/devxiyang/code/tmp/dispatch`

## Reference to Local Mapping

- Reference skill prompt:
  `/Users/devxiyang/code/tmp/dispatch/skills/dispatch/SKILL.md`
  ->
  [prompts/dispatcher-system.md](/Users/devxiyang/code/sideproject/agent-dispatch/prompts/dispatcher-system.md)

- Reference worker prompt section:
  `/Users/devxiyang/code/tmp/dispatch/skills/dispatch/SKILL.md`
  ->
  [prompts/worker-template.md](/Users/devxiyang/code/sideproject/agent-dispatch/prompts/worker-template.md)

- Reference first-run, config, recovery, and mailbox-related notes:
  `/Users/devxiyang/code/tmp/dispatch/skills/dispatch/references/*.md`
  ->
  [docs/dispatch-flows.md](/Users/devxiyang/code/sideproject/agent-dispatch/docs/dispatch-flows.md)

- Reference architecture and README narrative:
  `/Users/devxiyang/code/tmp/dispatch/docs/*.md`
  ->
  [README.md](/Users/devxiyang/code/sideproject/agent-dispatch/README.md)
  plus the local runtime implementation in:
  [dispatch-core](/Users/devxiyang/code/sideproject/agent-dispatch/crates/dispatch-core/src/model.rs)
  and
  [dispatch-cli executor](/Users/devxiyang/code/sideproject/agent-dispatch/crates/dispatch-cli/src/executor.rs)

## Important Adaptations

The reference project is a Claude Code skill.

This project is not a skill-first implementation. It is:

- a Rust control plane
- backend adapters for multiple agent CLIs
- a `pi` host extension

That changes three things:

1. prompt text is stored as repository assets, not only embedded skill prose
2. durable state lives in `task.json` and `events.jsonl`, not only in checklist markdown
3. host interactions can come from `pi`, not only Claude Code

## What Is Still Missing

These prompt/spec files are now in the repo, but they are not yet fully wired into runtime code generation.

The next implementation step is to make:

- dispatcher prompt assets drive host behavior
- worker prompt template drive backend prompt construction
- config and mailbox specs drive runtime validation and recovery
