use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
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
    let cfg: Config =
        toml::from_str(&contents).with_context(|| format!("parsing config {}", path.display()))?;
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

// ---------------------------------------------------------------------------
// Remote-URL inference (gh CLI) — every path is non-fatal.
// ---------------------------------------------------------------------------

/// Build a GitHub remote URL for the given owner/repo.
fn build_remote_url(owner: &str, repo: &str, ssh: bool) -> String {
    if ssh {
        format!("git@github.com:{owner}/{repo}.git")
    } else {
        format!("https://github.com/{owner}/{repo}.git")
    }
}

/// Parse a github.com remote URL (SSH or HTTPS) into `(owner, repo)`.
/// Returns `None` for non-github hosts or anything unparseable; strips a
/// trailing `.git`. Gates the `gh repo create` offer.
fn github_owner_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let rest = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    split_owner_repo(rest)
}

fn split_owner_repo(s: &str) -> Option<(String, String)> {
    // Drop trailing `.git` / slashes and any query/fragment.
    let s = s.trim_end_matches(".git").trim_end_matches('/');
    let s = s.split(['?', '#']).next().unwrap_or(s);
    let mut it = s.splitn(2, '/');
    let owner = it.next()?.trim();
    let repo = it.next()?.trim();
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Infer the repo name from the current working directory's basename
/// (matches `gh repo create`'s default). `None` at filesystem root.
fn repo_name_from_cwd() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let name = cwd.file_name()?;
    let s = name.to_string_lossy();
    (!s.is_empty()).then(|| s.into_owned())
}

/// Is a binary on PATH? (Pure PATH scan, never spawns — safe to probe.)
fn binary_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
}

/// Run `gh <args>` and return trimmed stdout. `None` if gh is missing, the
/// call fails, exits non-zero, or yields empty output.
fn gh_capture(args: &[&str]) -> Option<String> {
    if !binary_on_path("gh") {
        return None;
    }
    let output = Command::new("gh").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Authenticated gh user (`gh api user --jq .login`), or `None` if gh is
/// absent / unauthenticated.
fn gh_owner() -> Option<String> {
    gh_capture(&["api", "user", "--jq", ".login"])
}

/// gh's configured git protocol is SSH? Falls back to false (HTTPS).
fn gh_protocol_is_ssh() -> bool {
    gh_capture(&["config", "get", "git_protocol"]).is_some_and(|p| p.trim() == "ssh")
}

/// Create a private GitHub repo. Non-fatal: warns and continues on any error
/// (e.g. the repo already exists).
fn create_github_repo(owner: &str, repo: &str) {
    let name = format!("{owner}/{repo}");
    match Command::new("gh")
        .args(["repo", "create", &name, "--private"])
        .status()
    {
        Ok(status) if status.success() => println!("created private repo {name}"),
        Ok(status) => eprintln!(
            "warning: gh repo create exited {status} (repo may already exist); continuing"
        ),
        Err(e) => eprintln!("warning: could not run gh repo create ({e}); continuing"),
    }
}

fn prompt_remote(default: &str) -> String {
    if default.is_empty() {
        eprint!("Remote URL (origin) [empty to skip]: ");
    } else {
        eprint!("Remote URL (origin) [{default}] (enter to accept): ");
    }
    let _ = io::stderr().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => default.to_string(),
        Ok(_) => {
            let t = line.trim();
            if t.is_empty() {
                default.to_string()
            } else {
                t.to_string()
            }
        }
    }
}

/// Yes/no prompt. Returns `default_yes` on empty input / EOF.
fn prompt_yes_no(prompt: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    eprint!("{prompt} {hint}: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => default_yes,
        Ok(_) => {
            let t = line.trim().to_ascii_lowercase();
            if t.is_empty() {
                default_yes
            } else {
                t == "y" || t == "yes"
            }
        }
    }
}

pub fn cmd_init(remote: Option<String>) -> Result<()> {
    let remote = match remote {
        Some(r) => r,
        None => {
            // Prefill the remote from the gh CLI when available: the
            // authenticated user as owner, cwd basename as repo, gh's
            // configured protocol.
            let owner = gh_owner();
            let default = match (&owner, repo_name_from_cwd()) {
                (Some(o), Some(r)) => Some(build_remote_url(o, &r, gh_protocol_is_ssh())),
                _ => None,
            };
            let remote = prompt_remote(&default.unwrap_or_default());

            // Offer to create the repo on GitHub (opt-in; only when gh is
            // authenticated and the URL points at github.com).
            if !remote.is_empty()
                && owner.is_some()
                && let Some((o, r)) = github_owner_repo(&remote)
                && prompt_yes_no(&format!("Create {o}/{r} as a private GitHub repo?"), false)
            {
                create_github_repo(&o, &r);
            }

            remote
        }
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
    std::fs::copy(&home_file, &dest)
        .with_context(|| format!("copying {} -> {}", home_file.display(), dest.display()))?;

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

    #[test]
    fn build_remote_url_ssh_and_https() {
        assert_eq!(
            build_remote_url("alice", "dotfiles", true),
            "git@github.com:alice/dotfiles.git"
        );
        assert_eq!(
            build_remote_url("alice", "dotfiles", false),
            "https://github.com/alice/dotfiles.git"
        );
    }

    #[test]
    fn github_owner_repo_ssh_forms() {
        let expected = Some(("alice".to_string(), "dotfiles".to_string()));
        assert_eq!(
            github_owner_repo("git@github.com:alice/dotfiles.git"),
            expected
        );
        assert_eq!(github_owner_repo("git@github.com:alice/dotfiles"), expected);
        assert_eq!(
            github_owner_repo("ssh://git@github.com/alice/dotfiles.git"),
            expected
        );
    }

    #[test]
    fn github_owner_repo_https_form() {
        assert_eq!(
            github_owner_repo("https://github.com/alice/dotfiles.git"),
            Some(("alice".to_string(), "dotfiles".to_string()))
        );
    }

    #[test]
    fn github_owner_repo_rejects_non_github_and_garbage() {
        // Non-github hosts.
        assert_eq!(github_owner_repo("git@gitlab.com:alice/dotfiles.git"), None);
        assert_eq!(
            github_owner_repo("https://example.com/alice/dotfiles"),
            None
        );
        // Malformed / incomplete.
        assert_eq!(github_owner_repo(""), None);
        assert_eq!(github_owner_repo("not a url"), None);
        assert_eq!(github_owner_repo("https://github.com/onlyone"), None);
        assert_eq!(github_owner_repo("https://github.com/a/b/c"), None);
    }

    #[test]
    fn repo_name_from_cwd_is_some() {
        // cargo test's cwd is the package root; it has a basename.
        let name = repo_name_from_cwd();
        assert!(name.is_some(), "cwd should have a basename");
        assert!(!name.unwrap().is_empty());
    }
}
