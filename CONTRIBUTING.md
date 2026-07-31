# Contributing to Nova

Thank you for helping build Nova. The project is early, so clear problem statements and small,
well-tested changes are especially valuable.

## Before coding

- Search existing issues and discussions.
- Open an issue before substantial API, storage-format, or architecture work. Accepted proposals
  should be captured through the process in [`docs/rfcs`](docs/rfcs/README.md).
- Keep pull requests focused on one concern.
- Do not include generated build output or local database files.

## Local checks

Run these before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Add tests for bug fixes and observable behavior changes. Changes to durable formats must include
recovery and corruption tests and explain compatibility implications.

## Commits and pull requests

Use clear, imperative commit subjects. In the pull request, explain the problem, the chosen
approach, test evidence, and any performance or compatibility impact.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
