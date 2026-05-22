use indicatif::ProgressBar;
use rootcause::Result;

use crate::Config;
use crate::clients::GitHubClient;
use crate::clients::nix::Nix;
use crate::package::Package;
use crate::updater::{Updater, normalize_version};

pub struct GoUpdater {
    force: bool,
    github_client: GitHubClient,
}

fn go_package_is_current(force: bool, current_rev: Option<&str>, latest_rev: Option<&str>, current_version: &str, latest_version: Option<&str>) -> bool {
    !force && current_rev == latest_rev && latest_version.is_none_or(|version| current_version == version)
}

impl Updater for GoUpdater {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            force: config.force,
            github_client: GitHubClient::new()?,
        })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        let ast_tmp = package.ast();

        let current_git_commit = ast_tmp.get("rev");
        let latest_git_commit = self.github_client.latest_commit(&package.homepage)?;
        let latest_version = self.github_client.latest_release(&package.homepage)?.map(|tag| normalize_version(&package.name, &tag));

        if go_package_is_current(
            self.force,
            current_git_commit.as_deref(),
            latest_git_commit.as_deref(),
            &package.version,
            latest_version.as_deref(),
        ) {
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

        if let Some(version) = &latest_version
            && package.version != *version
        {
            ast.set("version", &package.version, version)?;
        }

        if current_git_commit.as_deref() != Some(latest_commit.as_str()) {
            ast.clear_vendor_hash("vendor")?;
            ast.update_vendor(package, "vendor", pb)?;
        }

        package.write(&ast)?;

        if current_git_commit.as_deref() != Some(latest_commit.as_str()) {
            package.result.git_commit(current_git_commit.as_deref(), Some(&latest_commit));
        }

        if let Some(version) = &latest_version
            && package.version != *version
        {
            package.result.version(Some(package.version.as_ref()), Some(version.as_ref()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::go_package_is_current;

    #[test]
    fn package_is_not_current_when_release_version_is_newer_than_package_version() {
        assert!(!go_package_is_current(false, Some("abc"), Some("abc"), "0.24.1", Some("0.24.3")));
    }
}
