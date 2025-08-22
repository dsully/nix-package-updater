use anyhow::Result;
use indicatif::ProgressBar;

use crate::Config;
use crate::clients::PyPiClient;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::Updater;

pub struct PyPiUpdater {
    pub config: Config,
    pub client: PyPiClient,
}

impl Updater for PyPiUpdater {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            client: PyPiClient::new(),
        })
    }

    fn update(&self, package: &mut Package, _pb: Option<&ProgressBar>) -> Result<()> {
        //
        // Get latest version from PyPI using the client
        let Some(data) = self.client.project(&package.name)? else {
            package.result.failed(format!("{}: Package not found on PyPI", package.name()));
            return Ok(());
        };

        let latest_version = data.info.version;

        if self.should_skip_update(self.config.force, &package.version, &latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        let mut ast = package.ast();

        // Update platform hashes
        if let Some(releases) = data.releases.get(&latest_version) {
            //
            let platform_blocks = ast.platforms();

            for block in platform_blocks {
                let (Some(platform_value), Some(old_hash)) = (block.attributes.get("platform"), block.attributes.get("hash")) else {
                    continue;
                };

                // Find matching wheel by platform
                let Some(url) = releases.iter().find(|w| w.filename.contains(platform_value)).map(|w| &w.url) else {
                    package.result.failed(format!("No wheel found for platform {platform_value}"));
                    return Ok(());
                };

                if let Some(new_hash) = Nix::prefetch_hash(url)? {
                    ast.set("hash", old_hash, &new_hash)?;
                } else {
                    package.result.failed(format!("Failed to get hash for platform {}", block.platform_name));
                    break;
                }
            }
        }

        ast.set("version", &package.version, &latest_version)?;

        package.write(&ast)?;
        package.result.version(Some(package.version.as_ref()), Some(latest_version.as_ref()));

        Ok(())
    }
}
