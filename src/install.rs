use crate::config::expand_tilde;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

const SKILL: &str = include_str!("../skill/SKILL.md");

/// Candidate agent skill roots. Install path is `<root>/skills/tide/SKILL.md`.
const CANDIDATES: &[(&str, &str)] = &[
    ("claude", "~/.claude"),
    ("opencode", "~/.config/opencode"),
    ("agents", "~/.agents"),
];

pub fn run(agent: Option<String>) -> Result<()> {
    let targets = select_targets(agent.as_deref())?;
    if targets.is_empty() {
        bail!("no install targets selected");
    }

    let mut wrote = 0usize;
    for root in &targets {
        let skill_dir = root.join("skills").join("tide");
        let skill_path = skill_dir.join("SKILL.md");
        std::fs::create_dir_all(&skill_dir)
            .with_context(|| format!("creating {}", skill_dir.display()))?;
        std::fs::write(&skill_path, SKILL)
            .with_context(|| format!("writing {}", skill_path.display()))?;
        println!("wrote: {}", skill_path.display());
        wrote += 1;
    }

    if wrote == 0 {
        bail!("failed to write skill to any target");
    }

    println!("restart your coding agent for the skill to load.");
    Ok(())
}

fn select_targets(agent: Option<&str>) -> Result<Vec<PathBuf>> {
    match agent {
        None => {
            let existing: Vec<PathBuf> = CANDIDATES
                .iter()
                .map(|(_, path)| expand_tilde(path))
                .filter(|p| p.is_dir())
                .collect();
            if existing.is_empty() {
                // Default home when nothing is detected.
                Ok(vec![expand_tilde("~/.config/opencode")])
            } else {
                Ok(existing)
            }
        }
        Some("claude") => Ok(vec![expand_tilde("~/.claude")]),
        Some("opencode") => Ok(vec![expand_tilde("~/.config/opencode")]),
        Some("all") => Ok(CANDIDATES
            .iter()
            .map(|(_, path)| expand_tilde(path))
            .collect()),
        Some(other) => bail!(
            "unknown agent '{other}'; valid choices: claude, opencode, all (or omit for detected)"
        ),
    }
}
