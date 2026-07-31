//! End-to-end integration tests for the `tide` binary.
//!
//! Drive the compiled binary as a subprocess with a per-test temp HOME and a
//! local bare git remote. No network. No process-global env mutation
//! (`std::env::set_var` is forbidden — cargo test runs in parallel).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn tide_bin() -> PathBuf {
    // Standard for `[[bin]]` / package-named binary under cargo test.
    option_env!("CARGO_BIN_EXE_tide")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/tide"))
}

/// Run `tide <args>` with HOME (and related env) pointed at `home`.
/// Never uses process-global `set_var`.
fn tide(bin: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(bin)
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "tide-test")
        .env("GIT_AUTHOR_EMAIL", "tide-test@example.com")
        .env("GIT_COMMITTER_NAME", "tide-test")
        .env("GIT_COMMITTER_EMAIL", "tide-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn tide {:?}: {e}", args))
}

fn git(args: &[&str], cwd: Option<&Path>) -> Output {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "tide-test")
        .env("GIT_AUTHOR_EMAIL", "tide-test@example.com")
        .env("GIT_COMMITTER_NAME", "tide-test")
        .env("GIT_COMMITTER_EMAIL", "tide-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.output()
        .unwrap_or_else(|e| panic!("failed to spawn git {:?}: {e}", args))
}

fn git_ok(args: &[&str], cwd: Option<&Path>) {
    let out = git(args, cwd);
    if !out.status.success() {
        panic!(
            "git {:?} failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
            args,
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

fn stdout_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_exit(out: &Output, expected: i32, ctx: &str) {
    let code = out.status.code().unwrap_or(-1);
    if code != expected {
        panic!(
            "{ctx}: expected exit {expected}, got {code}\nstdout:\n{}\nstderr:\n{}",
            stdout_str(out),
            stderr_str(out),
        );
    }
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    bare: PathBuf,
    repo: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Isolated HOME + bare remote + pre-written config + initialized tide repo.
fn setup_fixture() -> Fixture {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "tide-itest-{}-{}-{}",
        std::process::id(),
        n,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let home = root.join("home");
    let bare = root.join("bare.git");
    let repo = home.join(".local/share/tide/repo");

    fs::create_dir_all(&home).expect("create home");
    fs::create_dir_all(home.join(".config/tide")).expect("create config dir");
    fs::create_dir_all(&repo).expect("create repo dir");

    // Bare remote (local filesystem — no network).
    git_ok(&["init", "--bare", bare.to_str().unwrap()], None);

    // Expand $HOME literally into absolute paths written into the config.
    let home_s = home.to_string_lossy();
    let bare_s = bare.to_string_lossy();
    let repo_s = repo.to_string_lossy();
    let config = format!(
        r#"repo_path = "{repo_s}"
remote = "{bare_s}"
auto_push = true
secret_patterns = ["(?i)(token|password|secret|api[_-]?key)\\s*="]
entropy_threshold = 4.5
entropy_min_length = 20
"#
    );
    // Ensure tilde-style sources work: HOME is our temp home.
    let _ = home_s; // used via env when tide runs; config uses absolute repo/remote.
    fs::write(home.join(".config/tide/tide.toml"), config).expect("write tide.toml");

    // Initialize working repo with local identity (survives GIT_CONFIG_GLOBAL=/dev/null).
    git_ok(&["init"], Some(&repo));
    git_ok(&["config", "user.name", "tide-test"], Some(&repo));
    git_ok(
        &["config", "user.email", "tide-test@example.com"],
        Some(&repo),
    );
    git_ok(&["config", "commit.gpgsign", "false"], Some(&repo));
    git_ok(&["branch", "-M", "main"], Some(&repo));
    git_ok(
        &["remote", "add", "origin", bare.to_str().unwrap()],
        Some(&repo),
    );
    git_ok(&["commit", "--allow-empty", "-m", "init"], Some(&repo));
    git_ok(&["push", "-u", "origin", "main"], Some(&repo));

    Fixture {
        root,
        home,
        bare,
        repo,
    }
}

fn bare_commit_count(bare: &Path) -> u64 {
    let out = git(
        &[
            "--git-dir",
            bare.to_str().unwrap(),
            "rev-list",
            "--count",
            "main",
        ],
        None,
    );
    if !out.status.success() {
        panic!("rev-list failed: {}", stderr_str(&out));
    }
    stdout_str(&out)
        .trim()
        .parse()
        .expect("commit count should be a number")
}

fn bare_show(bare: &Path, path: &str) -> String {
    let rev = format!("main:{path}");
    let out = git(&["--git-dir", bare.to_str().unwrap(), "show", &rev], None);
    if !out.status.success() {
        panic!("git show {rev} failed: {}", stderr_str(&out));
    }
    stdout_str(&out)
}

// ---------------------------------------------------------------------------
// Test A — happy path: clean sync + push arrives at remote
// ---------------------------------------------------------------------------

#[test]
fn happy_path_sync_pushes_to_remote() {
    let fx = setup_fixture();
    let bin = tide_bin();
    assert!(
        bin.is_file(),
        "tide binary missing at {}; build first (cargo test builds it)",
        bin.display()
    );

    // 1. Benign bashrc.
    fs::write(fx.home.join(".bashrc"), "# my bashrc\nalias ll='ls -l'\n").expect("write .bashrc");

    // 2. tide add
    let out = tide(
        &bin,
        &fx.home,
        &["add", &format!("{}/.bashrc", fx.home.display())],
    );
    assert_exit(&out, 0, "tide add ~/.bashrc");
    let add_out = stdout_str(&out);
    assert!(
        add_out.contains("added:") || add_out.contains("bashrc"),
        "unexpected add stdout: {add_out}"
    );

    // 3. Modify bashrc.
    fs::write(
        fx.home.join(".bashrc"),
        "# my bashrc\nalias ll='ls -l'\nexport EDITOR=nvim\n",
    )
    .expect("rewrite .bashrc");

    // 4. tide scan — clean.
    let out = tide(&bin, &fx.home, &["scan"]);
    assert_exit(&out, 0, "tide scan (clean)");
    let scan_out = stdout_str(&out);
    assert!(
        scan_out.to_lowercase().contains("clean"),
        "scan stdout should contain 'clean', got: {scan_out}"
    );

    // 5. tide sync — push.
    let out = tide(&bin, &fx.home, &["sync"]);
    assert_exit(&out, 0, "tide sync");
    let sync_out = stdout_str(&out);
    assert!(
        sync_out.contains("pushed: yes"),
        "sync stdout should contain 'pushed: yes', got: {sync_out}\nstderr: {}",
        stderr_str(&out)
    );

    // 6. Bare remote has the commit and content.
    let count = bare_commit_count(&fx.bare);
    assert!(
        count >= 2,
        "bare remote should have >=2 commits after sync, got {count}"
    );
    let content = bare_show(&fx.bare, "bashrc");
    assert!(
        content.contains("EDITOR=nvim"),
        "remote bashrc should contain EDITOR=nvim, got:\n{content}"
    );

    // Keep fx alive until end (Drop cleans up).
    let _ = fx.repo;
}

// ---------------------------------------------------------------------------
// Test B — secret block: scan finds ghp_ token; sync does not push
// ---------------------------------------------------------------------------

#[test]
fn secret_block_refuses_push() {
    let fx = setup_fixture();
    let bin = tide_bin();
    assert!(bin.is_file(), "tide binary missing at {}", bin.display());

    let commits_before = bare_commit_count(&fx.bare);

    // Deterministic 36-char alphanumerics (ghp_ pattern requires {36,}).
    let suffix = "abcdefghijklmnopqrstuvwxyz0123456789";
    assert_eq!(suffix.len(), 36);
    let full_token = format!("ghp_{suffix}");
    let zshrc = format!("MY_TOKEN={full_token}\n");

    fs::write(fx.home.join(".zshrc"), &zshrc).expect("write .zshrc");

    // 1. tide add
    let out = tide(
        &bin,
        &fx.home,
        &["add", &format!("{}/.zshrc", fx.home.display())],
    );
    assert_exit(&out, 0, "tide add ~/.zshrc");

    // 2. tide scan — findings (exit 2), prefix mentioned, full token redacted.
    let out = tide(&bin, &fx.home, &["scan"]);
    assert_exit(&out, 2, "tide scan (secret)");
    let scan_out = stdout_str(&out);
    let scan_combined = format!("{scan_out}{}", stderr_str(&out));
    assert!(
        scan_combined.contains("prefix:ghp") || scan_combined.contains("prefix:ghp_"),
        "scan should mention prefix:ghp, got:\n{scan_combined}"
    );
    assert!(
        !scan_combined.contains(&full_token),
        "scan output must not contain the full token; got:\n{scan_combined}"
    );

    // 3. tide sync — blocked, no push.
    let out = tide(&bin, &fx.home, &["sync"]);
    assert_exit(&out, 2, "tide sync (secret block)");
    let sync_out = stdout_str(&out);
    let sync_err = stderr_str(&out);
    let sync_combined = format!("{sync_out}{sync_err}");
    assert!(
        !sync_out.contains("pushed: yes"),
        "sync must not report pushed: yes on secret block; stdout: {sync_out}"
    );
    assert!(
        sync_combined.to_lowercase().contains("block")
            || sync_combined.to_lowercase().contains("abort")
            || sync_combined.contains("secret"),
        "sync should report block/abort; got:\n{sync_combined}"
    );

    // 4. Remote commit count unchanged.
    let commits_after = bare_commit_count(&fx.bare);
    assert_eq!(
        commits_before, commits_after,
        "bare remote commit count must not change when secret is blocked ({commits_before} -> {commits_after})"
    );

    let _ = fx.repo;
}

// ---------------------------------------------------------------------------
// Test C — install-skill works without a config (pre-init)
// ---------------------------------------------------------------------------

struct TempHome {
    root: PathBuf,
    home: PathBuf,
}

impl TempHome {
    fn new() -> Self {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "tide-itest-{}-{}-{}",
            std::process::id(),
            n,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let home = root.join("home");
        fs::create_dir_all(&home).expect("create home");
        TempHome { root, home }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn install_skill_works_without_config() {
    // Pre-init: no ~/.config/tide/tide.toml exists. `install-skill` does not
    // depend on the config, so it must succeed before `tide init` is ever run —
    // this is what lets a fresh agent load the skill and then guide setup.
    let th = TempHome::new();
    let bin = tide_bin();
    assert!(bin.is_file(), "tide binary missing at {}", bin.display());

    // Precondition: no config present.
    assert!(
        !th.home.join(".config/tide/tide.toml").exists(),
        "precondition: no tide.toml should exist"
    );

    let out = tide(&bin, &th.home, &["install-skill"]);
    assert_exit(&out, 0, "tide install-skill without config");

    let skill = th.home.join(".agents/skills/tide/SKILL.md");
    assert!(
        skill.is_file(),
        "SKILL.md should be written at {}\nstdout:\n{}\nstderr:\n{}",
        skill.display(),
        stdout_str(&out),
        stderr_str(&out)
    );

    let body = fs::read_to_string(&skill).expect("read installed SKILL.md");
    assert!(
        body.contains("# tide"),
        "installed SKILL.md should contain the tide skill header, got:\n{body}"
    );
}

// ---------------------------------------------------------------------------
// Test D — init works without an authenticated gh (inference is non-fatal,
// never hangs on the prompt, never makes gh a hard requirement)
// ---------------------------------------------------------------------------

#[test]
fn init_works_without_gh() {
    // Hermetic regardless of the HOST's gh state: a stub `gh` is prepended to
    // PATH and always exits non-zero, so inference is skipped. stdin is null
    // so the remote prompt sees EOF (and cannot hang).
    let th = TempHome::new();
    let bin = tide_bin();
    assert!(bin.is_file(), "tide binary missing at {}", bin.display());

    // Stub gh that always fails (unauthenticated / error).
    let bin_dir = th.root.join("fakebin");
    fs::create_dir_all(&bin_dir).expect("create fakebin");
    let gh_stub = bin_dir.join("gh");
    fs::write(&gh_stub, "#!/bin/sh\nexit 1\n").expect("write gh stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&gh_stub).expect("stat gh stub").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&gh_stub, perms).expect("chmod gh stub");
    }

    let path = format!(
        "{}:{}",
        bin_dir.to_string_lossy(),
        std::env::var_os("PATH")
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );

    let out = Command::new(&bin)
        .args(["init"])
        .stdin(Stdio::null())
        .env("HOME", &th.home)
        .env("XDG_CONFIG_HOME", th.home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "tide-test")
        .env("GIT_AUTHOR_EMAIL", "tide-test@example.com")
        .env("GIT_COMMITTER_NAME", "tide-test")
        .env("GIT_COMMITTER_EMAIL", "tide-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("PATH", &path)
        .output()
        .expect("spawn tide init");

    assert_exit(&out, 0, "tide init without gh");

    let cfg_path = th.home.join(".config/tide/tide.toml");
    assert!(
        cfg_path.is_file(),
        "tide init should write config at {}",
        cfg_path.display()
    );

    let body = fs::read_to_string(&cfg_path).expect("read tide.toml");
    assert!(
        body.contains("repo_path"),
        "config should contain repo_path, got:\n{body}"
    );
    // No remote was inferred or entered (empty stdin, no gh default).
    assert!(
        !body.contains("github.com"),
        "no remote should be inferred without gh, got:\n{body}"
    );

    let repo = th.home.join(".local/share/tide/repo");
    assert!(
        repo.join(".git").exists(),
        "git repo should be initialized at {}",
        repo.display()
    );
}
