#![allow(dead_code)]

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{self, Config};

/// Expanded, absolute repo path from cfg.repo_path (tilde / $HOME expanded).
pub fn repo_path(cfg: &Config) -> PathBuf {
    config::expand_tilde(&cfg.repo_path)
}

/// Copy every watched source file into the repo at its `target` (creating
/// parent dirs). Returns the count copied. A missing source is warned via
/// `tracing::warn!` and skipped (does not abort the whole copy).
pub fn copy_watched_into_repo(cfg: &Config) -> Result<usize> {
    let repo = repo_path(cfg);
    let mut count = 0usize;

    for watch in &cfg.watches {
        let source = config::expand_tilde(&watch.source);
        if !source.is_file() {
            tracing::warn!(
                source = %source.display(),
                target = %watch.target,
                "watched source missing or not a file; skipping"
            );
            continue;
        }

        let target = &watch.target;
        let target_path = Path::new(target);

        // Reject absolute targets and any `..` / root components (path traversal).
        if target_path.is_absolute()
            || target_path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
        {
            tracing::warn!(
                "skipping unsafe target {target} for {source}",
                target = target,
                source = source.display()
            );
            continue;
        }

        // Defense in depth: dest must stay under the canonical repo directory.
        let dest = match std::fs::canonicalize(&repo) {
            Ok(canonical_repo) => {
                let dest = canonical_repo.join(target);
                if !dest.starts_with(&canonical_repo) {
                    tracing::warn!(
                        "skipping unsafe target {target} for {source}",
                        target = target,
                        source = source.display()
                    );
                    continue;
                }
                dest
            }
            Err(_) => {
                // Component check already passed; proceed with non-canonical repo.
                repo.join(target)
            }
        };

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }

        let bytes =
            std::fs::read(&source).with_context(|| format!("reading {}", source.display()))?;
        std::fs::write(&dest, bytes)
            .with_context(|| format!("writing {} -> {}", source.display(), dest.display()))?;
        count += 1;
    }

    Ok(count)
}

fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo).args(args);
    let output = cmd
        .output()
        .with_context(|| format!("running git -C {} {}", repo.display(), args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(-1);
        bail!(
            "git -C {} {} failed (exit {code}): {}",
            repo.display(),
            args.join(" "),
            stderr.trim()
        );
    }

    Ok(output)
}

pub fn add_all(repo: &Path) -> Result<()> {
    git(repo, &["add", "-A"]).context("git add -A")?;
    Ok(())
}

/// true == nothing staged (working tree clean vs index). Wraps `git diff --cached --quiet`.
pub fn staged_diff_quiet(repo: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--cached", "--quiet"])
        .output()
        .with_context(|| format!("running git -C {} diff --cached --quiet", repo.display()))?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        Some(code) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "git -C {} diff --cached --quiet failed (exit {code}): {}",
                repo.display(),
                stderr.trim()
            )
        }
        None => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "git -C {} diff --cached --quiet terminated by signal: {}",
                repo.display(),
                stderr.trim()
            )
        }
    }
}

pub fn staged_diff(repo: &Path) -> Result<String> {
    let output = git(repo, &["diff", "--cached"]).context("git diff --cached")?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn status_porcelain(repo: &Path) -> Result<String> {
    let output = git(repo, &["status", "--porcelain"]).context("git status --porcelain")?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn commit(repo: &Path, msg: &str) -> Result<()> {
    git(repo, &["commit", "-m", msg, "--"]).context("git commit")?;
    Ok(())
}

pub fn fetch(repo: &Path) -> Result<()> {
    git(repo, &["fetch"]).context("git fetch")?;
    Ok(())
}

/// `git pull --rebase -X theirs` (local wins on conflict). Error if no upstream.
pub fn pull_rebase_theirs(repo: &Path) -> Result<()> {
    git(repo, &["pull", "--rebase", "-X", "theirs"]).context("git pull --rebase -X theirs")?;
    Ok(())
}

pub fn push(repo: &Path) -> Result<()> {
    git(repo, &["push"]).context("git push")?;
    Ok(())
}

pub fn head_short_sha(repo: &Path) -> Result<String> {
    let output =
        git(repo, &["rev-parse", "--short", "HEAD"]).context("git rev-parse --short HEAD")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn has_origin(repo: &Path) -> bool {
    git(repo, &["remote", "get-url", "origin"]).is_ok()
}

/// Unstage everything (`git -C <repo> reset`), keeping working-tree files. Used
/// by sync to back out a secret-bearing staging before bailing.
pub fn reset_index(repo: &Path) -> anyhow::Result<()> {
    git(repo, &["reset"]).context("git reset")?;
    Ok(())
}
