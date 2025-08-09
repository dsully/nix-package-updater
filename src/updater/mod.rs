mod packages;

use anyhow::Result;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::Config;
use crate::clients::{GitHubClient, PyPiClient};
use crate::nix::ast::Ast;
use crate::nix::builder::build_package;
use crate::package::{Package, UpdateResult};

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

    #[allow(clippy::too_many_lines)]
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

        let config = Arc::new(self.config.clone());
        let mut cleanup = true;

        // Process packages in parallel
        let results: Vec<_> = packages
            .par_iter()
            .map(|package| {
                let config_clone = Arc::clone(&config);

                let pb = multi_progress.add(ProgressBar::new_spinner());

                pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());

                pb.set_message(format!("Processing {}...", package.display_name()));

                // Create a new updater instance for this thread
                let new_updater = NixPackageUpdater::new(self.config.clone());

                let update = match new_updater {
                    Ok(mut updater) => {
                        let mut update = updater
                            .update_package(package, Some(&pb))
                            .unwrap_or_else(|e| UpdateResult::failed(format!("Updater error: {e}")));

                        if update.updated {
                            update.built = build_package(package, Some(&pb), &self.build_path, self.config.cache, &config_clone).unwrap_or(false);
                        }

                        update
                    }
                    Err(e) => UpdateResult::failed(format!("Updater error: {e}")),
                };

                pb.finish_and_clear();

                (package, update)
            })
            .collect();

        //
        println!("\n{}", "Package Update Results".bold());

        println!("{}", "-".repeat(80));

        println!(
            "{:<30} {:<15} {:<10} {:<10} Details",
            "Package".cyan(),
            "Type".magenta(),
            "Update".yellow(),
            "Build".green()
        );

        println!("{}", "-".repeat(80));

        // Display results in sorted order
        for (package, result) in results {
            let update_status = if result.updated { "✓" } else { "✗" };
            let build_status = if result.built { "✓" } else { "✗" };

            if !result.built {
                cleanup = false;
            }

            let mut details = Vec::new();

            let changes: Vec<_> = [
                match (&result.old_version, &result.new_version) {
                    (Some(old_v), Some(new_v)) if old_v != new_v => Some(format!("{old_v} → {new_v}")),
                    _ => None,
                },
                match (&result.old_git_commit, &result.new_git_commit) {
                    (Some(old_h), Some(new_h)) if old_h != new_h => Some(format!("{} → {}", short_hash(old_h), short_hash(new_h))),
                    _ => None,
                },
            ]
            .into_iter()
            .flatten()
            .collect();

            if !changes.is_empty() {
                details.push(changes.join(", "));
            }

            if let Some(msg) = &result.message {
                details.push(msg.clone());
            }

            // Create hyperlinked package name if homepage is available
            let package_name_display = package.display_name();
            let package_name_width = package.display_width();

            // Manually pad the package name to account for escape sequences
            let package_name_padded = if package_name_width < 30 {
                format!("{}{}", package_name_display, " ".repeat(30 - package_name_width))
            } else {
                package_name_display
            };

            println!(
                "{} {:<15} {:<10} {:<10} {}",
                package_name_padded,
                package.kind.to_string().magenta(),
                update_status.yellow(),
                build_status.green(),
                details.join("\n")
            );
        }

        println!("{}", "-".repeat(80));

        if cleanup {
            let _ = fs::remove_dir_all(&self.build_path);
        }
    }

    fn update_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        use crate::package::PackageKind;

        if self.config.no_update {
            return Ok(UpdateResult::message("Skipping update"));
        }

        match package.kind {
            PackageKind::PyPi => self.update_pypi_package(package),
            PackageKind::GitHubRelease => self.update_github_package(package),
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
