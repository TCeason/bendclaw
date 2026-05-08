---
name: harden
description: Stress-test a proposed strategy, plan, or current git changes by hunting loopholes before implementation or commit. Trigger phrases: harden, stress test, poke holes, find loopholes.
---

# Harden

Stress-test a proposed strategy, design, or plan by actively hunting loopholes
and iterating fixes until the remaining risks are either closed or explicitly
accepted.

## When to Use

- User asks you to validate, pressure-test, or "poke holes in" a plan,
  strategy, or conclusion before coding starts. For a bare `/harden`, treat the
  immediately preceding plan/conclusion as the primary subject.
- User explicitly asks you to harden current git changes before commit or merge. In this
  case, inspect the diff to infer the implementation strategy, then harden that
  strategy.
- You have a proposed approach but suspect gaps and want a disciplined pass
  before committing.
- Do NOT use this as a line-by-line code review of diffs or PRs — use the
  `review` skill for that. Harden evaluates whether the strategy behind the
  change is robust.
- Do NOT use this as a default reflex after every small change. Invoke it when
  the blast radius of being wrong is high.

## Inputs

The subject of hardening is one of:

- the currently-proposed plan, strategy, or conclusion from the previous turn,
  plan mode, or the user's message. This is the default subject for a bare
  `/harden` request;
- the current git changes, when the user explicitly asks to harden local work
  (for example, `harden changes` or `/harden changes`). Inspect staged and
  unstaged diffs, summarize the inferred strategy, then harden that strategy
  rather than reviewing every changed line;
- when a plan/strategy/conclusion is available and local git changes also exist,
  use the diff only as supporting context. Combine relevant findings from the
  diff with the hardening pass, but do not switch the primary subject from the
  previous conclusion to the diff.

If no plan, strategy, or git change is available, ask the user to state the
subject in one paragraph before starting.

## The Loop

Repeat until convergence:

1. **Enumerate loopholes.** List concrete, named weaknesses. Each item must say
   *what* breaks and *under what condition*. Reject vague worries. Cover at
   least:
   - Edge cases: empty input, max size, concurrency, partial failure, retries,
     migration from existing state.
   - Assumptions: behaviors you inferred without reading the code. Mark them.
   - Integration: conventions in the codebase the plan bypasses or duplicates.
   - Verification: tests or checks that would not actually catch a regression.
   - Reversibility: what's hard to undo if this ships and is wrong.
2. **Fix or accept.** For each loophole, either:
   - update the plan or strategy with a specific change;
   - if hardening current git changes, identify the specific implementation
     adjustment needed; or
   - explicitly mark it as accepted risk with the reason (scope, cost, low
     probability).
   Do not close a loophole by waving at it.
3. **Re-check.** Look at the updated plan and ask: did the fixes introduce new
   loopholes? If yes, go to step 1. If no, stop.

Typical runs converge in 2-3 iterations. If you're past 4 iterations and still
finding substantive new loopholes, the underlying design is probably wrong —
surface that to the user instead of patching further.

## Anti-patterns

Hardening fails when it degrades into these:

- **Fallback sprawl.** Adding normalizers, retries, and defensive wrappers for
  scenarios that cannot happen. A plan is not safer because it handles more
  imaginary cases.
- **Abstraction creep.** Introducing new layers or indirection "just in case".
- **Vague confidence claims.** "I'm now confident" is not a stopping condition.
  The stopping condition is: remaining loopholes are fixed or named as risks.
- **Scope drift.** Hardening the plan should not grow the feature. If a
  loophole reveals the feature is too small, say so; don't silently expand it.

## Output

When you converge, present:

- **Subject** — the plan, strategy, or current git changes being hardened.
- **Updated direction** — the revised plan or the implementation adjustments
  needed for current changes.
- **Closed loopholes** — each one as: `condition → fix`, one line.
- **Accepted risks** — each one as: `condition → why it's acceptable`, one line.
- **Iterations** — a short count (e.g. "converged after 2 passes").

Keep it compact. One page is usually enough.
