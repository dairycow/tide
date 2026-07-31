use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_repo_path")]
    pub repo_path: String,
    #[serde(default)]
    pub remote: String,
    #[serde(default = "default_auto_push")]
    pub auto_push: bool,
    #[serde(default = "default_secret_patterns")]
    pub secret_patterns: Vec<String>,
    #[serde(default = "default_entropy_threshold")]
    pub entropy_threshold: f64,
    #[serde(default = "default_entropy_min_length")]
    pub entropy_min_length: usize,
    #[serde(default)]
    pub watches: Vec<Watch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watch {
    pub source: String,
    pub target: String,
}

fn default_repo_path() -> String {
    "~/.local/share/tide/repo".to_string()
}

fn default_auto_push() -> bool {
    true
}

fn default_secret_patterns() -> Vec<String> {
    vec!["(?i)(token|password|secret|api[_-]?key)\\s*=".to_string()]
}

fn default_entropy_threshold() -> f64 {
    4.5
}

fn default_entropy_min_length() -> usize {
    20
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repo_path: default_repo_path(),
            remote: String::new(),
            auto_push: default_auto_push(),
            secret_patterns: default_secret_patterns(),
            entropy_threshold: default_entropy_threshold(),
            entropy_min_length: default_entropy_min_length(),
            watches: Vec::new(),
        }
    }
}

fn home_dir() -> PathBuf {
    if let Some(base) = directories::BaseDirs::new() {
        return base.home_dir().to_path_buf();
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home);
    }
    PathBuf::from("/")
}

pub fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return home_dir();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    if s == "$HOME" {
        return home_dir();
    }
    if let Some(rest) = s.strip_prefix("$HOME/") {
        return home_dir().join(rest);
    }
    PathBuf::from(s)
}

pub fn config_path() -> PathBuf {
    expand_tilde("~/.config/tide/tide.toml")
}

pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let cfg: Config = toml::from_str(&contents)
        .with_context(|| format!("parsing config {}", path.display()))?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating config dir {}", parent.display()))?;
    }
    let contents = toml::to_string_pretty(cfg).context("serializing config")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("writing config {}", path.display()))?;
    Ok(())
}

/// Map a home source path to a repo-relative target.
///
/// Rule: make relative to `$HOME`, then strip one leading `.` from each path
/// segment. If not under `$HOME`, strip a leading `/` and apply the same
/// segment rule (best-effort).
pub fn map_target(source: &str) -> String {
    let expanded = expand_tilde(source);
    let home = home_dir();

    let rel = if expanded.starts_with(&home) {
        expanded
            .strip_prefix(&home)
            .map(|p| p.to_path_buf())
            .unwrap_or(expanded)
    } else {
        expanded
    };

    let s = rel.to_string_lossy();
    let s = s.trim_start_matches('/');

    s.split('/')
        .filter(|seg| !seg.is_empty())
        .map(|seg| seg.strip_prefix('.').unwrap_or(seg))
        .collect::<Vec<_>>()
        .join("/")
}

fn path_to_source_string(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    if path_str.starts_with('~') || path_str.starts_with("$HOME") {
        return path_str.into_owned();
    }
    let expanded = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let home = home_dir();
    if let Ok(rel) = expanded.strip_prefix(&home) {
        format!("~/{}", rel.display())
    } else {
        expanded.to_string_lossy().into_owned()
    }
}

fn prompt_remote() -> String {
    eprint!("Remote URL (origin) [empty to skip]: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => String::new(),
        Ok(_) => line.trim().to_string(),
    }
}

pub fn cmd_init(remote: Option<String>) -> Result<()> {
    let remote = match remote {
        Some(r) => r,
        None => prompt_remote(),
    };

    let cfg = Config {
        remote: remote.clone(),
        ..Config::default()
    };
    save(&cfg)?;
    println!("wrote config {}", config_path().display());

    let repo = expand_tilde(&cfg.repo_path);
    std::fs::create_dir_all(&repo)
        .with_context(|| format!("creating repo dir {}", repo.display()))?;

    let status = Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .status()
        .context("running git init")?;
    if !status.success() {
        bail!("git init failed with status {status}");
    }
    println!("initialized git repo at {}", repo.display());

    if !remote.is_empty() {
        let status = Command::new("git")
            .args(["remote", "add", "origin", &remote])
            .current_dir(&repo)
            .status()
            .context("running git remote add")?;
        if !status.success() {
            bail!("git remote add origin failed with status {status}");
        }
        println!("set origin -> {remote}");
    }

    Ok(())
}

pub fn cmd_add(path: PathBuf) -> Result<()> {
    if !config_path().exists() {
        bail!("no config found; run `tide init` first");
    }
    let mut cfg = load()?;

    let source = path_to_source_string(&path);
    let target = map_target(&source);

    let home_file = expand_tilde(&source);
    if !home_file.exists() {
        bail!("file not found: {}", home_file.display());
    }
    if !home_file.is_file() {
        bail!("not a file: {}", home_file.display());
    }

    let repo = expand_tilde(&cfg.repo_path);
    let dest = repo.join(&target);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::copy(&home_file, &dest).with_context(|| {
        format!(
            "copying {} -> {}",
            home_file.display(),
            dest.display()
        )
    })?;

    cfg.watches.retain(|w| w.source != source);
    cfg.watches.push(Watch {
        source: source.clone(),
        target: target.clone(),
    });
    save(&cfg)?;

    println!("added: {source} -> {target}");
    Ok(())
}

pub fn cmd_rm(path: PathBuf) -> Result<()> {
    if !config_path().exists() {
        bail!("no config found; run `tide init` first");
    }
    let mut cfg = load()?;

    let source = path_to_source_string(&path);
    let target = map_target(&source);
    let before = cfg.watches.len();
    cfg.watches
        .retain(|w| w.source != source && w.target != target && w.source != path.to_string_lossy());
    if cfg.watches.len() == before {
        bail!("not watched: {}", path.display());
    }
    save(&cfg)?;
    println!("removed: {source}");
    Ok(())
}

pub fn cmd_list() -> Result<()> {
    if !config_path().exists() {
        bail!("no config found; run `tide init` first");
    }
    let cfg = load()?;
    if cfg.watches.is_empty() {
        println!("(no watches)");
        return Ok(());
    }
    for w in &cfg.watches {
        println!("{} -> {}", w.source, w.target);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_target_bashrc() {
        let home = home_dir();
        let src = home.join(".bashrc");
        assert_eq!(map_target(&src.to_string_lossy()), "bashrc");
        assert_eq!(map_target("~/.bashrc"), "bashrc");
    }

    #[test]
    fn map_target_nested() {
        assert_eq!(
            map_target("~/.config/nvim/init.lua"),
            "config/nvim/init.lua"
        );
    }

    #[test]
    fn map_target_tmux() {
        assert_eq!(map_target("~/.tmux.conf"), "tmux.conf");
    }
}
