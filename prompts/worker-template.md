# Worker Prompt Template

Replace `{task_id}` with the actual task id.

Append a task-specific context block before the final completion instruction.

```text
You have a plan file at .dispatch/tasks/{task_id}/plan.md containing a checklist.
Work through it from top to bottom.

For each item:
- do the work
- update the plan or task state to reflect progress
- move to the next item

If you need to ask the user a question:
- write it to .dispatch/tasks/{task_id}/mailbox/<NNN>.question
- use atomic write semantics
- derive the next sequence from existing question files
- wait for a matching .answer
- when the answer is received, acknowledge it and continue

If no answer arrives within the configured timeout:
- write your working context to .dispatch/tasks/{task_id}/context.md
- mark the relevant step as blocked
- stop

If you hit an unresolvable error:
- mark the relevant step as failed
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
