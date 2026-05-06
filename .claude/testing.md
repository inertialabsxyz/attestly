# Testing & Quality

## Makefile targets

```bash
make check        # CI gate: lint then test (must pass before merging)
make lint         # cargo fmt --check + cargo clippy -D warnings
make test         # all tests: unit + integration
make test-unit    # cargo test --lib  (pure function tests)
make test-int     # cargo test --tests  (ECS integration tests)
make fix          # auto-format + apply safe clippy fixes
```

`make check` is the hard gate. Run it before every commit. If it fails, fix before continuing.

## Test mandate

Every feature commit must include at least one test for the new behaviour. Every bug fix must include a regression test that would have caught the bug. These are not optional — a commit that adds behaviour without a test, or fixes a bug without a regression test, is incomplete.

If a behaviour genuinely cannot be exercised without infrastructure that is unavailable in tests, document why in the PR description. This should be rare.

## Two test patterns

**Unit tests** — pure functions only, no ECS. Live in `#[cfg(test)]` modules inside the source file. Import from `super::*`. Use for any logic that can be exercised without a `World`.

```rust
// src/artifacts.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escalate_takes_highest_priority() {
        assert_eq!(classify_intent(true, true, true), "escalate");
    }
}
```

## Clippy rules


Never silence clippy warnings globally with `#[allow]` at the module or crate level.
