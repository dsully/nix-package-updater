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

pub fn normalize_version(package_name: &str, version: &str) -> String {
    let package_version_prefix = format!("{package_name}-v");
    let package_prefix = format!("{package_name}-");
    let version = version.rsplit('/').next().unwrap_or(version);

    version
        .strip_prefix(&package_version_prefix)
        .or_else(|| version.strip_prefix(&package_prefix))
        .or_else(|| version.strip_prefix('v'))
        .unwrap_or(version)
        .to_string()
}

/// Compare two semantic versions, returns true if a > b
pub fn version_is_greater(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va > vb,
        _ => a > b, // Fall back to string comparison if parsing fails
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_version;

    #[test]
    fn normalizes_package_prefixed_version() {
        assert_eq!(normalize_version("example", "example-v1.2.3"), "1.2.3");
    }

    #[test]
    fn normalizes_package_prefixed_version_without_v() {
        assert_eq!(normalize_version("example", "example-1.2.3"), "1.2.3");
    }

    #[test]
    fn normalizes_leading_version_prefix() {
        assert_eq!(normalize_version("example", "v1.2.3"), "1.2.3");
    }

    #[test]
    fn normalizes_path_prefixed_version() {
        assert_eq!(normalize_version("mcp-mux", "muxcore/v0.24.3"), "0.24.3");
    }

    #[test]
    fn keeps_unprefixed_version() {
        assert_eq!(normalize_version("example", "1.2.3"), "1.2.3");
    }
}
