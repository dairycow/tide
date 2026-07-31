use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{self, Config};
use crate::repo;

pub fn run(cfg: &Config) -> anyhow::Result<()> {
    let mut blockers = 0usize;

    // 1. tide version
    println!(
        "ok     tide version      v{}",
        env!("CARGO_PKG_VERSION")
    );

    // 2. config file
    let cfg_path = config::config_path();
    if cfg_path.exists() {
        println!("ok     config file       {}", cfg_path.display());
    } else {
        println!("fail   config file       missing: {}", cfg_path.display());
        blockers += 1;
    }

    // 3. repo dir
    let repo = repo::repo_path(cfg);
    let repo_ok = is_git_repo(&repo);
    if repo_ok {
        println!("ok     repo dir          {}", repo.display());
    } else if !repo.exists() {
        println!("fail   repo dir          missing: {}", repo.display());
        blockers += 1;
    } else {
        println!(
            "fail   repo dir          not a git repo: {}",
            repo.display()
        );
        blockers += 1;
    }

    // 4. origin remote
    if repo_ok && repo::has_origin(&repo) {
        println!("ok     origin remote     set");
    } else if repo_ok {
        println!("fail   origin remote     not set (cannot push)");
        blockers += 1;
    } else {
        println!("fail   origin remote     unavailable (no git repo)");
        blockers += 1;
    }

    // 5. watched files
    let n = cfg.watches.len();
    let mut missing = 0usize;
    for w in &cfg.watches {
        let source = config::expand_tilde(&w.source);
        if !source.exists() {
            println!(
                "warn   watched files     missing source: {}",
                w.source
            );
            missing += 1;
        }
    }
    if missing == 0 {
        println!("ok     watched files     {n} registered");
    } else {
        println!(
            "warn   watched files     {n} registered, {missing} source(s) missing"
        );
    }

    // 6. credential / ssh
    let has_ssh = has_ssh_pubkey();
    let has_cred = has_credential_helper();
    if has_ssh || has_cred {
        let detail = match (has_ssh, has_cred) {
            (true, true) => "ssh pubkey + credential.helper",
            (true, false) => "ssh pubkey present",
            (false, true) => "credential.helper set",
            (false, false) => unreachable!(),
        };
        println!("ok     credential/ssh    {detail}");
    } else {
        println!(
            "warn   credential/ssh    no ~/.ssh/*.pub and no credential.helper (push may fail)"
        );
    }

    // 7. scanners (self-probe PATH; independent of scan module)
    let gitleaks = on_path("gitleaks");
    let trufflehog = on_path("trufflehog");
    let g = if gitleaks { "present" } else { "absent" };
    let t = if trufflehog { "present" } else { "absent" };
    println!("ok     scanners          gitleaks={g} trufflehog={t}");

    // summary
    if blockers > 0 {
        println!("result: BLOCKERS ({blockers})");
        std::process::exit(1);
    }
    println!("result: ok");
    Ok(())
}

fn is_git_repo(repo: &Path) -> bool {
    if !repo.exists() {
        return false;
    }
    if repo.join(".git").exists() {
        return true;
    }
    Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn has_ssh_pubkey() -> bool {
    let ssh_dir = config::expand_tilde("~/.ssh");
    let Ok(entries) = std::fs::read_dir(&ssh_dir) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| e.path().extension().is_some_and(|ext| ext == "pub"))
}

fn has_credential_helper() -> bool {
    let Ok(output) = Command::new("git")
        .args(["config", "--global", "credential.helper"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    !String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

fn on_path(bin: &str) -> bool {
    if which(bin).is_some() {
        return true;
    }
    Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn which(bin: &str) -> Option<PathBuf> {
    let Ok(output) = Command::new("which").arg(bin).output() else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}
