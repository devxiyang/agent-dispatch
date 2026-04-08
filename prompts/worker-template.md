# Worker Prompt Template

Replace `{task_id}` with the actual task id.

Append a task-specific context block before the final completion instruction.

```text
You have a plan file at .dispatch/tasks/{task_id}/plan.md.
It may already contain a checklist, or it may start as a task brief that you need to turn into a working plan.

Before doing substantial work:
- read the current plan artifact
- if it is not yet a useful working plan, turn it into one
- if a checklist is useful, prefer concrete, verifiable items rather than vague notes
- keep the artifact updated as your working notes and checklist evolve

As you work:
- do the work
- update the plan artifact to reflect meaningful progress
- if you are using checklist markers, keep them honest:
  `[ ]` pending, `[x]` done, `[?]` blocked, `[!]` failed
- move to the next step you judge is appropriate
- prefer leaving behind a readable working record rather than silent progress

If you need to ask the user a question:
- write it to .dispatch/tasks/{task_id}/mailbox/<NNN>.question
- use atomic write semantics
- derive the next sequence from existing question files
- wait for a matching .answer
- when the answer is received, acknowledge it and continue
- do not write unsolicited mailbox files for status updates or extra notes

If no answer arrives within the configured timeout:
- write your working context to .dispatch/tasks/{task_id}/context.md
- note the blockage clearly in the plan artifact, including the missing answer or dependency
- stop

If you hit an unresolvable error:
- note the failure clearly in the plan artifact
- record the error clearly enough that a later worker can recover
- stop

When all items are complete:
- make sure the plan artifact reflects the completed state
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
- The mailbox is for clarifying questions, not for ordinary progress reporting.

## Progress Rules

- Prefer progressing the existing plan artifact over creating separate scratch files.
- If the plan already has a clear order, work roughly top to bottom unless there is a good reason not to.
- If you change the plan materially, leave the artifact in a state that another worker could pick up later.
- If the user or dispatcher provided a resume note, incorporate it into the next relevant step instead of ignoring it.

## Completion Rules

- Completion means the task state is durable, not just that the model replied.
- If a final artifact is part of the plan, write it before marking done.
- Do not mark completion until the relevant plan state, output artifact, and completion marker all agree.
