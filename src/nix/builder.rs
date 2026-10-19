use std::fs;
use std::path::Path;
use std::process::Command;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget};
use rootcause::Result;
use whoami::username;

use crate::nix::cachix;
use crate::package::{Package, UpdateStatus};

pub fn build_package(package: &mut Package, pb: &ProgressBar, build_path: &Path, cache: bool) -> Result<()> {
    fs::create_dir_all(build_path)?;

    let log_file = build_path.join(format!("{}.log", package.name));

    pb.set_message(format!("{}: Building ...", package.name()));

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

pub fn push_to_cachix(package: &mut Package, pb: &ProgressBar) -> Result<()> {
    pb.set_message(format!("{}: Pushing to cachix ...", package.name()));

    // The package spinner already reports this step; per-path bars would fight main's MultiProgress for stderr.
    let hidden = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());

    cachix::push(&username()?, &[format!(".#{}", package.name)], &hidden)?;

    package.result.status.insert(UpdateStatus::Cached);

    Ok(())
}
