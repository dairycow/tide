use crate::{config::Config, repo};

pub fn run(cfg: &Config) -> anyhow::Result<()> {
    let r = repo::repo_path(cfg);
    repo::copy_watched_into_repo(cfg)?;
    repo::add_all(&r)?;
    let diff = repo::staged_diff(&r)?;
    if diff.trim().is_empty() {
        println!("(no changes)");
    } else {
        print!("{diff}");
    }
    Ok(())
}
