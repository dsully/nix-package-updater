mod finder;
mod platform;
mod update;

use anyhow::Result;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::clients::{GitHubClient, PyPiClient};
use crate::config::Config;
use crate::nix;
use crate::package::{Package, UpdateResult};

pub struct NixPackageUpdater {
    pub packages: String,
    pub update: bool,
    pub force: bool,
    pub cache: bool,
    pub verbose: bool,
    pub config: Config,
    pub build_results_dir: PathBuf,
    pub failed_packages: Vec<String>,
    pub pypi_client: PyPiClient,
    pub github_client: GitHubClient,
}

impl NixPackageUpdater {
    #[allow(clippy::fn_params_excessive_bools)]
    pub fn new(packages: String, update: bool, force: bool, cache: bool, verbose: bool) -> Result<Self> {
        let config = Config::load();

        let build_results_dir = PathBuf::from("build-results");

        // Create build results directory
        fs::create_dir_all(&build_results_dir)?;

        Ok(Self {
            packages,
            update,
            force,
            cache,
            verbose,
            config,
            build_results_dir,
            failed_packages: Vec::new(),
            pypi_client: PyPiClient::new(),
            github_client: GitHubClient::new()?,
        })
    }

    #[allow(clippy::too_many_lines)]
    pub fn run(&mut self) {
        let mut packages = self.find_packages();

        if packages.is_empty() {
            println!("{}", "No packages found to process".yellow());

            return;
        }

        // Sort packages by name
        packages.sort_by(|a, b| a.name.cmp(&b.name));

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

        // Create shared resources for parallel processing
        let multi_progress = MultiProgress::new();

        let failed_packages = Arc::new(Mutex::new(Vec::new()));

        let config = Arc::new(self.config.clone());

        let build_results_dir = Arc::new(self.build_results_dir.clone());

        // Process packages in parallel
        let results: Vec<_> = packages
            .par_iter()
            .map(|package| {
                let pb = multi_progress.add(ProgressBar::new_spinner());

                pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());

                pb.set_message(format!("Processing {}...", package.display_name()));

                // Create a new updater instance for this thread
                let new_updater = NixPackageUpdater::new(self.packages.clone(), self.update, self.force, self.cache, self.verbose);

                let (package_update_result, build_success) = match new_updater {
                    Ok(mut updater) => {
                        // Update package
                        let update_outcome = updater.update_package(package, Some(&pb)).unwrap_or_else(|e| UpdateResult {
                            success: false,
                            old_version: None,
                            new_version: None,
                            message: Some(format!("Update error: {e}")),
                        });

                        // Build package
                        let build_success = nix::build_package(package, Some(&pb), &build_results_dir, self.cache, &config).unwrap_or(false);

                        (update_outcome, build_success)
                    }
                    Err(e) => {
                        let failed_result = UpdateResult {
                            success: false,
                            old_version: None,
                            new_version: None,
                            message: Some(format!("Updater error: {e}")),
                        };

                        (failed_result, false)
                    }
                };

                if !build_success {
                    failed_packages.lock().unwrap().push(package.name.clone());
                }

                pb.finish_and_clear();

                (package, package_update_result, build_success)
            })
            .collect();

        // Display results in sorted order
        for (package, update_result, build_success) in results {
            let update_status = if update_result.success { "✓" } else { "✗" };

            let build_status = if build_success { "✓" } else { "✗" };

            // Prepare details
            let mut details = Vec::new();

            if let (Some(old), Some(new)) = (&update_result.old_version, &update_result.new_version) {
                if old != new {
                    details.push(format!("{old} → {new}"));
                }
            }

            if let Some(msg) = &update_result.message {
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

        // Update failed packages list
        self.failed_packages.clone_from(&failed_packages.lock().unwrap());

        // Clean up if no failures
        if self.failed_packages.is_empty() {
            let _ = fs::remove_dir_all(&self.build_results_dir);
        } else {
            println!("\n{}", format!("Failed packages: {}", self.failed_packages.join(", ")).red());

            println!("{}", format!("Build logs available in {}/", self.build_results_dir.display()).yellow());
        }
    }

    fn update_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        use crate::package::PackageKind;

        if !self.update {
            return Ok(UpdateResult {
                success: true,
                old_version: None,
                new_version: None,
                message: Some("Skipping update".to_string()),
            });
        }

        match package.kind {
            PackageKind::PyPi => self.update_pypi_package(package),
            PackageKind::GitHubRelease => self.update_github_package(package),
            PackageKind::Cargo => self.update_rust_package(package, pb),
            PackageKind::Git => self.update_git_package(package, pb),
        }
    }
}
