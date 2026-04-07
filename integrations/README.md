# Integrations

This directory contains optional host adapters for `dispatch-cli`.

The core product surface is:

- prompts
- `dispatch-cli`
- task artifacts under `.dispatch/`

Anything under `integrations/` should be treated as a convenience layer for a specific host environment, not as the architectural center of the project.

## Rules

An integration should:

- call `dispatch-cli` rather than reimplement runtime behavior
- prefer `dispatch-cli --json` for operational calls
- keep user conversation in the host
- treat task artifacts as runtime-owned files and `dispatch-cli` as the action surface

An integration should not:

- redefine CLI semantics
- bypass the JSON envelope contract
- become the source of truth for task state

## Current Adapters

- `pi-dispatch-host`
  Example adapter for `pi`.

## Adding Another Host

If you add another host adapter, keep it thin:

1. translate host commands into `dispatch-cli` invocations
2. read structured output
3. render host-native status or UI affordances
4. avoid moving durable orchestration logic into the adapter

For the host-neutral integration rules, see
[host-integration-contract.md](../docs/host-integration-contract.md).
