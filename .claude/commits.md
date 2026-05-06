# Commits

**Gate:** `make check` must pass before every commit. No exceptions.

**Auto-commit:** Commit each logical change as it is completed, without waiting to be asked. Use judgment to determine when a change is coherent and complete — do not commit mid-feature or bundle unrelated changes.

**Message format:** `type(scope): short description`

- `type` — `feat`, `fix`, `refactor`, `test`, `docs`, `chore`
- `scope` — the plugin or module: `types`, `world`, `behaviour`, `artifacts`, `clock`, `output`, `provenance`, `chaos`, `persistence`, `llm`, `cli`, `scenario`
- Description — imperative, lowercase, no period. 72 characters total max.

```
feat(world): add spawn_org system with archetype-driven defaults
fix(artifacts): route ticket intents to correct artifact type
test(world): assert WorldActors indexes by role and department
refactor(types): move WorldActors and WorldWorkItems to types.rs
```

**Scope:** One logical change per commit. Don't bundle unrelated fixes.

**Don't commit:** `artifacts.json`, `provenance.json`, `data/` — runtime outputs covered by `.gitignore`.
