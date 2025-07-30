use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::Config;
use crate::package::Package;

pub fn build_package(package: &Package, build_results_dir: &Path, cache: bool, config: &Config) -> Result<bool> {
    fs::create_dir_all(build_results_dir)?;

    let log_file = build_results_dir.join(format!("{}.log", package.name));

    let output = Command::new("nix").args(["build", &format!(".#{}", package.name), "--no-link"]).output()?;

    let log_content = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::write(&log_file, log_content)?;

    if output.status.success() {
        if cache {
            push_to_cachix(package, config)?;
        }

        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn push_to_cachix(package: &Package, config: &Config) -> Result<()> {
    println!("{}", format!("Pushing {} to cachix...", package.name).cyan());

    let output = Command::new("nix").args(["path-info", &format!(".#{}", package.name)]).output()?;

    if output.status.success() {
        let paths = String::from_utf8_lossy(&output.stdout);

        for path in paths.lines() {
            if !path.is_empty() {
                let _ = Command::new("cachix")
                    .args(["push", "--compression-method", "xz", "--compression-level", "6", &config.cachix_name, path])
                    .output();
            }
        }
    }

    Ok(())
}

/// Extract capture group from error output
pub fn extract_hash_from_error(stderr: &str) -> Option<String> {
    if let Some(pos) = stderr.find("got:") {
        let after_got = &stderr[pos + 4..].trim();

        if let Some(end) = after_got.find(|c: char| c.is_whitespace()) {
            return Some(after_got[..end].to_string());
        }

        return Some((*after_got).to_string());
    }

    None
}
