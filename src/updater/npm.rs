use std::fs;
use std::path::Path;

use anyhow::Result;
use indicatif::ProgressBar;

use crate::Config;
use crate::clients::nix::Nix;
use crate::clients::{GitHubClient, NpmClient};
use crate::package::Package;
use crate::updater::{Updater, short_hash};

pub struct NpmUpdater {
    pub config: Config,
    pub npm_client: NpmClient,
    pub github_client: GitHubClient,
}

impl Updater for NpmUpdater {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            npm_client: NpmClient::new(),
            github_client: GitHubClient::new()?,
        })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        let ast_tmp = package.ast();

        // Get current git commit (rev) if it exists
        let current_git_commit = ast_tmp.get("rev");

        // Try to get latest commit from GitHub
        let latest_git_commit = self.github_client.latest_commit(&package.homepage)?;

        if let (Some(current), Some(latest)) = (&current_git_commit, &latest_git_commit)
            && self.should_skip_update(self.config.force, current, latest)
        {
            package.result.up_to_date();
            return Ok(());
        }

        // If we have a new commit, proceed with update
        let Some(latest_commit) = latest_git_commit else {
            package.result.failed("Could not get latest commit from GitHub");
            return Ok(());
        };

        // Get new hash using nurl
        let Some((new_hash, _)) = Nix::hash_and_rev(&package.homepage.to_string(), Some(&latest_commit))? else {
            package.result.failed("Failed to get new hash");
            return Ok(());
        };

        // Download package-lock.json from GitHub at the specific commit
        if let Some(pb) = pb {
            pb.set_message(format!("{}: Downloading package-lock.json...", package.name()));
        }

        // Use the specific commit hash to get the exact package-lock.json
        let package_lock_url = format!("https://raw.githubusercontent.com/{}/{}/package-lock.json", package.homepage.path(), latest_commit);

        let Some(package_lock_content) = self.npm_client.download_package_lock(&package_lock_url)? else {
            package.result.failed("Could not download package-lock.json from repository");
            return Ok(());
        };

        save_package_lock(&package.path, &package_lock_content)?;

        let mut ast = package.ast();

        // Update rev and hash
        ast.update_git(current_git_commit.as_deref(), &latest_commit, &new_hash, None)?;

        // Update version to include the commit hash
        let latest_version = short_hash(&latest_commit);

        // Check if version follows pattern "x.y.z-${rev}" and update accordingly
        if let Some(base_version) = package.version.split('-').next() {
            let new_version = format!("{base_version}-{latest_version}");
            ast.set("version", &package.version, &new_version)?;
        }

        // Clear npmDepsHash to force recalculation
        if let Some(old_npm_hash) = ast.get("npmDepsHash") {
            ast.set("npmDepsHash", &old_npm_hash, "")?;
        }

        // Update npmDepsHash using the vendor hash update mechanism
        ast.update_vendor(package, "npmDeps", pb)?;

        package.write(&ast)?;

        package.result.git_commit(current_git_commit.as_deref(), Some(&latest_commit)).version(
            Some(&package.version),
            Some(&format!("{}-{latest_version}", package.version.split('-').next().unwrap_or(&package.version))),
        );

        Ok(())
    }
}

/// Save package-lock.json next to the Nix file
fn save_package_lock(nix_path: &Path, content: &str) -> Result<()> {
    let package_lock_path = nix_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Could not get parent directory of Nix file"))?
        .join("package-lock.json");

    fs::write(&package_lock_path, content)?;

    Ok(())
}
