---
name: tide
description: Sync dotfile edits to the user's git repo via the tide CLI. Use when the user asks to edit, update, add to, or tweak their tmux, nvim, vim, bash, zsh, or git config, or invokes /tide. After editing the file, run "tide scan" then "tide sync" to detect changes and commit + push. tide runs its own secret scan as a final gate.
---

# tide

`tide` captures changes to a user's dotfiles and commits + pushes them to the
user's own git repo. It is invoked explicitly (by you or the user) — there is no
daemon. Default watched set: tmux, nvim, vim, bash, zsh, gitconfig (managed via
`tide add`).

## When to use

The user asks to change, add to, or tweak any of: tmux, nvim, vim, bash, zsh, or
git config (e.g. ~/.bashrc, ~/.tmux.conf, ~/.config/nvim/init.lua, ~/.gitconfig).
Also when they say "sync/push my dotfiles" or invoke /tide.

## Flow (mandatory order — this is the belt-and-suspenders secret defense)

1. Make the requested edit to the dotfile as normal.
2. Run `tide diff` and REVIEW the output yourself for anything secret-like —
   keys, tokens, passwords, private IPs, sensitive paths, .env content, long
   opaque strings. Your judgment catches what automated scanners miss. Do not
   skip this.
3. Run `tide scan`. It checks the would-be-uploaded content against known key
   prefixes (AKIA, ghp_/gho_/ghs_, sk-, xox[bpoa]-, glpat-, AIza, eyJ, PEM private
   keys), high-entropy strings, configured regex, and gitleaks/trufflehog if
   installed. Exit code 0 = clean, 2 = findings.
4. If EITHER step 2 or 3 finds something: do NOT run `tide sync`. Surface the
   exact file, line, and snippet to the user verbatim and ask how to proceed
   (e.g. move the value to an env var or a non-synced file). Never bypass a
   finding.
5. Only if both are clean, run `tide sync`. It copies changed files into the
   repo, runs its OWN regex gate as a final block, commits, rebases local on top
   of remote, and pushes. Relay its result line (files synced, commit sha,
   pushed=yes/no).

## If tide is not set up yet

Run `tide doctor`. If it reports problems:
- missing repo/config -> `tide init` (creates ~/.config/tide/tide.toml + git repo,
  prompts for the remote — prefilled from the `gh` CLI if installed, and offers to
  create a private GitHub repo; both optional and skipped silently without `gh`)
- a file isn't being tracked -> `tide add <path>`

## Commands

- `tide sync` — detect changes, commit, push (the main one)
- `tide diff` — show what would be uploaded (for your review)
- `tide scan` — automated secret scan (exit 2 = findings)
- `tide add <path>` / `tide rm <path>` / `tide list` — manage watched files
- `tide init` / `tide doctor` — setup and checks
