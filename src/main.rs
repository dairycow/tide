mod config;
mod diff;
mod doctor;
mod install;
mod repo;
mod scan;
mod sync;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "tide",
    about = "One-shot dotfile sync CLI — copy, commit, and push watched dotfiles to your own git repo",
    after_help = "Typical flow:\n  tide init          # set up config + repo + origin, adopt dotfiles\n  tide add ~/.bashrc # watch a dotfile\n  tide diff          # review what would be uploaded\n  tide sync          # secret-scan, commit, push"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Set up tide: write config, init the repo, set origin (default <you>/dotfiles), and offer to adopt detected dotfiles
    Init,
    /// Watch a home dotfile — register it and copy it into the repo now
    Add {
        /// Home path of the dotfile to watch (e.g. ~/.bashrc)
        path: std::path::PathBuf,
    },
    /// Stop watching a dotfile (the home file and repo copy are left as-is)
    Rm {
        /// Home path of the dotfile to stop watching
        path: std::path::PathBuf,
    },
    /// List watched dotfiles (one path per line)
    List,
    /// Show what would be uploaded — staged diff, no commit or push
    Diff,
    /// Copy changed dotfiles into the repo, secret-scan, commit, and push
    Sync,
    /// Check binary, repo, origin URL, credentials, watched files, and scanners
    Doctor,
    /// Install the agent skill into ~/.agents/skills/tide/
    #[command(name = "install-skill")]
    InstallSkill,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    if let Err(e) = dispatch(cli) {
        eprintln!("{e:#}");
        let code = if e.to_string().contains("no config found") {
            2
        } else {
            1
        };
        std::process::exit(code);
    }
}

fn load_cfg_or_hint() -> anyhow::Result<config::Config> {
    if !config::config_path().exists() {
        eprintln!("no config found; run `tide init` first");
        std::process::exit(2);
    }
    config::load()
}

fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match cli.cmd {
        Cmd::Init => config::cmd_init(None),
        Cmd::Add { path } => config::cmd_add(path),
        Cmd::Rm { path } => config::cmd_rm(path),
        Cmd::List => config::cmd_list(),
        Cmd::Diff => {
            let cfg = load_cfg_or_hint()?;
            diff::run(&cfg)
        }
        Cmd::Sync => {
            let cfg = load_cfg_or_hint()?;
            sync::run(&cfg)
        }
        Cmd::Doctor => {
            let cfg = load_cfg_or_hint()?;
            doctor::run(&cfg)
        }
        Cmd::InstallSkill => install::run(),
    }
}
