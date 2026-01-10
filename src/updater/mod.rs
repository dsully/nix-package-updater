pub mod cargo;
pub mod git;
pub mod github;
pub mod go;
pub mod npm;
pub mod pypi;

use indicatif::ProgressBar;
use rootcause::Result;

use crate::Config;
use crate::package::Package;

pub trait Updater: Sized {
    fn new(config: &Config) -> Result<Self>;
    fn update(&self, package: &mut Package, pb: Option<&ProgressBar>) -> Result<()>;

    fn should_skip_update(&self, force: bool, current: &str, latest: &str) -> bool {
        current == latest && !force
    }
}

/// Create a short git hash (first 8 characters) from a full hash or revision
pub fn short_hash(hash: impl AsRef<str>) -> String {
    let hash = hash.as_ref();

    hash.strip_prefix("sha256-").unwrap_or(hash).chars().take(8).collect()
}

/// Compare two semantic versions, returns true if a > b
pub fn version_is_greater(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va > vb,
        _ => a > b, // Fall back to string comparison if parsing fails
    }
}
