# tide — Decisions & Rules (source of truth)

Read this FIRST. Every worker must conform. The plan/checklist lives in
`docs/PLAN.md`.

## What tide is

A **single Rust CLI binary**, invoked explicitly by a user or a coding agent
after dotfile edits. It detects which watched dotfiles changed, copies them into
its own git repo, and commits + pushes. **No daemon, no filesystem watcher, no
chezmoi, no background service.** One-shot.

## Locked-in design decisions

1. **One-shot CLI.** No `tide run` loop, no systemd, no notify/inotify deps.
   Detection happens at sync time by copying home files into the repo and
   letting `git` detect changes (`git diff --cached --quiet`).
2. **Copy-on-change tracking** (NOT symlinks). Canonical copies live in the tide
   repo; home files stay as real files. Robust against apps that rewrite-by-rename.
3. **tide owns its own git repo** (default `~/.local/share/tide/repo`,
   configurable). NOT a bare repo in `$HOME`, NOT chezmoi.
4. **Git operations via shell-out** (`std::process::Command` calling `git`),
   reusing the user's SSH keys / credential helpers. Do NOT use the `git2` crate.
5. **Conflict policy:** `git pull --rebase -X theirs` — **local changes win**.
6. **Auto-push** is on by default; toggle via config `auto_push = false`.
7. **Secret defense is 4 layers** (see `## Secret defense` below).

## Commands (the full surface — do not invent more)

| Command | Behavior |
|---|---|
| `tide init` | Create/merge `~/.config/tide/tide.toml` + `git init` the repo (**idempotent**: preserves existing watches/config; only re-sets the remote; `origin` is set-or-updated). Prompt for remote URL (`origin`), prefilled from the `gh` CLI as `<owner>/dotfiles` (tide is a dotfile tool — the cwd basename collided with the tool's own source repo when run inside it; gh's configured protocol); press enter to accept. If gh is authenticated and the URL points at github.com, offer to create a private repo there (opt-in). All gh inference is non-fatal — missing/unauthenticated `gh` falls back to an empty prompt. Then **detect common dotfiles** under `$HOME` and offer to adopt them (`Add all? [y/N]`, default no; conservative allowlist + secret denylist; `chezmoi`-free: just `tide add` each). |
| `tide add <path>` | Register a home path as watched. Copy it into the repo now (compute `target`). Append to watch list in config. |
| `tide rm <path>` | Unregister a path (leave the home file and the repo copy as-is). |
| `tide list` | Print watched dotfile paths (one per line). |
| `tide diff` | Copy all watched home files → repo, `git add -A`, print `git diff --cached`. **NO commit, NO push.** |
| `tide sync` | Copy all watched home files → repo. `git add -A`. If `git diff --cached --quiet` → "nothing to sync", stop (exit 0). Else run the **full secret gate** (prefixes + entropy + regex + external `gitleaks`/`trufflehog` if on PATH — the former `tide scan` engine, folded in as of v0.4.0); on hit → `git reset`, warn, abort (exit 2), **nothing pushed**. Else commit, fetch, `pull --rebase -X theirs`, push (if `auto_push`). Print machine-friendly result. |
| `tide doctor` | Report: tide binary, repo exists + is git repo, `origin` **URL**, ssh key / credential helper present, watched file count, whether `gitleaks`/`trufflehog` present. Exit non-zero if a blocker found. |
| `tide install-skill` | Copy the embedded `SKILL.md` (`include_str!`) into `~/.agents/skills/tide/` — the cross-tool skill root. |

`install-skill` targets only `~/.agents/` (read by opencode, Grok, Codex, Gemini).
Claude Code does not yet read `~/.agents/` (anthropics/claude-code#66352); it will
not auto-discover the skill until that lands.

## Config schema (`~/.config/tide/tide.toml`)

```toml
repo_path        = "~/.local/share/tide/repo"     # tilde-expanded
remote           = "git@github.com:user/dotfiles.git"
auto_push        = true
secret_patterns  = ["(?i)(token|password|secret|api[_-]?key)\\s*="]
entropy_threshold   = 4.5      # bits/char; tunable
entropy_min_length  = 20       # min run length to consider
# watch list EMPTY by default; populated by `tide add`
[[watches]]
source = "~/.bashrc"
target = "bashrc"
```

All fields optional except `repo_path` (defaulted if missing). Tilde (`~`) and
`$HOME` must be expanded. Missing config file → `tide init` is required.

## Path-mapping rule (source → target)

`target = source made relative to $HOME, then strip one leading '.' from each
path segment`.

- `~/.bashrc` → `bashrc`
- `~/.config/nvim/init.lua` → `config/nvim/init.lua`
- `~/.tmux.conf` → `tmux.conf`

If `source` is not under `$HOME`, `target` is `source` with leading `/` and `.`
stripped (best-effort); `tide add` prints the computed `target` for confirmation.

## Secret defense (3 layers)

1. **Agent semantic review** of `tide diff` — documented in `SKILL.md`, not in
   the binary. Catches judgment calls no pattern matches. Run `tide diff` and
   eyeball it before syncing.
2. **`tide sync` full gate** — the engine in `scan.rs`, run over staged content
   before any commit/push (this absorbs the former `tide scan` command, v0.4.0):
   - **Known prefixes** (compiled once): `AKIA`, `ghp_`, `gho_`, `ghu_`, `ghs_`,
     `ghr_`, `sk-`, `xox[bpoa]-`, `glpat-`, `AIza`, `eyJ` (JWT), and the literal
     `-----BEGIN ... PRIVATE KEY-----`.
   - **High-entropy** runs: tokens matching `[A-Za-z0-9+/=_-]{N,}` (N =
     `entropy_min_length`) with Shannon entropy > `entropy_threshold` bits/char.
   - **Regex** from config `secret_patterns`.
   - **External scanner** — if `gitleaks` or `trufflehog` is on PATH, `tide sync`
     shells out to it over the repo and merges findings. Skips silently if absent.
   On any hit → unstages, aborts (exit 2), **nothing pushed**.
3. **Conflict policy** — `git pull --rebase -X theirs` so **local wins** on conflict.

`scan.rs` is **one shared engine**, now invoked only by `tide sync`'s gate (and
exercised by its own unit tests — `scan_text` stays `pub`). Findings shape:
`{ file, line, kind, snippet }`. Truncate `snippet` to ~40 chars and redact the
middle of obvious secrets before printing.

## Output contract (agent-friendly)

- Every command prints concise, parseable lines to stdout.
- Exit codes: `0` success/no-op/clean; `2` secret finding / block /
  user-fixable error; `1` unexpected failure.
- `tide sync` success line includes: files synced, commit short sha, `pushed=yes|no`.

## Verification gate (the single gate — never commit red)

```
cargo build --release && cargo test
```

Both must succeed before any worker commits. `cargo clippy -- -D warnings` should
also pass (treat as part of the gate where feasible).

## File ownership map (disjoint files — workers must NOT touch files they don't own)

| File | Owned by phase |
|---|---|
| `Cargo.toml` | scaffold only (full dep list up front; no later worker edits it) |
| `src/main.rs` | scaffold only (declares all `mod`s + clap arms calling each module's `pub fn run(...)`) |
| `src/config.rs` | scaffold |
| `src/repo.rs` | repo worker |
| `src/scan.rs` | scan worker |
| `src/diff.rs` | diff worker |
| `src/sync.rs` | sync worker |
| `src/doctor.rs` | doctor worker |
| `src/install.rs` | install worker |
| `skill/SKILL.md` | install worker |
| `README.md` | docs worker |
| `tests/integration.rs` | docs worker |

**Critical:** the scaffold worker creates a STUB for every module file
(`repo.rs`, `scan.rs`, `diff.rs`, `sync.rs`, `doctor.rs`, `install.rs`) so the
module tree compiles. Each stub exposes the public API the clap arm calls
(`pub fn run(...) -> anyhow::Result<()> { todo!("phase <N>") }`). Later workers
EXPAND their assigned stub in place — they do not edit `main.rs` or `Cargo.toml`.

## `Cargo.toml` — the full dependency list (set up front, do not add more)

```toml
[package]
name = "tide"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
regex = "1"
anyhow = "1"
thiserror = "1"
directories = "5"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

`src/main.rs` uses `tracing_subscriber::fmt().with_env_filter(...).init()` at the
top of `main`. Shannon entropy is hand-rolled (~15 lines), no extra crate.

## Conventions

- One commit per task. Commit message format: `phase N: <module> — <imperative>`.
- Tick the checkbox in `docs/PLAN.md` in the SAME commit that completes the task.
- Never weaken a gate/test to pass a number.
- Do NOT push (the orchestrator merges).
- Leave a clean tree (commit or stash everything; no stray files).
