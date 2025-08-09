use anyhow::Result;

use crate::clients::nix::Nix;
use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;

impl NixPackageUpdater {
    pub fn update_github_package(&self, package: &Package) -> Result<UpdateResult> {
        let Some(latest_tag) = self.github_client.latest_release(&package.homepage)? else {
            // No releases found - keep current version and hash
            return Ok(UpdateResult::message("No releases found on GitHub - keeping current version"));
        };

        let latest_version = latest_tag.trim_start_matches('v').to_string();

        if self.should_skip_update(&package.version, &latest_version) {
            return Ok(UpdateResult::up_to_date());
        }

        let mut ast = Self::ast(package);

        // Update version
        ast.set("version", &package.version, &latest_version)?;

        let new_hash = Nix::hash_and_rev(&format!("{}/archive/refs/tags/{latest_tag}.tar.gz", package.homepage), None)
            .ok()
            .flatten()
            .map(|(new_hash, _)| new_hash);

        // Update hash if we have both old and new
        if let Some(new_h) = &new_hash {
            ast.set("hash", &package.nix_hash, new_h)?;
        }

        // Update platform hashes using release tag
        let release_data = serde_json::json!({
            // Use release tag for hash generation
            "tag": latest_tag,
            "repo": package.homepage.fullname,
        });

        ast.update_github_hashes(&release_data)?;

        Self::write(&ast, package)?;

        Ok(UpdateResult::success().version(package.version.clone(), latest_version))
    }
}
