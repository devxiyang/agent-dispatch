# Worker Prompt Template

Replace `{task_id}` with the actual task id.

Append a task-specific context block before the final completion instruction.

```text
You have a plan file at .dispatch/tasks/{task_id}/plan.md.
It may already contain a checklist, or it may start as a task brief that you need to turn into a working plan.

Before doing substantial work:
- read the current plan artifact
- if it is not yet a useful working plan, turn it into one
- keep the artifact updated as your working notes and checklist evolve

As you work:
- do the work
- update the plan artifact to reflect progress when useful
- move to the next step you judge is appropriate

If you need to ask the user a question:
- write it to .dispatch/tasks/{task_id}/mailbox/<NNN>.question
- use atomic write semantics
- derive the next sequence from existing question files
- wait for a matching .answer
- when the answer is received, acknowledge it and continue

If no answer arrives within the configured timeout:
- write your working context to .dispatch/tasks/{task_id}/context.md
- note the blockage clearly in the plan artifact
- stop

If you hit an unresolvable error:
- note the failure clearly in the plan artifact
- record the error clearly
- stop

When all items are complete:
- mark the task complete
- write the final output artifact if requested
- write the completion marker
```

## Context Block Rules

The dispatcher should add a task-specific context block that:

- states the requested outcome in the user's terms
- points to reference files or artifacts the worker should read
- states constraints the worker must respect
- does not teach tools
- does not over-specify implementation

## Example Context Block

```text
Context:
- Outcome: review the auth module and list concrete risks.
- References: read src/auth/* and the current task plan before acting.
- Constraints: produce findings only; do not edit source.
```

## Mailbox Rules

- Mailbox traffic is worker-initiated only.
- Use `.question`, `.answer`, `.done` sequence files.
- Use atomic writes:
  write to `*.tmp`, then rename into place.

## Completion Rules

- Completion means the task state is durable, not just that the model replied.
- If a final artifact is part of the plan, write it before marking done.
