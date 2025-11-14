use anyhow::Result;
use indicatif::ProgressBar;

use crate::Config;
use crate::clients::GitHubClient;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::Updater;

pub struct GoUpdater {
    pub config: Config,
    pub github_client: GitHubClient,
}

impl Updater for GoUpdater {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
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

        let mut ast = package.ast();

        // Update rev and hash (version is updated automatically if it contains the old rev)
        ast.update_git(current_git_commit.as_deref(), &latest_commit, &new_hash, None)?;

        // Clear vendorHash to force recalculation
        if let Some(old_vendor_hash) = ast.get("vendorHash") {
            ast.set("vendorHash", &old_vendor_hash, "")?;
        }

        ast.update_vendor(package, "vendor", pb)?;

        package.write(&ast)?;

        package.result.git_commit(current_git_commit.as_deref(), Some(&latest_commit));

        Ok(())
    }
}
