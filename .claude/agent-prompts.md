# Agent Prompts

When work is split across multiple Claude Code agents running in isolated git worktrees, dispatch prompts in this repo follow the structure below. Agents do not share state during execution, so each prompt must be self-contained, hand-off context must be explicit, and parallel agents must have hard boundaries to merge cleanly.

These prompts compose with the other gate docs in this directory:

- [`commits.md`](commits.md) — `make check` before every commit; `type(scope): description` format
- [`testing.md`](testing.md) — `make check` is the hard gate; every feature needs a test, every fix needs a regression test
- [`review-gate.md`](review-gate.md) — spawn a review agent before opening a PR
- [`pull-requests.md`](pull-requests.md) — open a draft PR and post the Agent Run Report comment

Every dispatched agent is expected to honour these. Verification sections in agent prompts should call `make check` rather than raw `cargo` invocations where the gate applies.

## Document structure

1. Title — `# Agent Prompts — <Project / Milestone>`
2. One-paragraph framing (worktree isolation, no shared state, read sequencing before dispatch)
3. **Sequencing Overview** — ASCII diagram showing serial vs parallel steps, plus rules ("Do not start Step N until Step M is merged to main", "parallel pairs use separate worktrees")
4. One `## Step N — Phase X: <Name>` section per agent
5. Sections separated by `---`

## Per-agent section

```
## Step N — Phase X: <Name>

**Branch:** `phase/<n>-<slug>`
**Depends on:** <which steps must be merged first; omit on Step 1>

**Prompt:**

You are implementing <X> of <project>. The full specification is in
`planning/<doc>.md` under "<Section>". Read that section carefully
before writing any code.

### Context
<Codebase state the agent will encounter: existing plugins/modules,
what currently works, file paths, behaviour to preserve. Be concrete.>

### Your Task
1. <Numbered, concrete deliverables>
2. <Reference spec sections by name; inline code blocks for anything
   the agent must produce verbatim — especially stubs that prevent
   merge conflicts in parallel work>
...

### Do Not Touch
- `src/<file>.rs` — <one-line reason: "Phase N's domain", "does not
  exist yet", "not your domain">
...

### Closing the Loop
When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against the
   spec section listed above.
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`,
   title matches the commit convention in `.claude/commits.md`).
4. Post the Agent Run Report comment combining
   `git log main..HEAD --oneline` with the review report.

### Verification
\`\`\`bash
make check
# → passes

cargo run -- <command>
# → <observable output>
\`\`\`
```

## Style rules

- Address the agent in second person ("You are implementing...", "Your job is to...").
- Be explicit about the boundary between this agent and parallel agents — call out shared files (e.g. `types.rs`) and pre-place stubs in the **earliest** agent so parallel agents don't collide on them.
- Verification blocks use real shell commands and inline `→` arrows for expected observable output.
- Always include a `make check` line in verification — the testing/commit gate applies to every agent.
- Mention determinism / seed behaviour where relevant.
- For parallel agents, both must be able to merge cleanly without coordinating mid-flight — design the task split accordingly.
- Do not duplicate the contents of `commits.md`, `testing.md`, `review-gate.md`, or `pull-requests.md` into the prompt. Reference them by path so the agent can read them directly.

## When to skip parts

- If work is genuinely linear with no parallelism, drop the sequencing diagram but keep the per-agent section structure.
- If a step is exploratory rather than implementing a spec, the "specification is in `planning/...`" opener can be replaced with a direct problem statement, but Context / Your Task / Do Not Touch / Closing the Loop / Verification still apply.
