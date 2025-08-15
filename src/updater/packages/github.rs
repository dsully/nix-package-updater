use anyhow::Result;

use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::NixPackageUpdater;

impl NixPackageUpdater {
    pub fn update_github_package(&self, package: &mut Package) -> Result<()> {
        //
        let Some(latest_tag) = self.github_client.latest_release(&package.homepage)? else {
            package.result.message("No releases found on GitHub - keeping current version");
            return Ok(());
        };

        let latest_version = latest_tag.trim_start_matches('v').to_string();

        if self.should_skip_update(&package.version, &latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        let mut ast = Self::ast(package);

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

        package.result.success().version(package.version.clone(), latest_version);

        Ok(())
    }
}
