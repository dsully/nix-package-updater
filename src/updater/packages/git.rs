use anyhow::Result;
use indicatif::ProgressBar;
use std::fs;

use crate::clients::nix::Nix;
use crate::nix::{extract_field_from_ast, find_attr_value, update_attr_value};
use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;

use super::common::{short_hash, update_cargo_or_vendor_hash, update_rev_and_hash};

impl NixPackageUpdater {
    pub fn update_git_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Get current version if it exists
        let current_version = extract_field_from_ast(&package.file_path, "version");

        // Get homepage URL using AST
        let Some(url) = extract_field_from_ast(&package.file_path, "homepage") else {
            return Ok(UpdateResult {
                message: Some("Could not extract homepage URL".to_string()),
                ..Default::default()
            });
        };

        // Use nurl to get new hash/rev
        let Some((new_hash, new_rev)) = Nix::nurl_hash_and_rev(&url, None)? else {
            return Ok(UpdateResult {
                message: Some("nurl failed".to_string()),
                ..Default::default()
            });
        };

        // Get current values using AST
        let current_hash = extract_field_from_ast(&package.file_path, "hash");
        let current_rev = extract_field_from_ast(&package.file_path, "rev");

        if current_hash == Some(new_hash.clone()) && current_rev == new_rev && !self.force {
            // If version contains rev, it will stay the same
            let new_version = if let Some(ref ver) = current_version {
                if let Some(ref rev) = current_rev {
                    if ver.contains(rev) { Some(ver.clone()) } else { current_version.clone() }
                } else {
                    current_version.clone()
                }
            } else {
                // Use revision as version if no version field exists
                current_rev.as_ref().map(|r| short_hash(r))
            };

            return Ok(UpdateResult {
                success: true,
                old_version: current_version.clone().or_else(|| current_rev.as_ref().map(|r| short_hash(r))),
                new_version,
                old_hash: current_hash.clone(),
                new_hash: Some(new_hash.clone()),
                message: Some("Already up to date".to_string()),
            });
        }

        // Update content
        let mut new_content = update_rev_and_hash(&content, current_rev.as_deref(), &new_rev.clone().unwrap_or_default(), &new_hash, current_hash.as_deref());

        // Clear cargo/vendor hashes
        if new_content.contains("vendorHash") {
            let ast = rnix::Root::parse(&new_content);

            if let Some(old_vendor) = find_attr_value(&ast.syntax(), "vendorHash") {
                new_content = update_attr_value(&new_content, "vendorHash", &old_vendor, &new_hash);
            }
        }

        // Check if we need to update cargo/vendor hash before writing
        let needs_cargo_update = new_content.contains("cargoHash");

        fs::write(&package.file_path, new_content)?;

        // Update cargo/vendor hash if needed
        if needs_cargo_update {
            update_cargo_or_vendor_hash(package, "cargo", pb)?;
        }

        // Calculate new version - if version contains rev, it was updated
        let new_version = if let Some(ref ver) = current_version {
            if let (Some(old_rev), Some(new_r)) = (&current_rev, &new_rev) {
                if ver.contains(old_rev) {
                    Some(ver.replace(old_rev, new_r))
                } else {
                    current_version.clone()
                }
            } else {
                current_version.clone()
            }
        } else {
            // Use revision as version if no version field exists
            new_rev.as_ref().map(|r| short_hash(r))
        };

        Ok(UpdateResult {
            success: true,
            old_version: current_version.or_else(|| current_rev.as_ref().map(|r| short_hash(r))),
            new_version,
            old_hash: current_hash,
            new_hash: Some(new_hash),
            message: None,
        })
    }
}
