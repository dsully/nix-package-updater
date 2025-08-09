use anyhow::Result;
use indicatif::ProgressBar;
use std::fs;

use crate::clients::nix::Nix;
use crate::nix::{extract_field_from_ast, extract_github_info, find_attr_value, update_attr_value};
use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;

use super::common::{update_cargo_or_vendor_hash, update_rev_and_hash};

impl NixPackageUpdater {
    pub fn update_rust_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Get current hash before any updates
        let current_hash = extract_field_from_ast(&package.file_path, "hash");

        let (owner, repo) = extract_github_info(&package.file_path);

        if owner.is_none() || repo.is_none() {
            return Ok(UpdateResult {
                message: Some("Could not extract owner/repo".to_string()),
                ..Default::default()
            });
        }

        let owner = owner.unwrap();
        let repo = repo.unwrap();

        let Some(current_rev) = extract_field_from_ast(&package.file_path, "rev") else {
            return Ok(UpdateResult {
                message: Some("Could not extract rev".to_string()),
                ..Default::default()
            });
        };

        let Some(latest_rev) = self.github_client.latest_commit(&owner, &repo)? else {
            return Ok(UpdateResult {
                message: Some("Failed to fetch latest commit".to_string()),
                ..Default::default()
            });
        };

        let current_version = extract_field_from_ast(&package.file_path, "version");
        let latest_version = current_version.clone();

        if current_rev == latest_rev && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: current_version.clone(),
                new_version: latest_version.clone(),
                old_hash: current_hash.clone(),
                new_hash: current_hash,
                message: Some("Already up to date".to_string()),
            });
        }

        // Update using nurl
        let nurl_url = format!("https://github.com/{owner}/{repo}");

        let Some((new_hash, _)) = Nix::nurl_hash_and_rev(&nurl_url, Some(&latest_rev))? else {
            return Ok(UpdateResult {
                message: Some("Failed to get new hash".to_string()),
                ..Default::default()
            });
        };

        let mut new_content = update_rev_and_hash(&content, Some(&current_rev), &latest_rev, &new_hash, None);

        // Update version if we have a new one
        if let (Some(old_ver), Some(new_ver)) = (&current_version, &latest_version)
            && old_ver != new_ver
        {
            new_content = update_attr_value(&new_content, "version", old_ver, new_ver);
        }

        // Clear cargoHash by finding the current value and replacing with empty string
        let ast = rnix::Root::parse(&new_content);

        if let Some(old_cargo_hash) = find_attr_value(&ast.syntax(), "cargoHash") {
            new_content = update_attr_value(&new_content, "cargoHash", &old_cargo_hash, "");
        }

        fs::write(&package.file_path, new_content)?;

        // Update cargoHash
        update_cargo_or_vendor_hash(package, "cargo", pb)?;

        Ok(UpdateResult {
            success: true,
            old_version: current_version,
            new_version: latest_version,
            old_hash: current_hash,
            new_hash: Some(new_hash),
            message: None,
        })
    }
}
