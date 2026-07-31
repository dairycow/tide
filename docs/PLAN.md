# tide — Plan / Checklist

The decisions/rules live in `docs/DECISIONS.md` — read that first.
Tick a box `[ ]` → `[x]` in the SAME commit that completes its task.

## Phases

### Phase 1 — scaffold (sequential foundation)  — base branch
- [x] `Cargo.toml` rewritten with the full dep list (see DECISIONS).
- [x] `src/main.rs`: `tracing_subscriber` init + `clap` derive `Cli` with one
      subcommand per command in the table. Each arm calls `<module>::run(args)`.
      Declare `mod config; mod repo; mod scan; mod diff; mod sync; mod doctor;
      mod install;`.
- [x] `src/config.rs`: `Config` struct (serde), load/save to
      `~/.config/tide/tide.toml`, tilde + `$HOME` expansion, `Watches { source,
      target }`. `init`, `add`, `rm`, `list` fully implemented here (these own
      config mutation). Path-mapping helper for `source → target`.
- [x] STUB every other module (`repo.rs`, `scan.rs`, `diff.rs`, `sync.rs`,
      `doctor.rs`, `install.rs`) with `pub fn run(...) -> anyhow::Result<()> {
      todo!("phase <N>") }` matching the signatures `main.rs` calls.
- [x] `skill/SKILL.md` placeholder (1-line) so `include_str!` compiles; install
      worker fills it later. (Or use `include_str!` guarded — scaffold's choice,
      but it must COMPILE.)
- GATE: `cargo build --release && cargo test` green. Commit: `phase 1: scaffold`.

### Phase 2 — parallel batch (disjoint files)  — branch off integration
- [x] **repo** (`src/repo.rs`): git shell-out + copy layer (12 public fns);
      hardened against path-traversal targets after reviewer FIX_FIRST.
      Commit: `phase 2: repo — git shell-out + copy` + `… harden target path validation`.
- [x] **scan** (`src/scan.rs`): `Finding` + `scan_text`; prefixes + entropy
      (Shannon) + regex + external (gitleaks/trufflehog if present); hardened
      after reviewer FIX_FIRST (no panic on bad config, invalid patterns
      hard-error in run(), sk-proj/sk-ant, full short-secret masking).
      Commit: `phase 2: scan — secret detection engine` + `… harden config handling and redaction`.
- [x] **install** (`src/install.rs` + `skill/SKILL.md`): full SKILL.md (4-layer
      flow) + `install-skill` writing to `~/.claude` / `~/.config/opencode` /
      `~/.agents` skill dirs. Commit: `phase 2: install — SKILL.md`.

### Phase 3 — (diff sequential after scan; doctor parallel) — branch off integration
- [x] **diff** (`src/diff.rs`): copy → add → print staged diff, no commit/push.
      Commit: `phase 3: diff`.
- [x] **doctor** (`src/doctor.rs`): 7 checks + blocker exit semantics.
      Commit: `phase 2: doctor`.

### Phase 4 — sequential  — branch off integration
- [x] **sync** (`src/sync.rs`): copy → add → quiet-check → secret gate
      (prefix+entropy+regex, external=false) → commit → fetch →
      pull --rebase -X theirs → push (best-effort); reviewer PROCEED, no findings.
      Commit: `phase 4: sync`.

### Phase 5 — docs + tests  — branch off integration
- [x] `README.md`: what/install/usage/config/secret model.
- [x] `tests/integration.rs`: happy-path push to a local bare remote + secret
      block refusing push (2 e2e tests, passing).
- [x] GATE green. Commit: `phase 5: docs + tests`.

## Completion

All phases done on `tide-impl`. Final gate: `cargo build --release` + `cargo
test` (15 tests) + `cargo clippy --all-targets -- -D warnings` all GREEN.
Orchestrator end-to-end smoke against a throwaway HOME + local bare remote
confirmed: clean sync pushes to remote; a `ghp_` secret is blocked (scan & sync
exit 2) and never pushed. Known minor follow-up: a malformed `tide.toml` is
reported redundantly and exits 1 (not 2); non-blocking.

## Status legend
`[ ]` pending · `[~]` in progress · `[x]` done (committed, gate green)
