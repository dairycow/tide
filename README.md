# tide

One-shot dotfile sync CLI. After you (or a coding agent) edit watched files under
`$HOME`, `tide` copies them into its own git repo, commits, and pushes — no daemon,
no filesystem watcher, no chezmoi. Invoked explicitly by a user or agent when
changes are ready to ship.

## Install

```bash
cargo install --path .
# or
cargo build --release   # binary at target/release/tide
```

The binary is named `tide`. To install the agent skill document into the
cross-tool `~/.agents/skills/tide/` directory:

```bash
tide install-skill
```

Requires Rust **1.88+** (edition 2024; uses let-chains).

## Quick start

```bash
tide init                 # config + repo + origin (<you>/dotfiles via gh); detects & offers to adopt dotfiles
tide add ~/.bashrc        # watch a home file (or accept them during init)
# edit ~/.bashrc …
tide diff                 # review what would be uploaded
tide sync                 # full secret-gate, commit, push
```

## Commands

| Command | Behavior | Exit |
|---|---|---|
| `tide init` | Write/merge `~/.config/tide/tide.toml`, `git init` the repo, set-or-update `origin` (**idempotent** — preserves existing watches/config). Remote prefilled from `gh` as `<you>/dotfiles`; offers to create a private GitHub repo (opt-in). Then detects common dotfiles under `$HOME` and offers to adopt them (`Add all? [y/N]`, default no). Missing/unauthed `gh` is ignored. | 0 ok; 1 unexpected |
| `tide add <path>` | Register a home path, copy into repo, append to watch list | 0 ok; 1 unexpected; 2 no config |
| `tide rm <path>` | Unregister a path (home file + repo copy left as-is) | 0 ok; 1 unexpected; 2 no config |
| `tide list` | Print watched `source -> target` mappings | 0 ok; 2 no config |
| `tide diff` | Copy watched → repo, stage, print `git diff --cached` (no commit/push) | 0 ok; 2 no config |
| `tide sync` | Copy → stage → **full secret gate** (prefixes + entropy + regex + gitleaks/trufflehog if present) → commit → `pull --rebase -X theirs` → push (if `auto_push`). On a finding: unstages, aborts, **nothing pushed** (exit **2**). | **0** ok/nothing; **2** secret block / bad config; 1 unexpected |
| `tide doctor` | Report binary, repo, origin, credentials, watches, external scanners | 0 ok; non-zero on blocker |
| `tide install-skill` | Install embedded `SKILL.md` into `~/.agents/skills/tide/` | 0 ok |

Exit codes: **0** success / no-op / clean; **2** secret finding/block or user-fixable config error; **1** unexpected failure.

## Config

Default path: `~/.config/tide/tide.toml`.

```toml
repo_path           = "~/.local/share/tide/repo"
remote              = "git@github.com:user/dotfiles.git"
auto_push           = true
secret_patterns     = ["(?i)(token|password|secret|api[_-]?key)\\s*="]
entropy_threshold   = 4.5
entropy_min_length  = 20

[[watches]]
source = "~/.bashrc"
target = "bashrc"
```

**Path mapping (source → target):** make the path relative to `$HOME`, then strip
one leading `.` from each path segment.

| source | target |
|---|---|
| `~/.bashrc` | `bashrc` |
| `~/.config/nvim/init.lua` | `config/nvim/init.lua` |
| `~/.tmux.conf` | `tmux.conf` |

## Secret safety (3 layers)

1. **Agent review of `tide diff`** — semantic judgment (keys, tokens, `.env`, opaque strings) before any push; documented in the skill.
2. **`tide sync` full gate** — known prefixes (`AKIA`, `ghp_`, `sk-`, …), high-entropy runs, config `secret_patterns`, plus `gitleaks` / `trufflehog` if installed. On a finding it unstages, aborts, and pushes **nothing** (exit **2**); snippets are redacted. (This is the former `tide scan`, folded into `sync` in v0.4.0.)
3. **Conflict policy** — `git pull --rebase -X theirs` so **local wins** on conflict.

## Agent usage

Coding agents should follow `skill/SKILL.md` (edit → `tide diff` review
→ `tide sync`, whose gate blocks on any secret). Install it into `~/.agents/skills/tide/` with:

```bash
tide install-skill
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Contributions intentionally submitted for inclusion must be dual-licensed
under the same terms.
