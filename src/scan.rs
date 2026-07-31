use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use regex::{Regex, RegexSet};

use crate::config::Config;
use crate::repo;

#[derive(Debug, Clone)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: String,    // "prefix:AKIA" | "entropy" | "regex" | "gitleaks" | "trufflehog"
    pub snippet: String, // REDACTED, <= ~40 chars
}

// ---------------------------------------------------------------------------
// Known secret prefixes (compiled once)
// ---------------------------------------------------------------------------

struct PrefixEngine {
    set: RegexSet,
    /// Individual regexes aligned with `set` pattern order, for capture.
    regs: Vec<Regex>,
    /// Kind tokens aligned with pattern order, e.g. "AKIA".
    tokens: Vec<&'static str>,
}

fn prefix_engine() -> &'static PrefixEngine {
    static ENGINE: OnceLock<PrefixEngine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        // (pattern, kind token after "prefix:")
        let specs: &[(&str, &str)] = &[
            (r"AKIA[0-9A-Z]{16}", "AKIA"),
            (r"ghp_[A-Za-z0-9]{36,}", "ghp_"),
            (r"gho_[A-Za-z0-9]{36,}", "gho_"),
            (r"ghu_[A-Za-z0-9]{36,}", "ghu_"),
            (r"ghs_[A-Za-z0-9]{36,}", "ghs_"),
            (r"ghr_[A-Za-z0-9]{36,}", "ghr_"),
            (r"sk-[A-Za-z0-9]{20,}", "sk-"),
            (r"xox[bpoa]-[A-Za-z0-9-]{10,}", "xox"),
            (r"glpat-[A-Za-z0-9_-]{20}", "glpat-"),
            (r"AIza[0-9A-Za-z_-]{35}", "AIza"),
            (r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}", "eyJ"),
            (r"-----BEGIN [A-Z ]*PRIVATE KEY-----", "BEGIN"),
        ];
        let patterns: Vec<&str> = specs.iter().map(|(p, _)| *p).collect();
        let set = RegexSet::new(&patterns).expect("prefix patterns must compile");
        let regs: Vec<Regex> = patterns
            .iter()
            .map(|p| Regex::new(p).expect("prefix pattern must compile"))
            .collect();
        let tokens: Vec<&'static str> = specs.iter().map(|(_, t)| *t).collect();
        PrefixEngine { set, regs, tokens }
    })
}

// ---------------------------------------------------------------------------
// Shannon entropy
// ---------------------------------------------------------------------------

fn shannon_entropy(s: &[u8]) -> f64 {
    let mut counts = [0u32; 256];
    for &b in s {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

fn all_identical(s: &[u8]) -> bool {
    match s.first() {
        None => true,
        Some(&first) => s.iter().all(|&b| b == first),
    }
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

/// Keep at most first 4 and last 4 chars of the secret, middle → `…`. Cap ~40.
fn redact(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    let n = chars.len();
    let redacted = if n == 0 {
        String::new()
    } else if n <= 4 {
        format!("{}…", chars.iter().collect::<String>())
    } else if n <= 8 {
        let head: String = chars.iter().take(4).collect();
        format!("{head}…")
    } else {
        let head: String = chars.iter().take(4).collect();
        let tail: String = chars[n - 4..].iter().collect();
        format!("{head}…{tail}")
    };
    // Cap overall snippet length (~40 chars).
    let capped: String = redacted.chars().take(40).collect();
    capped
}

// ---------------------------------------------------------------------------
// Diff-aware line iteration
// ---------------------------------------------------------------------------

/// Parse the new-file start line from a hunk header like `@@ -1,2 +3,4 @@`.
fn parse_hunk_new_start(header: &str) -> Option<usize> {
    // Find `+<num>` after the first space-ish content.
    let plus = header.find('+')?;
    let rest = &header[plus + 1..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    num.parse().ok()
}

struct LineCtx<'a> {
    content: &'a str,
    file: String,
    line: usize,
}

/// Yield (content, file, line) for each scannable line in `text`.
/// When unified-diff hunks are present, uses `+++ b/<path>` and `@@ +l @@`.
/// Otherwise falls back to `file_label` and 1-based sequential line numbers.
fn iter_scannable_lines<'a>(text: &'a str, file_label: &str) -> Vec<LineCtx<'a>> {
    let mut out = Vec::new();
    let mut file = file_label.to_string();
    let mut new_line: usize = 0;
    let mut in_hunk = false;
    let mut plain_line: usize = 0;
    let mut saw_diff = false;

    for raw in text.lines() {
        if let Some(path) = raw.strip_prefix("+++ b/") {
            file = path.to_string();
            saw_diff = true;
            in_hunk = false;
            continue;
        }
        if raw.starts_with("+++ ") {
            saw_diff = true;
            in_hunk = false;
            continue;
        }
        if raw.starts_with("@@") {
            saw_diff = true;
            new_line = parse_hunk_new_start(raw).unwrap_or(0);
            in_hunk = true;
            continue;
        }

        if in_hunk {
            if let Some(content) = raw.strip_prefix('+') {
                // Added line (not +++ — those handled above).
                out.push(LineCtx {
                    content,
                    file: file.clone(),
                    line: new_line,
                });
                new_line = new_line.saturating_add(1);
            } else if let Some(content) = raw.strip_prefix('-') {
                // Deleted line — still scan (defensive) but don't advance new_line.
                out.push(LineCtx {
                    content,
                    file: file.clone(),
                    line: 0,
                });
            } else if let Some(content) = raw.strip_prefix(' ') {
                out.push(LineCtx {
                    content,
                    file: file.clone(),
                    line: new_line,
                });
                new_line = new_line.saturating_add(1);
            }
            // Other hunk lines (e.g. `\ No newline`) skipped.
            continue;
        }

        // Non-diff (or pre-hunk) lines.
        if saw_diff {
            // Diff metadata before/between hunks — skip.
            continue;
        }
        plain_line += 1;
        out.push(LineCtx {
            content: raw,
            file: file_label.to_string(),
            line: plain_line,
        });
    }

    out
}

// ---------------------------------------------------------------------------
// Core detectors
// ---------------------------------------------------------------------------

fn scan_line_prefixes(content: &str, file: &str, line: usize, out: &mut Vec<Finding>) {
    let eng = prefix_engine();
    let matched = eng.set.matches(content);
    for idx in matched {
        for m in eng.regs[idx].find_iter(content) {
            out.push(Finding {
                file: file.to_string(),
                line,
                kind: format!("prefix:{}", eng.tokens[idx]),
                snippet: redact(m.as_str()),
            });
        }
    }
}

fn scan_line_entropy(
    content: &str,
    file: &str,
    line: usize,
    min_len: usize,
    threshold: f64,
    entropy_re: &Regex,
    out: &mut Vec<Finding>,
) {
    for m in entropy_re.find_iter(content) {
        let s = m.as_str();
        if s.len() < min_len {
            continue;
        }
        let bytes = s.as_bytes();
        if all_identical(bytes) {
            continue;
        }
        if shannon_entropy(bytes) > threshold {
            out.push(Finding {
                file: file.to_string(),
                line,
                kind: "entropy".to_string(),
                snippet: redact(s),
            });
        }
    }
}

fn scan_line_regex(
    content: &str,
    file: &str,
    line: usize,
    set: &RegexSet,
    regs: &[Regex],
    out: &mut Vec<Finding>,
) {
    if set.is_empty() {
        return;
    }
    let matched = set.matches(content);
    for idx in matched {
        for m in regs[idx].find_iter(content) {
            out.push(Finding {
                file: file.to_string(),
                line,
                kind: "regex".to_string(),
                snippet: redact(m.as_str()),
            });
        }
    }
}

fn compile_user_patterns(patterns: &[String]) -> (RegexSet, Vec<Regex>) {
    let mut valid_strs: Vec<String> = Vec::new();
    let mut regs: Vec<Regex> = Vec::new();
    for p in patterns {
        match Regex::new(p) {
            Ok(re) => {
                valid_strs.push(p.clone());
                regs.push(re);
            }
            Err(e) => {
                tracing::warn!(pattern = %p, error = %e, "invalid secret_pattern; skipping");
            }
        }
    }
    let set = RegexSet::new(&valid_strs).unwrap_or_else(|_| {
        RegexSet::new(Vec::<&str>::new()).expect("empty RegexSet always compiles")
    });
    (set, regs)
}

// ---------------------------------------------------------------------------
// External scanners (PATH probed once)
// ---------------------------------------------------------------------------

struct ExternalAvailability {
    gitleaks: bool,
    trufflehog: bool,
}

fn external_availability() -> &'static ExternalAvailability {
    static AVAIL: OnceLock<ExternalAvailability> = OnceLock::new();
    AVAIL.get_or_init(|| ExternalAvailability {
        gitleaks: binary_on_path("gitleaks"),
        trufflehog: binary_on_path("trufflehog"),
    })
}

fn binary_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
        // Windows-style .exe not needed on Linux; still check bare name only.
    }
    false
}

/// Minimal JSON string-field extractor (no serde_json in deps).
fn json_string_field(obj: &str, key: &str) -> Option<String> {
    // "key"\s*:\s*"((?:\\.|[^"\\])*)"
    let pat = format!(r#""{}"\s*:\s*"((?:\\.|[^"\\])*)""#, regex::escape(key));
    let re = Regex::new(&pat).ok()?;
    let caps = re.captures(obj)?;
    let raw = caps.get(1)?.as_str();
    Some(json_unescape(raw))
}

fn json_usize_field(obj: &str, key: &str) -> Option<usize> {
    let pat = format!(r#""{}"\s*:\s*(\d+)"#, regex::escape(key));
    let re = Regex::new(&pat).ok()?;
    let caps = re.captures(obj)?;
    caps.get(1)?.as_str().parse().ok()
}

fn json_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('u') => {
                    // Skip simple \uXXXX
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(cp) = u32::from_str_radix(&hex, 16)
                        && let Some(ch) = char::from_u32(cp)
                    {
                        out.push(ch);
                    }
                }
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Split a JSON array (or concatenated objects) into top-level `{...}` objects.
fn json_objects(blob: &str) -> Vec<&str> {
    let mut objs = Vec::new();
    let bytes = blob.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i;
            let mut depth = 0i32;
            let mut in_str = false;
            let mut escape = false;
            while i < bytes.len() {
                let b = bytes[i];
                if in_str {
                    if escape {
                        escape = false;
                    } else if b == b'\\' {
                        escape = true;
                    } else if b == b'"' {
                        in_str = false;
                    }
                } else {
                    match b {
                        b'"' => in_str = true,
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                // Safe: we only split on ASCII braces outside strings.
                                objs.push(&blob[start..=i]);
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    objs
}

fn run_gitleaks(repo: &Path) -> Vec<Finding> {
    let output = match Command::new("gitleaks")
        .args([
            "detect",
            "--source",
            &repo.to_string_lossy(),
            "--report-format",
            "json",
            "--report-path",
            "-",
            "--no-git",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %e, "gitleaks failed to start");
            return Vec::new();
        }
    };

    // gitleaks exits non-zero when secrets are found; still parse stdout/report.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Report may be on stdout; some versions write only on success path.
    let blob = if !stdout.trim().is_empty() {
        stdout.as_ref()
    } else {
        stderr.as_ref()
    };

    if !output.status.success()
        && stdout.trim().is_empty()
        && !blob.trim().starts_with('[')
        && !blob.contains("RuleID")
    {
        // Real error (not "leaks found").
        if !stderr.trim().is_empty() {
            tracing::warn!(
                status = ?output.status.code(),
                stderr = %stderr.trim(),
                "gitleaks error; continuing without its findings"
            );
        }
        // Still try to parse if anything looks like JSON below.
    }

    let mut findings = Vec::new();
    for obj in json_objects(blob) {
        let secret = json_string_field(obj, "Secret")
            .or_else(|| json_string_field(obj, "Match"))
            .unwrap_or_default();
        if secret.is_empty() {
            continue;
        }
        let file = json_string_field(obj, "File").unwrap_or_else(|| "repo".to_string());
        let line = json_usize_field(obj, "StartLine").unwrap_or(0);
        let _rule = json_string_field(obj, "RuleID");
        findings.push(Finding {
            file,
            line,
            kind: "gitleaks".to_string(),
            snippet: redact(&secret),
        });
    }
    findings
}

fn run_trufflehog(repo: &Path) -> Vec<Finding> {
    let output = match Command::new("trufflehog")
        .args([
            "filesystem",
            "--directory",
            &repo.to_string_lossy(),
            "--json",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            // Older/newer CLIs may use --dir instead of --directory.
            match Command::new("trufflehog")
                .args(["filesystem", "--dir", &repo.to_string_lossy(), "--json"])
                .output()
            {
                Ok(o) => o,
                Err(e2) => {
                    tracing::warn!(error = %e, error2 = %e2, "trufflehog failed to start");
                    return Vec::new();
                }
            }
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // trufflehog may exit non-zero with findings; only warn if no JSON on stdout.
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            tracing::warn!(
                status = ?output.status.code(),
                stderr = %stderr.trim(),
                "trufflehog error; continuing without its findings"
            );
            return Vec::new();
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut findings = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Prefer Raw / RawV2 as the secret body.
        let secret = json_string_field(line, "Raw")
            .or_else(|| json_string_field(line, "RawV2"))
            .or_else(|| json_string_field(line, "Redacted"))
            .unwrap_or_default();
        if secret.is_empty() {
            // Still record detector hit if present.
            if json_string_field(line, "DetectorName").is_none() {
                continue;
            }
        }
        let file = json_string_field(line, "file")
            .or_else(|| json_string_field(line, "File"))
            .unwrap_or_else(|| "repo".to_string());
        let line_no = json_usize_field(line, "line")
            .or_else(|| json_usize_field(line, "Line"))
            .unwrap_or(0);
        let snippet = if secret.is_empty() {
            json_string_field(line, "DetectorName").unwrap_or_else(|| "…".to_string())
        } else {
            redact(&secret)
        };
        findings.push(Finding {
            file,
            line: line_no,
            kind: "trufflehog".to_string(),
            snippet: {
                let s: String = snippet.chars().take(40).collect();
                s
            },
        });
    }
    findings
}

fn run_external(repo: &Path) -> Vec<Finding> {
    let avail = external_availability();
    let mut all = Vec::new();
    if avail.gitleaks {
        all.extend(run_gitleaks(repo));
    }
    if avail.trufflehog {
        all.extend(run_trufflehog(repo));
    }
    all
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Pure scan of a text blob (e.g. a `git diff`). Applies known prefixes +
/// high-entropy runs + config regex. If `external == true` AND a scanner binary
/// is on PATH, also shells out to gitleaks/trufflehog and merges findings.
pub fn scan_text(text: &str, file_label: &str, cfg: &Config, external: bool) -> Vec<Finding> {
    let mut findings = Vec::new();

    let eng_entropy = Regex::new(&format!(
        r"[A-Za-z0-9+/=_-]{{{},}}",
        cfg.entropy_min_length.max(1)
    ))
    .expect("entropy regex must compile");

    let (user_set, user_regs) = compile_user_patterns(&cfg.secret_patterns);

    for ctx in iter_scannable_lines(text, file_label) {
        scan_line_prefixes(ctx.content, &ctx.file, ctx.line, &mut findings);
        scan_line_entropy(
            ctx.content,
            &ctx.file,
            ctx.line,
            cfg.entropy_min_length,
            cfg.entropy_threshold,
            &eng_entropy,
            &mut findings,
        );
        scan_line_regex(
            ctx.content,
            &ctx.file,
            ctx.line,
            &user_set,
            &user_regs,
            &mut findings,
        );
    }

    if external {
        let repo: PathBuf = repo::repo_path(cfg);
        if repo.is_dir() {
            findings.extend(run_external(&repo));
        }
    }

    findings
}

/// `tide scan` CLI handler (called by main).
pub fn run(cfg: &Config) -> anyhow::Result<()> {
    let repo = repo::repo_path(cfg);
    repo::copy_watched_into_repo(cfg)?;
    repo::add_all(&repo)?;
    let diff = repo::staged_diff(&repo)?;
    let findings = scan_text(&diff, "staged", cfg, true);
    if findings.is_empty() {
        println!("clean");
        return Ok(());
    }
    for f in &findings {
        println!("{}:{} {}: {}", f.file, f.line, f.kind, f.snippet);
    }
    std::process::exit(2);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> Config {
        Config::default()
    }

    #[test]
    fn detects_known_prefix_akia() {
        let cfg = test_cfg();
        // AKIA + 16 alphanumeric
        let text = "export AWS_KEY=AKIAIOSFODNN7EXAMPLE";
        let findings = scan_text(text, "test", &cfg, false);
        assert!(
            findings.iter().any(|f| f.kind.starts_with("prefix:")),
            "expected prefix finding, got: {:?}",
            findings
        );
        let f = findings
            .iter()
            .find(|f| f.kind.starts_with("prefix:"))
            .unwrap();
        assert_eq!(f.kind, "prefix:AKIA");
        // Snippet must be redacted (not the full secret).
        assert!(!f.snippet.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(f.snippet.contains('…'));
    }

    #[test]
    fn detects_high_entropy() {
        let cfg = test_cfg();
        // 40-char mixed base64-ish run; entropy well above 4.5.
        let secret = "aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3xY5zA7b";
        assert!(
            secret.len() >= 40,
            "test secret too short: {}",
            secret.len()
        );
        // Avoid default regex (token|password|...) by using a neutral label.
        let text = format!("blob={secret}");
        let findings = scan_text(&text, "test", &cfg, false);
        assert!(
            findings.iter().any(|f| f.kind == "entropy"),
            "expected entropy finding for {secret} (H={:.3}), got: {:?}",
            shannon_entropy(secret.as_bytes()),
            findings
        );
    }

    #[test]
    fn detects_config_regex() {
        let cfg = test_cfg();
        // Default pattern: (?i)(token|password|secret|api[_-]?key)\s*=
        let text = "TOKEN=abc123";
        let findings = scan_text(text, "test", &cfg, false);
        assert!(
            findings.iter().any(|f| f.kind == "regex"),
            "expected regex finding, got: {:?}",
            findings
        );
    }

    #[test]
    fn clean_string_empty() {
        let cfg = test_cfg();
        let text = "hello world\njust a normal config line\nfoo=bar\n";
        let findings = scan_text(text, "test", &cfg, false);
        assert!(
            findings.is_empty(),
            "expected no findings, got: {:?}",
            findings
        );
    }

    #[test]
    fn redact_never_shows_full_secret() {
        let s = "AKIAIOSFODNN7EXAMPLE";
        let r = redact(s);
        assert!(r.len() < s.len() || r.contains('…'));
        assert!(!r.contains("IOSFODNN7"));
        assert!(r.chars().count() <= 40);
    }

    #[test]
    fn shannon_identical_is_zero() {
        assert!((shannon_entropy(b"aaaaaaaaaa") - 0.0).abs() < 1e-9);
        assert!(all_identical(b"aaaaaaaaaa"));
    }
}
