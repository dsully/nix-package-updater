use anyhow::Result;
use colored::Colorize;
use indicatif::ProgressBar;
use std::fs;
use std::process::Command;

use crate::nix::{extract_hash_from_error, find_attr_value, update_attr_value};
use crate::package::Package;

/// Create a short git hash (first 8 characters) from a full hash or revision
pub fn short_hash(hash: &str) -> String {
    hash.strip_prefix("sha256-").unwrap_or(hash).chars().take(8).collect()
}

pub fn update_rev_and_hash(content: &str, old_rev: Option<&str>, new_rev: &str, new_hash: &str, old_hash: Option<&str>) -> String {
    let mut new_content = content.to_string();

    if let Some(old_rev) = old_rev
        && !new_rev.is_empty()
    {
        new_content = update_attr_value(&new_content, "rev", old_rev, new_rev);

        // Update version if it contains rev
        let ast = rnix::Root::parse(&new_content);

        if let Some(current_version) = find_attr_value(&ast.syntax(), "version")
            && current_version.contains(old_rev)
        {
            let new_version = current_version.replace(old_rev, new_rev);

            new_content = update_attr_value(&new_content, "version", &current_version, &new_version);
        }
    }

    // Update hash
    let old_hash = if let Some(h) = old_hash {
        h.to_string()
    } else {
        let ast = rnix::Root::parse(content);

        find_attr_value(&ast.syntax(), "hash").unwrap_or_default()
    };

    if !old_hash.is_empty() && !new_hash.is_empty() {
        new_content = update_attr_value(&new_content, "hash", &old_hash, new_hash);
    }

    new_content
}

pub fn update_cargo_or_vendor_hash(package: &Package, hash_type: &str, pb: Option<&ProgressBar>) -> Result<()> {
    if let Some(pb) = pb {
        pb.set_message(format!("{}: Building to get new {hash_type}Hash...", package.display_name()));
    } else {
        println!("{}", format!("{}: Building to get new {hash_type}Hash...", package.display_name()).yellow());
    }

    let output = Command::new("nix").args(["build", &format!(".#{}", package.name), "--no-link"]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        if let Some(new_hash) = extract_hash_from_error(&stderr) {
            let content = fs::read_to_string(&package.file_path)?;

            let attr_name = format!("{hash_type}Hash");

            // Find the old hash value
            let ast = rnix::Root::parse(&content);

            if let Some(old_hash) = find_attr_value(&ast.syntax(), &attr_name) {
                let new_content = update_attr_value(&content, &attr_name, &old_hash, &new_hash);

                fs::write(&package.file_path, new_content)?;
            } else {
                // If no existing hash or empty hash, update it
                let old_pattern = format!(r#"{attr_name} = """"#);
                let new_pattern = format!(r#"{attr_name} = "{new_hash}""#);

                fs::write(&package.file_path, content.replace(&old_pattern, &new_pattern))?;
            }
        }
    }

    Ok(())
}
