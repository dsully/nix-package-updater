use anyhow::Result;
use indicatif::ProgressBar;

use crate::clients::nix::Nix;
use crate::package::{Package, UpdateResult};
use crate::updater::{NixPackageUpdater, short_hash};

impl NixPackageUpdater {
    pub fn update_rust_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        // Get current hash before any updates
        //
        let ast_tmp = Self::ast(package);

        let Some(current_git_commit) = ast_tmp.get("rev") else {
            return Ok(UpdateResult::failed("Could not extract rev"));
        };

        let Some(latest_git_commit) = self.github_client.latest_commit(&package.homepage)? else {
            return Ok(UpdateResult::failed("Failed to fetch latest commit"));
        };

        if self.should_skip_update(&current_git_commit, &latest_git_commit) {
            return Ok(UpdateResult::up_to_date());
        }

        // Update using nurl
        let Some((new_hash, _)) = Nix::hash_and_rev(&package.homepage.to_string(), Some(&latest_git_commit))? else {
            return Ok(UpdateResult::failed("Failed to get new hash"));
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
        ast.update_vendor(&package.display_name(), &package.path, "cargo", pb)?;

        Self::write(&ast, package)?;

        Ok(UpdateResult::success()
            .version(package.version.clone(), latest_version)
            .git_commit(current_git_commit.clone(), latest_git_commit.clone()))
    }
}
