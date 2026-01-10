use indicatif::ProgressBar;
use rootcause::Result;

use crate::Config;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::Updater;

pub struct GitRepository {
    force: bool,
}

impl Updater for GitRepository {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self { force: config.force })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        let Some((new_hash, new_rev)) = Nix::hash_and_rev(&package.homepage.to_string(), None)? else {
            package.result.failed("nurl failed");
            return Ok(());
        };

        let mut ast = package.ast();
        let old_rev = ast.get("rev");

        if package.nix_hash == new_hash && old_rev == new_rev && !self.force {
            package.result.up_to_date();
            return Ok(());
        }

        // Update rev and hash
        ast.update_git(old_rev.as_deref(), &new_rev.clone().unwrap_or_default(), &new_hash, Some(&package.nix_hash))?;

        ast.clear_vendor_hash("vendor")?;

        if ast.get("cargoHash").is_some() {
            ast.update_vendor(package, "cargo", pb)?;
        }

        package.write(&ast)?;
        package.result.git_commit(old_rev.as_deref(), new_rev.as_deref());

        Ok(())
    }
}
