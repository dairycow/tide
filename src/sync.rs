use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;

use crate::config::Config;
use crate::repo;
use crate::scan;

pub fn run(cfg: &Config) -> anyhow::Result<()> {
    let repo = repo::repo_path(cfg);

    // 0. Validate config regex FIRST — never silently weaken detection.
    for (index, pattern) in cfg.secret_patterns.iter().enumerate() {
        if let Err(e) = Regex::new(pattern) {
            eprintln!("error: invalid secret_patterns entry {index}: {pattern:?}: {e}");
            std::process::exit(2);
        }
    }

    // 1. Copy watched home files into the repo + stage.
    repo::copy_watched_into_repo(cfg)?;
    repo::add_all(&repo)?;

    // 2. Nothing to do?
    if repo::staged_diff_quiet(&repo)? {
        println!("nothing to sync");
        return Ok(());
    }

    // 3. SECRET GATE — built-in engine; no external scanners.
    let diff = repo::staged_diff(&repo)?;
    let findings = scan::scan_text(&diff, "staged", cfg, false);
    if !findings.is_empty() {
        for f in &findings {
            eprintln!("BLOCKED {}: {}:{} {}", f.kind, f.file, f.line, f.snippet);
        }
        let _ = repo::reset_index(&repo);
        eprintln!(
            "sync aborted: {} secret finding(s); nothing pushed. Fix and re-run.",
            findings.len()
        );
        std::process::exit(2);
    }

    // 4. Commit.
    let targets: Vec<String> = diff
        .lines()
        .filter_map(|l| l.strip_prefix("+++ b/").map(str::to_string))
        .filter(|p| p != "/dev/null")
        .collect();
    let targets_joined = if targets.is_empty() {
        cfg.watches.len().to_string()
    } else {
        targets.join(", ")
    };
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let msg = format!("tide: sync {targets_joined} @ {epoch}");
    repo::commit(&repo, &msg)?;
    let sha = repo::head_short_sha(&repo)?;

    // 5. Publish — best effort; never lose the local commit over network failure.
    let mut pushed = "no";
    if cfg.auto_push && repo::has_origin(&repo) {
        if let Err(e) = repo::fetch(&repo) {
            tracing::warn!("fetch failed: {e:#}");
        }
        if let Err(e) = repo::pull_rebase_theirs(&repo) {
            tracing::warn!("pull --rebase -X theirs failed (local kept): {e:#}");
        }
        match repo::push(&repo) {
            Ok(()) => pushed = "yes",
            Err(e) => tracing::warn!("push failed: {e:#}"),
        }
    }

    // 6. Result line.
    println!(
        "synced: {} | commit: {} | pushed: {}",
        targets_joined, sha, pushed
    );
    Ok(())
}
