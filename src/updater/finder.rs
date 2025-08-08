use std::collections::HashSet;
use std::fs;
use walkdir::WalkDir;

use crate::nix::{contains_function_call, find_attr_value};
use crate::package::{Package, PackageKind};

impl super::NixPackageUpdater {
    pub fn find_packages(&self) -> Vec<Package> {
        let mut packages = Vec::new();

        let mut found_files = HashSet::new();

        // Parse package filter
        let package_filter: Option<Vec<&str>> = if self.packages == "all" {
            None
        } else {
            Some(self.packages.split(',').collect())
        };

        // Walk through all .nix files in the packages directory
        for entry in WalkDir::new("packages/")
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "nix") && e.file_type().is_file())
        {
            let path = entry.path();

            let path_str = path.to_string_lossy();

            // Skip if already processed
            if found_files.contains(path_str.as_ref()) {
                continue;
            }

            // Read and parse the file
            if let Ok(content) = fs::read_to_string(path) {
                let ast = rnix::Root::parse(&content);

                let root = ast.syntax();

                // Look for pname attribute
                if let Some(pname) = find_attr_value(&root, "pname") {
                    // Apply package filter if specified
                    if let Some(ref filter) = package_filter
                        && !filter.iter().any(|&pkg| pname.contains(pkg)) {
                            continue;
                        }

                    // Skip excluded packages
                    if self.config.is_excluded(&pname) {
                        continue;
                    }

                    // Determine package type by checking content
                    let package_type = if contains_function_call(&root, "fetchPypi") {
                        PackageKind::PyPi
                    } else if contains_function_call(&root, "rustPlatform.buildRustPackage") {
                        PackageKind::Cargo
                    } else if content.contains("github.com") && content.contains("releases") && content.contains("download") {
                        PackageKind::GitHubRelease
                    } else if find_attr_value(&root, "src").is_some() {
                        PackageKind::Git
                    } else {
                        // Skip files that don't match any known package type
                        continue;
                    };

                    // Extract homepage if available
                    let homepage = find_attr_value(&root, "homepage");

                    packages.push(Package {
                        name: pname,
                        file_path: path.to_path_buf(),
                        kind: package_type,
                        homepage,
                    });

                    found_files.insert(path_str.to_string());
                }
            }
        }

        packages
    }
}
