use std::process::Command;

use anyhow::Result;
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
            Ok(Some(serde_json::from_slice::<NixPrefetchResult>(&output.stdout)?.hash))
        } else {
            Ok(None)
        }
    }

    pub fn hash_and_rev(url: &str, rev: Option<&str>) -> Result<Option<(String, Option<String>)>> {
        let mut cmd = Command::new("nurl");
        cmd.arg("--json").arg(url);

        if let Some(r) = rev {
            cmd.arg(r);
        }

        let output = cmd.output()?;

        if !output.status.success() {
            return Ok(None);
        }

        match String::from_utf8_lossy(&output.stdout).trim_end().lines().last() {
            Some(last_line) if !last_line.is_empty() => {
                let result: NurlResult = serde_json::from_str(last_line)?;
                Ok(Some((result.args.hash, result.args.rev)))
            }
            _ => Ok(None),
        }
    }
}
