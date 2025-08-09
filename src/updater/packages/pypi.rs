use crate::package::{Package, UpdateResult};
use crate::updater::NixPackageUpdater;
use anyhow::Result;

impl NixPackageUpdater {
    pub fn update_pypi_package(&self, package: &Package) -> Result<UpdateResult> {
        //
        // Get latest version from PyPI using the client
        let Some(data) = self.pypi_client.project(&package.name)? else {
            return Ok(UpdateResult::failed(format!("Package '{}' not found on PyPI", package.display_name())));
        };

        let latest_version = data.info.version;

        if self.should_skip_update(&package.version, &latest_version) {
            return Ok(UpdateResult::up_to_date());
        }

        let mut ast = Self::ast(package);

        // Update platform hashes
        if let Some(releases) = data.releases.get(&latest_version) {
            ast.update_pypi_hashes(releases, "pypi")?;
        }

        ast.set("version", &package.version, &latest_version)?;

        Self::write(&ast, package)?;

        Ok(UpdateResult::success().version(package.version.clone(), latest_version))
    }
}
