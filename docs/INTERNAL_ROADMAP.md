# Internal roadmap

Working notes for in-progress and near-term development on the `dev` branch. This file tracks
granular, fast-moving implementation tasks that don't belong in the public-facing `ROADMAP.md`
(which is organized by capability milestone and exit criteria for external contributors).

## Milestone 1 — durable single-node vertical slice (extended scope)

### Tooling and developer experience (added)

- [x] `nova-server` startup banner — version, listen address, PID, and data dir shown on launch
- [x] `nova-cli` interactive REPL mode (connect once, issue multiple commands, like `redis-cli`)
- [x] `nova-cli` colorized, pretty-printed responses (tables for `RANGE`, key/value for `INFO`)
- [ ] `nova-cli` command history and line editing (currently plain stdin, no arrow-key recall)
- [ ] `nova-cli` shorthand `-h`/`-p` host and port flags to match common client conventions

### Still open from the original slice checklist (see `ROADMAP.md`)

- [ ] End-to-end tests that launch the server and exercise real client connections
- [ ] Clean shutdown with a final durability barrier
- [ ] Torn/truncated WAL-tail detection and safe repair
- [ ] Maximum frame, metric, request, and connection limits
- [x] Duplicate timestamp and out-of-order write semantics — defined as an upsert keyed on
  `(metric, timestamp)` with no ordering requirement; documented on `Engine::write`/`range` and
  in the README protocol section, and locked in with three new engine tests covering duplicate
  overwrite, out-of-order insertion, and both surviving a WAL-replay restart
- [ ] Batch write command with atomicity rules
- [ ] Config file plus environment and command-line precedence
- [x] Structured error codes rather than human-text-only errors — `ParseError`, `EngineError`,
  and `WalError` each expose a `code()` method returning a stable `SCREAMING_SNAKE_CASE`
  identifier (`UNKNOWN_COMMAND`, `WRONG_ARITY`, `INVALID_METRIC`, `INVALID_NUMBER`,
  `INVALID_RANGE`, `CORRUPT`, `IO`); `nova-server` now replies `ERR <CODE> <message>` instead of
  `ERR <message>`, documented in the README protocol section, with a `code()` test per crate
- [x] Version and build metadata in `INFO` — `INFO` now reports `version`, `git` (short SHA via
  new `crates/server/build.rs`), `rustc`, and `profile`, in addition to `metrics`/`points`; the
  startup banner shows the same `git`/`profile` pair
- [ ] Graceful resource exhaustion behavior

## Notes

- This file is for internal tracking only and can be edited freely without going through the same
  bar as `ROADMAP.md`.
- Promote an item to `ROADMAP.md` once it's stable enough to be a public commitment.
