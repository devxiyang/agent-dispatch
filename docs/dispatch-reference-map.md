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
- host-neutral prompt assets plus optional host adapters

That changes three things:

1. prompt text is stored as repository assets, not only embedded skill prose
2. durable state lives in `task.json` and `events.jsonl`, not only in checklist markdown
3. host interactions can come from any calling agent that can invoke `dispatch-cli --json`

## Current State

These prompt/spec files are now wired into the current implementation:

- dispatcher behavior is documented in
  [prompts/dispatcher-system.md](../prompts/dispatcher-system.md)
- worker prompt construction is driven by
  [prompts/worker-template.md](../prompts/worker-template.md)
- runtime durability is implemented in
  [dispatch-core](../crates/dispatch-core/src/model.rs)
  and
  [dispatch-cli runtime](../crates/dispatch-cli/src/runtime.rs)

The remaining work is no longer about importing the reference structure.
It is about refining behavior, validation, and host ergonomics within the prompt + CLI architecture.
