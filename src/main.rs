#![allow(clippy::module_name_repetitions, clippy::missing_errors_doc, clippy::missing_panics_doc, clippy::struct_excessive_bools)]

mod clients;
mod nix;
mod package;
mod updater;

use std::io;
use std::str::FromStr;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use colored::Colorize;
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::{Deserialize, Serialize};

use crate::updater::NixPackageUpdater;

#[derive(Parser, Clone, Debug, Serialize, Deserialize)]
#[command(
    name = "nix-updater",
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
    ./update

    # Update specific packages
    ./update --packages "package1,package2"

    # Update only PyPI packages
    ./update --type pypi

    # Build only, no updates
    ./update --no-update

    # Force update even if up to date
    ./update --force

    # Push successful builds to cachix
    ./update --cache

    # Generate shell completions
    ./update completions bash"#
)]
struct Config {
    packages: Vec<String>,

    #[arg(long, global = true)]
    cachix_name: Option<String>,

    #[arg(long, global = true)]
    exclude: Vec<String>,

    /// Skip updating packages, only build
    #[arg(long, global = true)]
    no_update: bool,

    /// Force update even if packages are up to date
    #[arg(short, long, global = true)]
    force: bool,

    /// Push successful builds to cachix
    #[arg(short, long, global = true)]
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

fn main() -> Result<()> {
    let strategy = choose_base_strategy().expect("Unable to find base strategy");
    let path = strategy.config_dir().join("nix-updater").join("config.toml");

    let config: Config = Figment::new()
        .merge(Serialized::defaults(Config::parse()))
        .merge(Toml::file(path))
        .merge(Env::prefixed("NIX_UPDATER_").split("_"))
        .extract()?;

    // Handle completions subcommand
    if let Some(shell) = config.completions {
        let mut cmd = Config::command();
        let name = &cmd.get_name().to_string();

        eprintln!("Generating completion file for {shell}...");

        generate(Shell::from_str(&shell).expect("Invalid shell!"), &mut cmd, name, &mut io::stdout());

        return Ok(());
    }

    let mut updater = match NixPackageUpdater::new(config) {
        Ok(updater) => updater,
        Err(e) => {
            eprintln!("\n{}", format!("Error initializing updater: {e}").red());

            std::process::exit(1);
        }
    };

    updater.run();

    Ok(())
}
