use crate::config::expand_tilde;
use anyhow::{Context, Result};

const SKILL: &str = include_str!("../skill/SKILL.md");

const SKILL_DIR: &str = "~/.agents/skills/tide";

pub fn run() -> Result<()> {
    let skill_dir = expand_tilde(SKILL_DIR);
    let skill_path = skill_dir.join("SKILL.md");
    std::fs::create_dir_all(&skill_dir)
        .with_context(|| format!("creating {}", skill_dir.display()))?;
    std::fs::write(&skill_path, SKILL)
        .with_context(|| format!("writing {}", skill_path.display()))?;
    println!("wrote: {}", skill_path.display());
    println!("restart your coding agent for the skill to load.");
    Ok(())
}
