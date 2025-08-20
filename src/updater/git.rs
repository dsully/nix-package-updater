use anyhow::Result;
use indicatif::ProgressBar;

use crate::Config;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::Updater;

pub struct GitRepository {
    pub config: Config,
}

impl Updater for GitRepository {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self { config: config.clone() })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        //
        // Use nurl to get new hash/rev
        let Some((new_hash, new_rev)) = Nix::hash_and_rev(&package.homepage.to_string(), None)? else {
            package.result.failed("nurl failed");
            return Ok(());
        };

        let mut ast = package.ast();
        let old_rev = ast.get("rev");

        if package.nix_hash == new_hash && old_rev == new_rev && !self.config.force {
            package.result.up_to_date();
            return Ok(());
        }

        // Update rev and hash
        ast.update_git(old_rev.as_deref(), &new_rev.clone().unwrap_or_default(), &new_hash, Some(&package.nix_hash))?;

        // Clear cargo/vendor hashes if they exist
        if let Some(old_vendor) = ast.get("vendorHash") {
            ast.set("vendorHash", &old_vendor, "")?;
        }

        // Check if we need to update cargo/vendor hash before writing
        if ast.get("cargoHash").is_some() {
            ast.update_vendor(package, "cargo", pb)?;
        }

        package.write(&ast)?;
        package.result.git_commit(old_rev.as_deref(), new_rev.as_deref());

        Ok(())
    }
}
