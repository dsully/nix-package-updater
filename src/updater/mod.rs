pub mod cargo;
pub mod git;
pub mod github;
pub mod pypi;

use anyhow::Result;
use indicatif::ProgressBar;

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
pub fn short_hash(hash: &str) -> String {
    hash.strip_prefix("sha256-").unwrap_or(hash).chars().take(8).collect()
}
