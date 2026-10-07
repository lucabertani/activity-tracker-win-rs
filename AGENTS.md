# AGENTS.md

## Rust quality gates

In this project, after any Rust change always run both commands:

```bash
cargo clippy -- -D warnings
cargo fmt --all -- --check
```

If either command reports problems, they **must be fixed** (re-run until both
are clean). Do not commit or hand off code while clippy or fmt is failing.
