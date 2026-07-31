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
- [ ] **repo** (`src/repo.rs`): `resolve_repo`, `copy_home_to_repo`,
      `git(cmd, args)` shell-out helper running in `repo_path`, `add_all`,
      `staged_diff_quiet`, `staged_diff`, `commit`, `fetch`, `pull_rebase_theirs`,
      `push`, `head_short_sha`. Errors via `thiserror`. Never panics.
      Commit: `phase 2: repo — git shell-out + copy`.
- [ ] **scan** (`src/scan.rs`): `Finding { file, line, kind, snippet }`,
      `scan(repo_path, &Config, full: bool) -> Vec<Finding>`. Prefixes + entropy
      (hand-rolled Shannon) + regex. `full=true` also dispatches to
      `gitleaks`/`trufflehog` if present (probe PATH once, cache). `sync` calls
      with `full=false` (regex only). Print helper + exit-code helper (0/2).
      Commit: `phase 2: scan — secret detection engine`.
- [ ] **install** (`src/install.rs` + `skill/SKILL.md`): full SKILL.md content
      (per DECISIONS + the agent flow w/ 4-layer review). `install-skill`
      detects `~/.claude`, `~/.config/opencode`, `~/.agents` and writes
      `<dir>/skills/tide/SKILL.md` for each found (or `--agent` filter). Uses
      `include_str!("../skill/SKILL.md")`. Commit: `phase 2: install — SKILL.md`.

### Phase 3 — parallel batch (disjoint files)  — branch off integration
- [ ] **diff** (`src/diff.rs`): copy watched → repo, `git add -A`, print
      `git diff --cached`. Exit 0. No commit/push. Commit: `phase 3: diff`.
- [ ] **doctor** (`src/doctor.rs`): run all checks; print status table; exit
      non-zero on blocker. Commit: `phase 3: doctor`.

### Phase 4 — sequential  — branch off integration
- [ ] **sync** (`src/sync.rs`): the core flow (copy → add → quiet-check → regex
      gate → commit → fetch → pull --rebase -X theirs → push). Prints the result
      line; exit codes per DECISIONS. Commit: `phase 4: sync`.

### Phase 5 — docs + tests  — branch off integration
- [ ] `README.md`: what/install/usage/config/secret model.
- [ ] `tests/integration.rs`: temp `HOME`, temp bare git remote, `tide init`,
      `tide add ~/.bashrc`, edit it, `tide scan` clean, `tide sync`, assert a
      commit exists and the bare remote received it; plus a secret-in-file case
      asserting `tide scan` exits 2 and `tide sync` refuses.
- [ ] GATE green. Commit: `phase 5: docs + tests`.

## Status legend
`[ ]` pending · `[~]` in progress · `[x]` done (committed, gate green)
