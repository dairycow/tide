# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-31

Initial public release.

### Added
- `tide init` — write `~/.config/tide/tide.toml`, `git init` the repo, set `origin`.
- `tide add` / `tide rm` / `tide list` — register and inspect watched home paths.
- `tide diff` — copy watched files into the repo, stage, and print the staged diff (no commit/push).
- `tide scan` — full secret-detection engine: known key prefixes, Shannon-entropy
  detection, configured regex patterns, and external `gitleaks`/`trufflehog` when on PATH.
- `tide sync` — copy → stage → secret gate → commit → `pull --rebase -X theirs` →
  push (when `auto_push`); pushes nothing on a secret finding (exit 2).
- `tide doctor` — environment and configuration diagnostics with blocker-aware exit codes.
- `tide install-skill` — install the embedded agent skill document into detected
  OpenCode / Claude Code skill directories.
- Four-layer secret defense (semantic review, full scan, sync gate, local-wins conflict policy).
- Dual MIT / Apache-2.0 license.
- CI (format check, clippy with `-D warnings`, test matrix).

[0.1.0]: https://github.com/dairycow/tide/releases/tag/v0.1.0
