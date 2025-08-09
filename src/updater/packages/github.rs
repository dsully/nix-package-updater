use std::fs;

use anyhow::Result;
use git_url_parse::GitUrl;

use crate::clients::nix::Nix;
use crate::nix::{extract_field_from_ast, update_attr_value};
use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;
use crate::updater::platform::update_platform_hashes_github;

impl NixPackageUpdater {
    pub fn update_github_package(&self, package: &Package) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Get current hash if it exists (GitHub releases may have source hashes)
        let current_hash = extract_field_from_ast(&package.file_path, "hash");

        // Get current version using AST
        let Some(current_version) = extract_field_from_ast(&package.file_path, "version") else {
            return Ok(UpdateResult {
                message: Some("Could not extract version".to_string()),
                ..Default::default()
            });
        };

        let Some(homepage) = extract_field_from_ast(&package.file_path, "homepage") else {
            return Ok(UpdateResult {
                message: Some("Could not extract homepage URL".to_string()),
                ..Default::default()
            });
        };

        let url = GitUrl::parse(&homepage)?;

        let owner = url.owner.as_ref().ok_or_else(|| anyhow::anyhow!("Could not extract owner from GitHub URL: {homepage}"))?;
        let repo_name = url.name;

        let Some(latest_tag) = self.github_client.latest_release(owner, &repo_name)? else {
            // No releases found - keep current version and hash
            return Ok(UpdateResult {
                success: true,
                old_version: Some(current_version.clone()),
                new_version: Some(current_version),
                old_hash: current_hash.clone(),
                new_hash: current_hash,
                message: Some("No releases found on GitHub - keeping current version".to_string()),
            });
        };

        let latest_version = latest_tag.trim_start_matches('v').to_string();

        if current_version == latest_version && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: Some(current_version.clone()),
                new_version: Some(latest_version),
                old_hash: current_hash.clone(),
                new_hash: current_hash.clone(),
                message: Some("Already up to date".to_string()),
            });
        }

        // Update version
        let mut new_content = update_attr_value(&content, "version", &current_version, &latest_version);

        let new_hash = current_hash.as_ref().and_then(|_| {
            Nix::nurl_hash_and_rev(&format!("https://github.com/{owner}/{repo_name}/archive/refs/tags/{latest_tag}.tar.gz"), None)
                .ok()
                .flatten()
                .map(|(new_hash, _)| new_hash)
        });

        // Update hash if we have both old and new
        if let (Some(old_h), Some(new_h)) = (&current_hash, &new_hash) {
            new_content = update_attr_value(&new_content, "hash", old_h, new_h);
        }

        // Update platform hashes using release tag
        let release_data = serde_json::json!({
            "tag": latest_tag,  // Use release tag for hash generation
            "repo": url.fullname,
        });

        new_content = update_platform_hashes_github(&new_content, &release_data)?;

        fs::write(&package.file_path, new_content)?;

        Ok(UpdateResult {
            success: true,
            old_version: Some(current_version),
            new_version: Some(latest_version),
            old_hash: current_hash,
            new_hash: new_hash.clone(),
            message: None,
        })
    }
}
