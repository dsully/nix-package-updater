use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

use indicatif::ProgressBar;
use whoami::username;

use crate::package::{Package, UpdateStatus};

pub fn build_package(package: &mut Package, pb: Option<&ProgressBar>, build_path: &Path, cache: bool) -> Result<()> {
    fs::create_dir_all(&build_path)?;

    let log_file = build_path.join(format!("{}.log", package.name));

    let output = Command::new("nix").args(["build", &format!(".#{}", package.name), "--no-link"]).output()?;

    let log_content = format!("stdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));

    fs::write(&log_file, log_content)?;

    if output.status.success() {
        package.result.status.insert(UpdateStatus::Built);

        if cache {
            push_to_cachix(package, pb)?;
        }
    }

    Ok(())
}

pub fn push_to_cachix(package: &mut Package, pb: Option<&ProgressBar>) -> Result<()> {
    //
    if let Some(pb) = pb {
        pb.set_message(format!("Pushing {} to cachix...", package.name()));
    } else {
        println!("{}", format!("Pushing {} to cachix...", package.name()).cyan());
    }

    let output = Command::new("nix").args(["path-info", &format!(".#{}", package.name)]).output()?;

    if output.status.success() {
        let paths = String::from_utf8_lossy(&output.stdout);

        for path in paths.lines() {
            if !path.is_empty() {
                Command::new("cachix")
                    .args(["push", "--compression-method", "xz", "--compression-level", "6", &username(), path])
                    .output()?;

                package.result.status.insert(UpdateStatus::Cached);
            }
        }
    }

    Ok(())
}
