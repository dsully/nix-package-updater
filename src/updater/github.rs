use indicatif::ProgressBar;
use rootcause::Result;

use crate::Config;
use crate::clients::GitHubClient;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::Updater;

pub struct GitHubRelease {
    force: bool,
    client: GitHubClient,
}

impl Updater for GitHubRelease {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            force: config.force,
            client: GitHubClient::new()?,
        })
    }

    fn update(&self, package: &mut Package, _pb: Option<&ProgressBar>) -> Result<()> {
        let Some(latest_tag) = self.client.latest_release(&package.homepage)? else {
            package.result.message("No releases found on GitHub - keeping current version");
            return Ok(());
        };

        let latest_version = latest_tag.trim_start_matches('v').to_string();

        if self.should_skip_update(self.force, &package.version, &latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        let mut ast = package.ast();

        ast.set("version", &package.version, &latest_version)?;

        let new_hash = Nix::hash_and_rev(&format!("{}/archive/refs/tags/{latest_tag}.tar.gz", package.homepage), None)
            .ok()
            .flatten()
            .map(|(new_hash, _)| new_hash);

        if let Some(new_h) = &new_hash {
            ast.set("hash", &package.nix_hash, new_h)?;
        }

        let platform_blocks = ast.platforms();
        let repo_path = package.homepage.path();

        for block in platform_blocks {
            if let Some(filename) = block.attributes.get("filename")
                && let Some(old_hash) = block.attributes.get("hash")
            {
                let url = format!("https://github.com/{repo_path}/releases/download/{latest_tag}/{filename}");

                // Get new hash
                if let Some(new_hash) = Nix::prefetch_hash(&url)? {
                    ast.set("hash", old_hash, &new_hash)?;
                } else {
                    package.result.failed(format!("Failed to get hash for {filename}"));
                    break;
                }
            }
        }

        package.write(&ast)?;
        package.result.version(Some(package.version.as_ref()), Some(latest_version.as_ref()));

        Ok(())
    }
}
