use anyhow::Result;
use std::fs;

use crate::nix::{extract_field_from_ast, update_attr_value};
use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;
use crate::updater::platform::update_platform_hashes;

impl NixPackageUpdater {
    pub fn update_pypi_package(&self, package: &Package) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Get current version using AST
        let Some(current_version) = extract_field_from_ast(&package.file_path, "version") else {
            return Ok(UpdateResult {
                message: Some("Could not extract version".to_string()),
                ..Default::default()
            });
        };

        // Get latest version from PyPI using the client
        let Some(data) = self.pypi_client.project(&package.name)? else {
            return Ok(UpdateResult {
                message: Some(format!("Package '{}' not found on PyPI", package.display_name())),
                ..Default::default()
            });
        };

        let latest_version = data.info.version;

        if current_version == latest_version && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: Some(current_version.clone()),
                new_version: Some(latest_version),
                message: Some("Already up to date".to_string()),
                ..Default::default()
            });
        }

        // Update version
        let mut new_content = update_attr_value(&content, "version", &current_version, &latest_version);

        // Update platform hashes
        if let Some(releases) = data.releases.get(&latest_version) {
            new_content = update_platform_hashes(&new_content, releases, "pypi")?;
        }

        fs::write(&package.file_path, new_content)?;

        Ok(UpdateResult {
            success: true,
            old_version: Some(current_version),
            new_version: Some(latest_version),
            ..Default::default()
        })
    }
}
