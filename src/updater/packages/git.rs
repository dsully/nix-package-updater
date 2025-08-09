use anyhow::Result;
use indicatif::ProgressBar;

use crate::clients::nix::Nix;
use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;

impl NixPackageUpdater {
    pub fn update_git_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        //
        // Use nurl to get new hash/rev
        let Some((new_hash, new_rev)) = Nix::hash_and_rev(&package.homepage.to_string(), None)? else {
            return Ok(UpdateResult::failed("nurl failed"));
        };

        let ast_tmp = Self::ast(package);
        let current_rev = ast_tmp.get("rev");

        if package.nix_hash == new_hash && current_rev == new_rev && !self.config.force {
            return Ok(UpdateResult::up_to_date());
        }

        let mut ast = Self::ast(package);

        // Update rev and hash
        ast.update_git(current_rev.as_deref(), &new_rev.clone().unwrap_or_default(), &new_hash, Some(&package.nix_hash))?;

        // Clear cargo/vendor hashes if they exist
        if let Some(old_vendor) = ast.get("vendorHash") {
            ast.set("vendorHash", &old_vendor, "")?;
        }

        // Check if we need to update cargo/vendor hash before writing
        if ast.get("cargoHash").is_some() {
            ast.update_vendor(&package.display_name(), &package.path, "cargo", pb)?;
        }

        Self::write(&ast, package)?;

        let mut result = UpdateResult::success();

        if let (Some(old_rev), Some(new_rev)) = (current_rev, new_rev) {
            result = result.git_commit(old_rev, new_rev);
        }

        Ok(result)
    }
}
