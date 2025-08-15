mod packages;

use anyhow::Result;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;

use crate::Config;
use crate::clients::{GitHubClient, PyPiClient};
use crate::nix::ast::Ast;
use crate::nix::builder::build_package;
use crate::package::{Package, PackageKind};

pub struct NixPackageUpdater {
    pub config: Config,
    pub build_path: PathBuf,
    pub pypi_client: PyPiClient,
    pub github_client: GitHubClient,
}

impl NixPackageUpdater {
    pub fn new(config: Config) -> Result<Self> {
        let path = PathBuf::from("build-results");

        fs::create_dir_all(&path)?;

        Ok(Self {
            config,
            build_path: path,
            pypi_client: PyPiClient::new(),
            github_client: GitHubClient::new()?,
        })
    }

    pub fn run(&mut self) {
        let mut packages = Package::discover(&self.config.packages, &self.config.exclude);

        if packages.is_empty() {
            println!("{}", "No packages found to process".yellow());
            return;
        }

        // Sort packages by name
        packages.sort_by(|a, b| a.name.cmp(&b.name));

        // Create shared resources for parallel processing
        let multi_progress = MultiProgress::new();

        let _: Vec<_> = packages
            .par_iter_mut()
            .map(|package| {
                let pb = multi_progress.add(ProgressBar::new_spinner());

                pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());

                pb.set_message(format!("Processing {}...", package.name()));

                if let Ok(mut updater) = NixPackageUpdater::new(self.config.clone()) {
                    //
                    if let Err(e) = updater.update_package(package, Some(&pb)) {
                        package.result.failed(format!("Updater error: {e}"));
                    } else if package.result.updated {
                        let _ = build_package(package, Some(&pb), &self.build_path, self.config.cache);
                    }
                }

                pb.finish_and_clear();
            })
            .collect();

        println!(
            "{:<30} {:<8} {:<8} {:<8} {:<8} Details",
            "Package".bright_white().bold(),
            "Source".bright_white().bold(),
            "Updated".bright_white().bold(),
            "Built".bright_white().bold(),
            "Cached".bright_white().bold()
        );

        println!("{}", "-".repeat(72));

        for package in &packages {
            let mut details = Vec::new();

            let changes = package.result.changes();

            if !changes.is_empty() {
                details.push(changes.join(", "));
            }

            if let Some(msg) = &package.result.message {
                details.push(msg.clone());
            }

            println!(
                "{} {:<8} {:<8} {:<8} {:<8} {}",
                // Pad the package name to account for the OSC-8 codes.
                format_args!("{}{}", package.name(), " ".repeat(30 - package.display_width())),
                package.kind.to_string().magenta(),
                package.result.status(package.result.updated),
                package.result.status(package.result.built),
                package.result.status(package.result.cached),
                details.join("\n")
            );
        }

        if packages.iter().all(|p| p.result.built) {
            fs::remove_dir_all(&self.build_path).expect("Failed to remove build directory");
        }
    }

    fn update_package(&mut self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        if self.config.no_update {
            package.result.message("Skipping update");

            return Ok(());
        }

        match package.kind {
            PackageKind::PyPi => self.update_pypi_package(package),
            PackageKind::GitHub => self.update_github_package(package),
            PackageKind::Cargo => self.update_rust_package(package, pb),
            PackageKind::Git => self.update_git_package(package, pb),
        }
    }

    /// Common helper to check if update should be skipped
    fn should_skip_update(&self, current: &str, latest: &str) -> bool {
        current == latest && !self.config.force
    }

    /// Common helper to create an Ast from a package
    fn ast(package: &Package) -> Ast {
        Ast::from_ast(package.ast.clone())
    }

    /// Common helper to finalize an Ast by writing to file
    fn write(ast: &Ast, package: &Package) -> Result<()> {
        Ok(std::fs::write(&package.path, ast.content())?)
    }
}

/// Create a short git hash (first 8 characters) from a full hash or revision
pub fn short_hash(hash: &str) -> String {
    hash.strip_prefix("sha256-").unwrap_or(hash).chars().take(8).collect()
}
