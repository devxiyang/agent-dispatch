# Dispatch Reference Map

This file maps the original reference implementation to the new repository structure.

## Reference Project

Reference root:

- upstream `dispatch` repository clone

## Reference to Local Mapping

- Reference skill prompt:
  `skills/dispatch/SKILL.md` in the upstream reference repo
  ->
  [prompts/dispatcher-system.md](../prompts/dispatcher-system.md)

- Reference worker prompt section:
  `skills/dispatch/SKILL.md` in the upstream reference repo
  ->
  [prompts/worker-template.md](../prompts/worker-template.md)

- Reference first-run, config, recovery, and mailbox-related notes:
  `skills/dispatch/references/*.md` in the upstream reference repo
  ->
  [docs/dispatch-flows.md](dispatch-flows.md)

- Reference architecture and README narrative:
  `docs/*.md` in the upstream reference repo
  ->
  [README.md](../README.md)
  plus the local runtime implementation in:
  [dispatch-core](../crates/dispatch-core/src/model.rs)
  and
  [dispatch-cli executor](../crates/dispatch-cli/src/executor.rs)

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
