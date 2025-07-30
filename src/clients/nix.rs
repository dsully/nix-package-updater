use anyhow::Result;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]

pub struct NixPrefetchResult {
    pub hash: String,
}

#[derive(Debug, Deserialize)]

pub struct NurlResult {
    pub args: NurlArgs,
}

#[derive(Debug, Deserialize)]

pub struct NurlArgs {
    pub hash: String,
    pub rev: Option<String>,
}

pub fn get_nix_hash(url: &str) -> Result<Option<String>> {
    let output = Command::new("nix").args(["store", "prefetch-file", url, "--json"]).output()?;

    if output.status.success() {
        let result: NixPrefetchResult = serde_json::from_slice(&output.stdout)?;

        Ok(Some(result.hash))
    } else {
        Ok(None)
    }
}

pub fn get_nurl_data(url: &str, rev: Option<&str>) -> Result<Option<NurlResult>> {
    let mut cmd = Command::new("nurl");

    cmd.args(["--json", url]);

    if let Some(r) = rev {
        cmd.arg(r);
    }

    let output = cmd.output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);

        // nurl outputs the JSON on the last line
        if let Some(last_line) = stdout.lines().last() {
            Ok(Some(serde_json::from_str(last_line)?))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}
