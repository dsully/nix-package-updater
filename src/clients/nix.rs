use std::path::Path;
use std::process::Command;

use rootcause::{Result, report};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct NixPrefetchResult {
    pub hash: String,
}

#[derive(Debug, Deserialize)]
struct NurlResult {
    pub args: NurlArgs,
}

#[derive(Debug, Deserialize)]
struct NurlArgs {
    pub hash: String,
    pub rev: Option<String>,
}

#[derive(Debug, Default)]
pub struct Nix;

impl Nix {
    pub fn prefetch_hash(url: &str) -> Result<Option<String>> {
        let output = Command::new("nix").args(["store", "prefetch-file", url, "--json"]).output()?;

        if output.status.success() {
            return Ok(Some(serde_json::from_slice::<NixPrefetchResult>(&output.stdout)?.hash));
        }

        Ok(None)
    }

    /// Add a file to the store the way `requireFile` expects it and return its flat SRI hash.
    pub fn add_fixed(path: &Path) -> Result<String> {
        let added = Command::new("nix-store").args(["--add-fixed", "sha256"]).arg(path).output()?;

        if !added.status.success() {
            return Err(report!("nix-store --add-fixed failed for {}: {}", path.display(), String::from_utf8_lossy(&added.stderr)));
        }

        let hashed = Command::new("nix").args(["hash", "file", "--sri"]).arg(path).output()?;

        if !hashed.status.success() {
            return Err(report!("nix hash file failed for {}: {}", path.display(), String::from_utf8_lossy(&hashed.stderr)));
        }

        Ok(String::from_utf8_lossy(&hashed.stdout).trim().to_string())
    }

    pub fn hash_and_rev(url: &str, rev: Option<&str>) -> Result<Option<(String, Option<String>)>> {
        let output = Command::new("nurl").arg("--json").arg(url).args(rev.as_ref()).output()?;

        if output.status.success() {
            return match String::from_utf8_lossy(&output.stdout).trim_end().lines().last() {
                Some(last_line) if !last_line.is_empty() => {
                    let result: NurlResult = serde_json::from_str(last_line)?;
                    Ok(Some((result.args.hash, result.args.rev)))
                }
                _ => Ok(None),
            };
        }

        Ok(None)
    }

    pub fn prefetch_fetchcrate(pname: &str, version: &str) -> Result<Option<String>> {
        let crate_url = format!("https://crates.io/crates/{pname}");
        let output = Command::new("nurl").args(["--json", "--fetcher", "fetchCrate", &crate_url, version]).output()?;

        if output.status.success() {
            return match String::from_utf8_lossy(&output.stdout).trim_end().lines().last() {
                Some(last_line) if !last_line.is_empty() => {
                    let result: NurlResult = serde_json::from_str(last_line)?;
                    Ok(Some(result.args.hash))
                }
                _ => Ok(None),
            };
        }

        Ok(None)
    }
}
