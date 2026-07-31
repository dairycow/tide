# Contributing to tide

Thanks for your interest in improving `tide`! This is a small, focused CLI — the
design decisions and the "do not invent more commands" rule live in
[`docs/DECISIONS.md`](docs/DECISIONS.md). Please read it before proposing changes
to the command surface or config schema.

## Development setup

Requires **Rust 1.88+** (edition 2024; uses let-chains) and `git` on your PATH.

```bash
git clone https://github.com/dairycow/tide.git
cd tide
cargo build
```

## The verification gate

Before you commit, **all three** of these must be green:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

CI runs the same. Never weaken a test or gate to make a number pass.

## Conventions

- **One logical change per commit.** Keep commits focused and reviewable.
- **Commit messages** are imperative, e.g. `scan: add glpat- prefix detection`.
- **Secrets:** never commit a real credential. The repo's own scan engine
  (`tide scan`) exists for exactly this — if you add example secrets in tests,
  mark them obviously (e.g. `EXAMPLE_NOT_REAL`).
- **Command surface:** do not add new subcommands without first updating
  `docs/DECISIONS.md`. The surface is intentionally small.
- **Tests:** add unit tests under the relevant module's `#[cfg(test)]` block and,
  where useful, an end-to-end test in `tests/integration.rs`.

## Licensing

Contributions are welcome and gladly accepted. By submitting a contribution you
agree it is dual-licensed under the same terms as the project — **MIT or
Apache-2.0**, at the recipient's option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
