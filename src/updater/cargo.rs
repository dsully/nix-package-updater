use anyhow::Result;
use indicatif::ProgressBar;

use crate::Config;
use crate::clients::nix::Nix;
use crate::clients::{CratesIoClient, GitHubClient};
use crate::nix::ast::Ast;
use crate::package::Package;
use crate::updater::{Updater, short_hash};

pub struct Cargo {
    pub config: Config,
    pub github_client: GitHubClient,
    pub crates_client: CratesIoClient,
}

impl Updater for Cargo {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            github_client: GitHubClient::new()?,
            crates_client: CratesIoClient::new()?,
        })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        // Detect if this is a fetchCrate package or git-based package
        let root = package.ast.syntax();

        if Ast::contains_function_call(&root, "fetchCrate") {
            self.update_fetch_crate(package, pb)
        } else {
            self.update_git_based(package, pb)
        }
    }
}

impl Cargo {
    /// Update packages that use fetchCrate (from crates.io)
    fn update_fetch_crate(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        //
        // Query crates.io for latest version
        let Some(crate_info) = self.crates_client.crate_info(&package.name)? else {
            package.result.failed("Crate not found on crates.io");
            return Ok(());
        };

        let latest_version = &crate_info.crate_data.max_version;

        // Skip if already up to date
        if self.should_skip_update(self.config.force, &package.version, latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        // Get new hash for the crate using nurl with fetchCrate fetcher
        let Some(new_hash) = Nix::prefetch_fetchcrate(&package.name, latest_version)? else {
            package.result.failed("Failed to get hash for crate");
            return Ok(());
        };

        let mut ast = package.ast();

        if package.version != *latest_version {
            ast.set("version", &package.version, latest_version)?;
        }

        if let Some(old_hash) = ast.get("hash") {
            ast.set("hash", &old_hash, &new_hash)?;
        }

        // Clear cargoHash by finding the current value and replacing with empty string
        if let Some(old_cargo_hash) = ast.get("cargoHash") {
            ast.set("cargoHash", &old_cargo_hash, "")?;
        }

        // Update cargoHash
        ast.update_vendor(package, "cargo", pb)?;

        package.write(&ast)?;

        package.result.version(Some(package.version.as_ref()), Some(latest_version));

        Ok(())
    }

    /// Update packages that use git sources (fetchFromGitHub, etc.)
    fn update_git_based(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        // Get current hash before any updates
        //
        let ast_tmp = package.ast();

        let Some(current_git_commit) = ast_tmp.get("rev") else {
            package.result.failed("Could not extract rev");
            return Ok(());
        };

        let Some(latest_git_commit) = self.github_client.latest_commit(&package.homepage)? else {
            package.result.failed("Failed to fetch latest commit");
            return Ok(());
        };

        if self.should_skip_update(self.config.force, &current_git_commit, &latest_git_commit) {
            package.result.up_to_date();
            return Ok(());
        }

        // Update using nurl
        let Some((new_hash, _)) = Nix::hash_and_rev(&package.homepage.to_string(), Some(&latest_git_commit))? else {
            package.result.failed("Failed to get new hash");
            return Ok(());
        };

        let mut ast = package.ast();

        // Update rev and hash
        ast.update_git(Some(&current_git_commit), &latest_git_commit, &new_hash, None)?;

        // Update version if we have a new one (using the commit as version for git packages)
        let latest_version = short_hash(&latest_git_commit);

        if package.version != latest_version {
            ast.set("version", &package.version, &latest_version)?;
        }

        // Clear cargoHash by finding the current value and replacing with empty string
        if let Some(old_cargo_hash) = ast.get("cargoHash") {
            ast.set("cargoHash", &old_cargo_hash, "")?;
        }

        // Update cargoHash
        ast.update_vendor(package, "cargo", pb)?;

        package.write(&ast)?;

        package
            .result
            .git_commit(Some(current_git_commit.as_ref()), Some(latest_git_commit.as_ref()))
            .version(Some(package.version.as_ref()), Some(latest_version.as_ref()));

        Ok(())
    }
}
