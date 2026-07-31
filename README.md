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

The binary is named `tide`. To install the agent skill document into detected
OpenCode / Claude Code skill directories:

```bash
tide install-skill
```

Requires Rust **1.88+** (edition 2024; uses let-chains).

## Quick start

```bash
tide init                 # config + repo + remote prompt
tide add ~/.bashrc        # watch a home file
# edit ~/.bashrc …
tide sync                 # detect, secret-gate, commit, push
```

## Commands

| Command | Behavior | Exit |
|---|---|---|
| `tide init` | Write `~/.config/tide/tide.toml`, `git init` the repo, set `origin` | 0 ok; 1 unexpected |
| `tide add <path>` | Register a home path, copy into repo, append to watch list | 0 ok; 1 unexpected; 2 no config |
| `tide rm <path>` | Unregister a path (home file + repo copy left as-is) | 0 ok; 1 unexpected; 2 no config |
| `tide list` | Print watched `source -> target` mappings | 0 ok; 2 no config |
| `tide diff` | Copy watched → repo, stage, print `git diff --cached` (no commit/push) | 0 ok; 2 no config |
| `tide scan` | Full secret scan over staged content (prefixes + entropy + regex + gitleaks/trufflehog if present) | **0** clean; **2** findings / bad config |
| `tide sync` | Copy → stage → secret gate → commit → `pull --rebase -X theirs` → push (if `auto_push`) | **0** ok/nothing; **2** secret block / bad config; 1 unexpected |
| `tide doctor` | Report binary, repo, origin, credentials, watches, external scanners | 0 ok; non-zero on blocker |
| `tide install-skill` | Install embedded `SKILL.md` into agent skill dirs (`--agent claude\|opencode\|all`) | 0 ok |

Exit codes: **0** success / no-op / clean; **2** scan findings, secret block, or user-fixable config error; **1** unexpected failure.

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

## Secret safety (4 layers)

1. **Agent review of `tide diff`** — semantic judgment (keys, tokens, `.env`, opaque strings) before any push; documented in the skill.
2. **`tide scan`** — known prefixes (`AKIA`, `ghp_`, `sk-`, …), high-entropy runs, config `secret_patterns`, plus `gitleaks` / `trufflehog` if installed. Exit **2** on findings; snippets are redacted.
3. **`tide sync` built-in gate** — prefixes + entropy + regex on staged content; on hit, unstages, aborts, **nothing pushed** (exit **2**).
4. **Conflict policy** — `git pull --rebase -X theirs` so **local wins** on conflict.

## Agent usage

Coding agents should follow `skill/SKILL.md` (edit → `tide diff` review → `tide scan`
→ only then `tide sync`). Install it into local agent skill dirs with:

```bash
tide install-skill
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Contributions intentionally submitted for inclusion must be dual-licensed
under the same terms.
