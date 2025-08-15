use anyhow::Result;

use crate::package::Package;
use crate::updater::NixPackageUpdater;

impl NixPackageUpdater {
    pub fn update_pypi_package(&self, package: &mut Package) -> Result<()> {
        //
        // Get latest version from PyPI using the client
        let Some(data) = self.pypi_client.project(&package.name)? else {
            package.result.failed(format!("Package '{}' not found on PyPI", package.name()));
            return Ok(());
        };

        let latest_version = data.info.version;

        if self.should_skip_update(&package.version, &latest_version) {
            package.result.up_to_date();
            return Ok(());
        }

        let mut ast = Self::ast(package);

        // Update platform hashes
        if let Some(releases) = data.releases.get(&latest_version) {
            ast.update_pypi_hashes(releases, "pypi")?;
        }

        ast.set("version", &package.version, &latest_version)?;

        Self::write(&ast, package)?;

        package.result.success().version(package.version.clone(), latest_version);

        Ok(())
    }
}
