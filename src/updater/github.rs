use indicatif::ProgressBar;
use rootcause::Result;

use crate::Config;
use crate::clients::GitHubClient;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::{Updater, normalize_version};

pub struct GitHubRelease {
    force: bool,
    client: GitHubClient,
}

fn release_asset_filename(package_name: &str, platform_name: &str, attributes: &std::collections::HashMap<String, String>) -> Option<String> {
    attributes.get("filename").cloned().or_else(|| {
        attributes.get("suffix").map(|suffix| {
            let target = if platform_name.split_once('-').is_some_and(|(arch, _)| suffix.starts_with(arch)) {
                suffix.clone()
            } else if let Some((arch, _)) = platform_name.split_once('-') {
                format!("{arch}-{suffix}")
            } else {
                suffix.clone()
            };

            format!("{package_name}-{target}.tar.gz")
        })
    })
}

fn asset_url(template: &str, package_name: &str, version: &str, attributes: &std::collections::HashMap<String, String>) -> String {
    attributes.iter().fold(
        template.replace("${pname}", package_name).replace("${version}", version),
        |url, (key, value)| url.replace(&format!("${{source.{key}}}"), value),
    )
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

        let latest_version = normalize_version(&package.name, &latest_tag);

        if self.should_skip_update(self.force, &package.version, &latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        let mut ast = package.ast();

        ast.set("version", &package.version, &latest_version)?;

        let platform_blocks = ast.platforms();

        if platform_blocks.is_empty()
            && let Some((new_hash, _)) = Nix::hash_and_rev(&format!("{}/archive/refs/tags/{latest_tag}.tar.gz", package.homepage), None)
                .ok()
                .flatten()
        {
            ast.set("hash", &package.nix_hash, &new_hash)?;
        }

        let url_template = ast.get("url").filter(|url| url.contains("${source."));
        let repo_path = package.homepage.path();

        for block in platform_blocks {
            let Some(old_hash) = block.attributes.get("hash") else {
                continue;
            };

            let url = if let Some(template) = &url_template {
                asset_url(template, &package.name, &latest_version, &block.attributes)
            } else if let Some(filename) = release_asset_filename(&package.name, &block.platform_name, &block.attributes) {
                format!("https://github.com/{repo_path}/releases/download/{latest_tag}/{filename}")
            } else {
                continue;
            };

            if let Some(new_hash) = Nix::prefetch_hash(&url)? {
                ast.set("hash", old_hash, &new_hash)?;
            } else {
                package.result.failed(format!("Failed to get hash for {url}"));
                return Ok(());
            }
        }

        package.write(&ast)?;
        package.result.version(Some(package.version.as_ref()), Some(latest_version.as_ref()));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{asset_url, release_asset_filename};

    #[test]
    fn asset_url_fills_template_from_platform_attributes() {
        let attributes = HashMap::from([("suffix".to_string(), "darwin-arm64".to_string()), ("hash".to_string(), "sha256-x".to_string())]);

        assert_eq!(
            asset_url(
                "https://github.com/opensec-git/Pi-Bolt/releases/download/bolt-v${version}/${pname}-${source.suffix}.tar.xz",
                "pi-bolt",
                "0.8.5",
                &attributes
            ),
            "https://github.com/opensec-git/Pi-Bolt/releases/download/bolt-v0.8.5/pi-bolt-darwin-arm64.tar.xz"
        );
    }

    #[test]
    fn release_asset_filename_uses_explicit_filename() {
        let attributes = HashMap::from([("filename".to_string(), "tool-linux.tar.gz".to_string())]);

        assert_eq!(release_asset_filename("tool", "x86_64-linux", &attributes).as_deref(), Some("tool-linux.tar.gz"));
    }

    #[test]
    fn release_asset_filename_builds_tarball_name_from_suffix() {
        let attributes = HashMap::from([("suffix".to_string(), "unknown-linux-gnu".to_string())]);

        assert_eq!(
            release_asset_filename("icm", "x86_64-linux", &attributes).as_deref(),
            Some("icm-x86_64-unknown-linux-gnu.tar.gz")
        );
    }
}
