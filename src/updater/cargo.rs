use indicatif::ProgressBar;
use rootcause::Result;

use crate::Config;
use crate::clients::nix::Nix;
use crate::clients::{CratesIoClient, GitHubClient};
use crate::nix::ast::Ast;
use crate::package::Package;
use crate::updater::{Updater, normalize_version, short_hash, version_is_greater};

pub struct Cargo {
    force: bool,
    github_client: &'static GitHubClient,
    crates_client: CratesIoClient,
}

fn cargo_vendor_needs_update(current_rev: Option<&str>, latest_rev: Option<&str>, current_version: &str, latest_version: &str) -> bool {
    current_rev != latest_rev || current_version != latest_version
}

/// Whether the latest release or Cargo.toml version is ahead of the packaged version.
fn version_is_newer(current: &str, release: Option<&str>, cargo: Option<&str>) -> bool {
    // Only semver: the string fallback in `version_is_greater` would flag tags like "nightly" on every run.
    let Ok(current) = semver::Version::parse(current) else {
        return false;
    };

    release
        .into_iter()
        .chain(cargo)
        .filter_map(|candidate| semver::Version::parse(candidate).ok())
        .any(|candidate| candidate > current)
}

impl Updater for Cargo {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            force: config.force,
            github_client: GitHubClient::shared()?,
            crates_client: CratesIoClient::new()?,
        })
    }

    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        // Detect if this is a fetchCrate package or git-based package
        let root = package.ast.syntax();

        if Ast::contains_function_call(&root, "fetchCrate") {
            self.update_fetch_crate(package, pb)
        } else {
            self.update_git_based(package, pb)
        }
    }
}

impl Cargo {
    /// Update packages that use fetchCrate (from crates.io)
    fn update_fetch_crate(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        //
        // Query crates.io for latest version
        let Some(crate_info) = self.crates_client.crate_info(&package.name)? else {
            package.result.failed("Crate not found on crates.io");
            return Ok(());
        };

        let latest_version = &crate_info.crate_data.max_version;

        // Skip if already up to date
        if self.should_skip_update(self.force, &package.version, latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        // Get new hash for the crate using nurl with fetchCrate fetcher
        let Some(new_hash) = Nix::prefetch_fetchcrate(&package.name, latest_version)? else {
            package.result.failed("Failed to get hash for crate");
            return Ok(());
        };

        let mut ast = package.ast();

        if package.version != *latest_version {
            ast.set("version", &package.version, latest_version)?;
        }

        if let Some(old_hash) = ast.get("hash") {
            ast.set("hash", &old_hash, &new_hash)?;
        }

        if cargo_vendor_needs_update(None, None, &package.version, latest_version) {
            ast.clear_vendor_hash("cargo")?;
            ast.update_vendor(package, "cargo", pb)?;
        }

        package.write(&ast)?;

        package.result.version(Some(package.version.as_ref()), Some(latest_version));

        Ok(())
    }

    /// Update packages that use git sources (fetchFromGitHub, etc.)
    fn update_git_based(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
        // Get current hash before any updates
        //
        let ast_tmp = package.ast();

        let Some(current_git_commit) = ast_tmp.get("rev") else {
            package.result.failed("Could not extract rev");
            return Ok(());
        };

        // Release and Cargo.toml versions are needed even when the rev is current: upstream may bump the
        // version without a new commit relative to our rev.
        let (latest_git_commit, release_tag, current_cargo_version) = std::thread::scope(|s| {
            let latest_git_commit = s.spawn(|| self.github_client.latest_commit(&package.homepage));
            let release_tag = s.spawn(|| self.github_client.latest_release(&package.homepage));
            let current_cargo_version = self.github_client.cargo_version(&package.homepage, &current_git_commit);

            (
                latest_git_commit.join().expect("latest_commit thread panicked"),
                release_tag.join().expect("latest_release thread panicked"),
                current_cargo_version,
            )
        });

        let Some(latest_git_commit) = latest_git_commit? else {
            package.result.failed("Failed to fetch latest commit");
            return Ok(());
        };

        let release_version = release_tag.ok().flatten().map(|tag| normalize_version(&package.name, &tag));

        if current_git_commit == latest_git_commit
            && !self.force
            && !version_is_newer(&package.version, release_version.as_deref(), current_cargo_version.as_ref().ok().and_then(Option::as_deref))
        {
            package.result.up_to_date();
            return Ok(());
        }

        let (hash_and_rev, cargo_version) = std::thread::scope(|s| {
            let hash_and_rev = s.spawn(|| Nix::hash_and_rev(&package.homepage.to_string(), Some(&latest_git_commit)));

            let cargo_version = if latest_git_commit == current_git_commit {
                current_cargo_version
            } else {
                self.github_client.cargo_version(&package.homepage, &latest_git_commit)
            };

            (hash_and_rev.join().expect("nurl thread panicked"), cargo_version)
        });

        let Some((new_hash, _)) = hash_and_rev? else {
            package.result.failed("Failed to get new hash");
            return Ok(());
        };

        let mut ast = package.ast();

        // Update rev and hash
        ast.update_git(Some(&current_git_commit), &latest_git_commit, &new_hash, None)?;

        let cargo_version = cargo_version.ok().flatten();

        // Pick the higher version, or fall back to short hash for non-semantic packages
        let latest_version = match (&release_version, &cargo_version) {
            (Some(rel), Some(cargo)) => {
                // Compare versions - pick higher one
                if version_is_greater(cargo, rel) { cargo.clone() } else { rel.clone() }
            }
            (Some(rel), None) => rel.clone(),
            (None, Some(cargo)) => cargo.clone(),
            (None, None) => {
                // No version source found - only use short hash if current version is hash-like
                let is_semantic_version = package.version.contains('.') && package.version.chars().any(|c| c.is_ascii_digit());

                if is_semantic_version { package.version.clone() } else { short_hash(&latest_git_commit) }
            }
        };

        if package.version != latest_version {
            ast.set("version", &package.version, &latest_version)?;
        }

        if cargo_vendor_needs_update(Some(&current_git_commit), Some(&latest_git_commit), &package.version, &latest_version) {
            ast.clear_vendor_hash("cargo")?;
            ast.update_vendor(package, "cargo", pb)?;
        }

        package.write(&ast)?;

        package.result.git_commit(Some(current_git_commit.as_ref()), Some(latest_git_commit.as_ref()));

        if package.version != latest_version {
            package.result.version(Some(package.version.as_ref()), Some(latest_version.as_ref()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{cargo_vendor_needs_update, version_is_newer};

    #[test]
    fn newer_cargo_version_at_same_rev_is_an_update() {
        assert!(version_is_newer("0.6.0", Some("0.6.0"), Some("0.7.0")));
    }

    #[test]
    fn newer_release_is_an_update() {
        assert!(version_is_newer("0.6.0", Some("0.6.1"), None));
    }

    #[test]
    fn equal_or_older_versions_are_not_updates() {
        assert!(!version_is_newer("0.7.0", Some("0.6.0"), Some("0.7.0")));
        assert!(!version_is_newer("0.7.0", None, None));
    }

    #[test]
    fn non_semver_versions_are_never_updates() {
        assert!(!version_is_newer("0.7.0", Some("nightly"), None));
        assert!(!version_is_newer("unstable-2024-01-01", Some("0.7.0"), None));
    }

    #[test]
    fn cargo_vendor_does_not_update_when_rev_and_version_are_unchanged() {
        assert!(!cargo_vendor_needs_update(Some("abc"), Some("abc"), "1.0.0", "1.0.0"));
    }

    #[test]
    fn cargo_vendor_updates_when_rev_changes() {
        assert!(cargo_vendor_needs_update(Some("abc"), Some("def"), "1.0.0", "1.0.0"));
    }

    #[test]
    fn cargo_vendor_updates_when_version_changes() {
        assert!(cargo_vendor_needs_update(None, None, "1.0.0", "1.0.1"));
    }
}
