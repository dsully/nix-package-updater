#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::struct_excessive_bools
)]

mod clients;
mod config;
mod nix;
mod package;
mod updater;

use anyhow::Result;
use clap::{Command, CommandFactory, Parser, Subcommand};
use clap_complete::{Generator, Shell, generate};
use colored::Colorize;
use std::io;

use crate::updater::NixPackageUpdater;

#[derive(Parser)]
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
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Comma-separated list of packages or 'all'
    #[arg(short, long, default_value = "all", global = true)]
    packages: String,

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
}

#[derive(Subcommand)]

enum Commands {
    /// Generate shell completions
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn print_completions<G: Generator>(generator: G, cmd: &mut Command) {
    generate(generator, cmd, cmd.get_name().to_string(), &mut io::stdout());
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle completions subcommand
    if let Some(Commands::Completions { shell }) = cli.command {
        let mut cmd = Cli::command();

        eprintln!("Generating completion file for {shell}...");

        print_completions(shell, &mut cmd);

        return Ok(());
    }

    // Run the main updater logic
    if cli.verbose {
        eprintln!("{}", "Verbose mode enabled".dimmed());
    }

    let mut updater = match NixPackageUpdater::new(cli.packages, !cli.no_update, cli.force, cli.cache, cli.verbose) {
        Ok(updater) => updater,
        Err(e) => {
            eprintln!("\n{}", format!("Error initializing updater: {e}").red());

            std::process::exit(1);
        }
    };

    updater.run();

    Ok(())
}
