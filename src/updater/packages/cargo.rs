use anyhow::Result;
use indicatif::ProgressBar;

use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::{NixPackageUpdater, short_hash};

impl NixPackageUpdater {
    pub fn update_rust_package(&mut self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        // Get current hash before any updates
        //
        let ast_tmp = Self::ast(package);

        let Some(current_git_commit) = ast_tmp.get("rev") else {
            package.result.failed("Could not extract rev");
            return Ok(());
        };

        let Some(latest_git_commit) = self.github_client.latest_commit(&package.homepage)? else {
            package.result.failed("Failed to fetch latest commit");
            return Ok(());
        };

        if self.should_skip_update(&current_git_commit, &latest_git_commit) {
            package.result.up_to_date();
            return Ok(());
        }

        // Update using nurl
        let Some((new_hash, _)) = Nix::hash_and_rev(&package.homepage.to_string(), Some(&latest_git_commit))? else {
            package.result.failed("Failed to get new hash");
            return Ok(());
        };

        let mut ast = Self::ast(package);

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

        Self::write(&ast, package)?;

        package
            .result
            .success()
            .version(package.version.clone(), latest_version)
            .git_commit(current_git_commit.clone(), latest_git_commit.clone());

        Ok(())
    }
}
