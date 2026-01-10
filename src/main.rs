#![allow(clippy::module_name_repetitions, clippy::missing_errors_doc, clippy::missing_panics_doc, clippy::struct_excessive_bools)]

mod clients;
mod nix;
mod package;
mod updater;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use std::{fs, io};

use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use colored::Colorize;
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use rayon::prelude::*;
use rootcause::hooks::Hooks;
use rootcause::{Result, report};
use rootcause_backtrace::BacktraceCollector;
use rootcause_tracing::{RootcauseLayer, SpanCollector};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::nix::builder::build_package;
use crate::package::{Package, PackageKind, UpdateStatus};
use crate::updater::Updater;
use crate::updater::cargo::Cargo;
use crate::updater::git::GitRepository;
use crate::updater::github::GitHubRelease;
use crate::updater::go::GoUpdater;
use crate::updater::npm::NpmUpdater;
use crate::updater::pypi::PyPiUpdater;

#[derive(Parser, Clone, Debug, Serialize, Deserialize)]
#[command(
    name = "nix-package-updater",
    version,
    about = "Update and build Nix packages from various sources",
    long_about = r#"Nix Package Updater

Update and build Nix packages from various sources:

• PyPI packages - Updates from Python Package Index
• GitHub releases - Updates from GitHub release assets
• Rust/Cargo packages - Updates git revisions and cargo hashes
• Git packages - Updates from git repositories

Examples:

    # Update and build all packages
    nix-package-updater

    # Update specific packages
    nix-package-updater package1 package2

    # Update only PyPI packages
    nix-package-updater --type pypi

    # Build only, no updates
    nix-package-updater --build-only

    # Force update even if up to date
    nix-package-updater --force

    # Push successful builds to cachix
    nix-package-updater --cache

    # Generate shell completions
    nix-package-updater completions bash"#
)]
struct Config {
    packages: Vec<String>,

    #[arg(long, global = true)]
    exclude: Vec<String>,

    /// Skip updating packages, only build
    #[arg(long, global = true)]
    build_only: bool,

    /// Force update even if packages are up to date
    #[arg(short, long, global = true)]
    force: bool,

    /// Push successful builds to cachix
    #[arg(short, long, global = true, default_value = "true")]
    cache: bool,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Dry run - show what would be updated without making changes
    #[arg(long, global = true)]
    dry_run: bool,

    /// Generate shell completions
    #[arg(long, global = true)]
    completions: Option<String>,
}

fn init_tracing(verbose: bool) {
    let indicatif_layer = IndicatifLayer::new();

    let filter = if verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(RootcauseLayer)
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_target(false)
                .with_writer(indicatif_layer.get_stderr_writer()),
        )
        .with(indicatif_layer)
        .init();

    let _ = Hooks::new()
        .report_creation_hook(SpanCollector::new())
        .report_creation_hook(BacktraceCollector::new_from_env())
        .install();
}

fn discover_packages(config: &Config) -> Vec<Package> {
    ["packages/", "nix/packages/"]
        .iter()
        .flat_map(|&path| Package::discover(Path::new(path), &config.packages, &config.exclude))
        .collect_vec()
}

fn process_packages(packages: &mut [Package], config: &Config, build_path: &Path) {
    let multi = MultiProgress::new();

    let style = ProgressStyle::with_template("{spinner:.cyan.bold} {msg}")
        .expect("Couldn't set spinner style")
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");

    packages.par_iter_mut().for_each(|package| {
        let pb = multi.add(ProgressBar::new_spinner());
        pb.enable_steady_tick(Duration::from_millis(50));
        pb.set_style(style.clone());

        if !config.build_only {
            pb.set_message(format!("{}: Checking for version updates ...", package.name()));

            let update_result = match package.kind {
                PackageKind::PyPi => PyPiUpdater::new(config).and_then(|u| u.update(package, Some(&pb))),
                PackageKind::GitHub => GitHubRelease::new(config).and_then(|u| u.update(package, Some(&pb))),
                PackageKind::Cargo => Cargo::new(config).and_then(|u| u.update(package, Some(&pb))),
                PackageKind::Npm => NpmUpdater::new(config).and_then(|u| u.update(package, Some(&pb))),
                PackageKind::Go => GoUpdater::new(config).and_then(|u| u.update(package, Some(&pb))),
                PackageKind::Git => GitRepository::new(config).and_then(|u| u.update(package, Some(&pb))),
            };

            if let Err(e) = update_result {
                pb.suspend(|| error!(package = %package.name, "Update failed: {e}"));
                package.result.failed(format!("Update error: {e}"));
            }
        }

        if (package.result.status.contains(&UpdateStatus::Updated) || config.force || config.build_only)
            && let Err(e) = build_package(package, &pb, build_path, config.cache)
        {
            pb.suspend(|| error!(package = %package.name, "Build failed: {e}"));
            package.result.failed(format!("Build error: {e}"));
        }

        pb.finish_and_clear();
    });
}

fn print_results(packages: &[Package]) {
    println!(
        "{:<30} {:<8} {:<8} {:<8} {:<8} Details",
        "Package".bright_white().bold(),
        "Source".bright_white().bold(),
        "Updated".bright_white().bold(),
        "Built".bright_white().bold(),
        "Cached".bright_white().bold()
    );

    println!("{}", "-".repeat(74));

    packages
        .iter()
        .filter(|package| !package.is_up_to_date())
        .sorted_by(|a, b| a.name.cmp(&b.name))
        .for_each(|package| {
            let mut details = Vec::new();

            if !package.result.changes.is_empty() {
                details.push(package.result.changes.join(", "));
            }

            if let Some(msg) = &package.result.message {
                details.push(msg.clone());
            }

            println!(
                "{} {:<8} {:<8} {:<8} {:<8} {}",
                format_args!("{}{}", package.name(), " ".repeat(30 - package.display_width())),
                package.kind.to_string().magenta(),
                package.result.status(UpdateStatus::Updated),
                package.result.status(UpdateStatus::Built),
                package.result.status(UpdateStatus::Cached),
                details.join("\n")
            );
        });
}

fn main() -> Result<()> {
    let strategy = choose_base_strategy().expect("Unable to find base strategy");
    let path = strategy.config_dir().join("nix-updater").join("config.toml");

    let config: Config = Figment::new()
        .merge(Serialized::defaults(Config::parse()))
        .merge(Toml::file(path))
        .merge(Env::prefixed("NIX_UPDATER_").split("_"))
        .extract()?;

    init_tracing(config.verbose);

    if let Some(shell) = config.completions {
        let mut cmd = Config::command();
        let name = &cmd.get_name().to_string();

        info!("Generating completion file for {shell}...");

        let shell_type = Shell::from_str(&shell).map_err(|_| report!("Invalid shell: {shell}. Valid shells: bash, zsh, fish, elvish, powershell"))?;

        generate(shell_type, &mut cmd, name, &mut io::stdout());

        return Ok(());
    }

    let mut packages = discover_packages(&config);

    if packages.is_empty() {
        println!("{}", "No packages found to process".yellow());
        return Ok(());
    }

    let build_path = PathBuf::from("build-results");

    process_packages(&mut packages, &config, &build_path);

    if packages.iter().all(|p| p.result.status.contains(&UpdateStatus::UpToDate)) {
        println!("{}", "No packages needed updating.".yellow());
        return Ok(());
    }

    print_results(&packages);

    if packages.iter().all(|p| p.result.status.contains(&UpdateStatus::Built))
        && let Err(e) = fs::remove_dir_all(&build_path)
    {
        warn!("Failed to remove build directory: {e}");
    }

    Ok(())
}
