use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

use indicatif::ProgressBar;

use crate::Config;
use crate::package::Package;

pub fn build_package(package: &Package, pb: Option<&ProgressBar>, build_path: &Path, cache: bool, config: &Config) -> Result<bool> {
    let log_file = build_path.join(format!("{}.log", package.name));

    let output = Command::new("nix").args(["build", &format!(".#{}", package.name), "--no-link"]).output()?;

    let log_content = format!("stdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));

    fs::write(&log_file, log_content)?;

    if output.status.success() {
        if cache && let Some(ref cachix_name) = config.cachix_name {
            push_to_cachix(package, pb, cachix_name)?;
        } else {
            anyhow::bail!("A cachix name must be set to use the cache!")
        }

        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn push_to_cachix(package: &Package, pb: Option<&ProgressBar>, cachix: &str) -> Result<()> {
    //
    if let Some(pb) = pb {
        pb.set_message(format!("Pushing {} to cachix...", package.display_name()));
    } else {
        println!("{}", format!("Pushing {} to cachix...", package.display_name()).cyan());
    }

    let output = Command::new("nix").args(["path-info", &format!(".#{}", package.name)]).output()?;

    if output.status.success() {
        let paths = String::from_utf8_lossy(&output.stdout);

        for path in paths.lines() {
            if !path.is_empty() {
                let _ = Command::new("cachix")
                    .args(["push", "--compression-method", "xz", "--compression-level", "6", cachix, path])
                    .output();
            }
        }
    }

    Ok(())
}
