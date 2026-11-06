use std::fs;
use std::path::Path;

use indicatif::ProgressBar;
use rootcause::{Result, report};

use crate::Config;
use crate::clients::nix::Nix;
use crate::clients::{GitHubClient, NpmClient};
use crate::package::Package;
use crate::updater::{Updater, short_hash};

pub struct NpmUpdater {
    force: bool,
    npm_client: NpmClient,
    github_client: &'static GitHubClient,
}

/// Without a `rev` attribute, the commit is tracked as the `<base>-<short hash>` version suffix this updater writes.
fn npm_package_is_current(force: bool, current_rev: Option<&str>, latest_rev: Option<&str>, version: &str) -> bool {
    let Some(latest) = latest_rev.filter(|_| !force) else {
        return false;
    };

    match current_rev {
        Some(current) => current == latest,
        None => version.rsplit_once('-').is_some_and(|(_, suffix)| suffix == short_hash(latest)),
    }
}

impl Updater for NpmUpdater {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            force: config.force,
            npm_client: NpmClient::new()?,
            github_client: GitHubClient::shared()?,
        })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        let ast_tmp = package.ast();

        let current_git_commit = ast_tmp.get("rev");
        let latest_git_commit = self.github_client.latest_commit(&package.homepage)?;

        if npm_package_is_current(self.force, current_git_commit.as_deref(), latest_git_commit.as_deref(), &package.version) {
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

        // buildNpmPackage reads package-lock.json from src by default. Only fetch and vendor a
        // standalone lockfile when the package definition references ./package-lock.json (upstreams
        // that don't ship one). Otherwise bumping the source is sufficient.
        if references_package_lock(ast_tmp.content()) {
            if let Some(pb) = pb {
                pb.set_message(format!("{}: Downloading package-lock.json...", package.name()));
            }

            // Use the specific commit hash to get the exact package-lock.json
            let package_lock_url = format!("https://raw.githubusercontent.com/{}/{}/package-lock.json", package.homepage.path(), latest_commit);

            let Some(package_lock_content) = self.npm_client.download_package_lock(&package_lock_url)? else {
                package.result.failed("Could not download package-lock.json from repository");
                return Ok(());
            };

            if !package.dry_run {
                save_package_lock(&package.path, &package_lock_content)?;
            }
        }

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

        ast.clear_vendor_hash("npmDeps")?;
        ast.update_vendor(package, "npmDeps", pb)?;

        package.write(&ast)?;

        package.result.git_commit(current_git_commit.as_deref(), Some(&latest_commit)).version(
            Some(&package.version),
            Some(&format!("{}-{latest_version}", package.version.split('-').next().unwrap_or(&package.version))),
        );

        Ok(())
    }
}

/// Whether the package definition vendors a standalone `./package-lock.json`.
fn references_package_lock(content: &str) -> bool {
    content.contains("package-lock.json")
}

/// Save package-lock.json next to the Nix file
fn save_package_lock(nix_path: &Path, content: &str) -> Result<()> {
    let package_lock_path = nix_path
        .parent()
        .ok_or_else(|| report!("Could not get parent directory of Nix file"))?
        .join("package-lock.json");

    fs::write(&package_lock_path, content)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::npm_package_is_current;

    const LATEST: &str = "cc9a0821351d60162ab09c5eb5c8a87321f7d718";

    #[test]
    fn current_when_version_suffix_matches_latest_commit_without_rev() {
        assert!(npm_package_is_current(false, None, Some(LATEST), "2.23.0-cc9a0821"));
    }

    #[test]
    fn not_current_when_version_suffix_differs_without_rev() {
        assert!(!npm_package_is_current(false, None, Some(LATEST), "2.23.0-deadbeef"));
    }

    #[test]
    fn not_current_when_version_has_no_hash_suffix() {
        assert!(!npm_package_is_current(false, None, Some(LATEST), "2.23.0"));
    }

    #[test]
    fn rev_comparison_wins_when_rev_present() {
        assert!(npm_package_is_current(false, Some(LATEST), Some(LATEST), "2.23.0-deadbeef"));
        assert!(!npm_package_is_current(false, Some("abc"), Some(LATEST), "2.23.0-cc9a0821"));
    }

    #[test]
    fn never_current_when_forced_or_latest_unknown() {
        assert!(!npm_package_is_current(true, Some(LATEST), Some(LATEST), "2.23.0-cc9a0821"));
        assert!(!npm_package_is_current(false, None, None, "2.23.0-cc9a0821"));
    }
}
