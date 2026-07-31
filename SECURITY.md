# Security Policy

`tide` copies files out of your `$HOME` into its own git repo and pushes them to a
remote you configure. Its primary risk is **leaking secrets that live in watched
dotfiles**. The binary is built around preventing exactly that.

## Supported versions

Only the latest release line receives security fixes.

| Version | Supported |
| ------- | --------- |
| 0.4.x   | yes       |

## Secret defense (built-in, 3 layers)

1. **Semantic review of `tide diff`** — human/agent judgment before anything is pushed.
2. **`tide sync` full gate** — known key prefixes (`AKIA`, `ghp_/gho_/ghs_`, `sk-`, `xox[bpoa]-`,
   `glpat-`, `AIza`, `eyJ`, PEM private keys), Shannon-entropy detection, configured
   regex patterns, and external scanners (`gitleaks` / `trufflehog`) when installed. On a
   hit it unstages, aborts, and **pushes nothing** (exit 2). (This is the former
   `tide scan`, folded into `sync` in v0.4.0.)
3. **Conflict policy** — `git pull --rebase -X theirs` so local changes win on conflict.

Snippets in sync/scan output are truncated and redacted. **Never bypass a finding** — move
the secret to an environment variable or a non-watched file instead.

## Reporting a vulnerability

If you believe you have found a security issue, **please do not open a public issue**.
Report it privately by opening a [private security advisory] on GitHub, or email the
maintainer. Include:

- a description of the issue and its impact,
- the `tide` version (`tide doctor` output if possible),
- reproduction steps (redact any real secrets).

You should receive an initial response within a few days.

[private security advisory]: https://github.com/dairycow/tide/security/advisories/new

## Scope

In scope: anything in this repository that could cause a secret to be committed or
pushed unexpectedly, weaken the scan/gate, mishandle paths (e.g. traversal outside
the tide repo), or otherwise compromise the user's dotfiles or git history.

Out of scope: secrets you explicitly place in a watched file and then explicitly
push past a gate after `tide` warned you.
