use anyhow::Result;
use indicatif::ProgressBar;

use crate::Config;
use crate::clients::GitHubClient;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::Updater;

pub struct GitHubRelease {
    pub config: Config,
    pub client: GitHubClient,
}

impl Updater for GitHubRelease {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            client: GitHubClient::new()?,
        })
    }

    fn update(&self, package: &mut Package, _pb: Option<&ProgressBar>) -> Result<()> {
        //
        let Some(latest_tag) = self.client.latest_release(&package.homepage)? else {
            package.result.message("No releases found on GitHub - keeping current version");
            return Ok(());
        };

        let latest_version = latest_tag.trim_start_matches('v').to_string();

        if self.should_skip_update(self.config.force, &package.version, &latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        let mut ast = package.ast();

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

        // ast.update_github_hashes(&release_data)?;

        // Check for platformData structures
        let platform_blocks = ast.platforms();

        // Handle structured platform data
        for block in platform_blocks {
            if let Some(filename) = block.attributes.get("filename")
                && let Some(old_hash) = block.attributes.get("hash")
            {
                let url = format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    release_data["repo"].as_str().unwrap(),
                    release_data["tag"].as_str().unwrap(),
                    filename
                );

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
