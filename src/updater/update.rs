use anyhow::Result;
use colored::Colorize;
use indicatif::ProgressBar;
use std::fs;
use std::process::Command;

use crate::clients::nix::get_nurl_data;
use crate::nix::{extract_field_from_ast, extract_github_info, extract_hash_from_error, find_attr_value, update_attr_value};
use crate::package::{Package, UpdateResult};

use super::platform::{extract_github_repo, update_platform_hashes, update_platform_hashes_github};

impl super::NixPackageUpdater {
    pub fn update_pypi_package(&self, package: &Package) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Get current version using AST
        let Some(current_version) = extract_field_from_ast(&package.file_path, "version")? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Could not extract version".to_string()),
            });
        };

        // Get latest version from PyPI using the client
        let Some(data) = self.pypi_client.get_project(&package.name)? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some(format!("Package '{}' not found on PyPI", package.name)),
            });
        };

        let latest_version = data.info.version;

        if current_version == latest_version && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: Some(current_version.clone()),
                new_version: Some(latest_version),
                message: Some("Already up to date".to_string()),
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
            message: None,
        })
    }

    pub fn update_github_package(&self, package: &Package) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Extract repo info using AST
        let homepage = extract_field_from_ast(&package.file_path, "homepage")?;

        let repo = if let Some(url) = &homepage {
            extract_github_repo(url)
        } else {
            // Try to find GitHub URL in content
            if let Some(pos) = content.find("github.com/") {
                let url_part = &content[pos..];

                if let Some(end) = url_part.find('"') {
                    extract_github_repo(&url_part[..end])
                } else {
                    None
                }
            } else {
                None
            }
        };

        let Some(repo) = repo else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Could not extract GitHub repo".to_string()),
            });
        };

        // Parse owner/repo
        let parts: Vec<&str> = repo.split('/').collect();

        if parts.len() != 2 {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Invalid GitHub repo format".to_string()),
            });
        }

        let owner = parts[0];

        let repo_name = parts[1];

        // Get current version using AST
        let Some(current_version) = extract_field_from_ast(&package.file_path, "version")? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Could not extract version".to_string()),
            });
        };

        // Get latest release using octocrab
        let Some(latest_tag) = self.github_client.get_latest_release(owner, repo_name)? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("No releases found on GitHub".to_string()),
            });
        };

        let latest_version = latest_tag.trim_start_matches('v').to_string();

        if current_version == latest_version && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: Some(current_version.clone()),
                new_version: Some(latest_version),
                message: Some("Already up to date".to_string()),
            });
        }

        // Update version
        let mut new_content = update_attr_value(&content, "version", &current_version, &latest_version);

        // Update platform hashes
        let release_data = serde_json::json!({
            "tag": latest_tag,
            "repo": repo,
        });

        new_content = update_platform_hashes_github(&new_content, &release_data)?;

        fs::write(&package.file_path, new_content)?;

        Ok(UpdateResult {
            success: true,
            old_version: Some(current_version),
            new_version: Some(latest_version),
            message: None,
        })
    }

    pub fn update_rust_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Extract repo info from fetchFromGitHub
        let (owner, repo) = extract_github_info(&package.file_path)?;

        if owner.is_none() || repo.is_none() {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Could not extract owner/repo".to_string()),
            });
        }

        let owner = owner.unwrap();

        let repo = repo.unwrap();

        // Get current rev using AST
        let Some(current_rev) = extract_field_from_ast(&package.file_path, "rev")? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Could not extract rev".to_string()),
            });
        };

        // Get latest commit using octocrab
        let Some(latest_rev) = self.github_client.get_latest_commit(&owner, &repo)? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Failed to fetch latest commit".to_string()),
            });
        };

        if current_rev == latest_rev && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: None,
                new_version: None,
                message: Some("Already up to date".to_string()),
            });
        }

        // Update using nurl
        let nurl_url = format!("https://github.com/{owner}/{repo}");

        let Some(nurl_data) = get_nurl_data(&nurl_url, Some(&latest_rev))? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Failed to get new hash".to_string()),
            });
        };

        let new_hash = nurl_data.args.hash;

        let mut new_content = Self::update_rev_and_hash(&content, Some(&current_rev), &latest_rev, &new_hash, None);

        // Clear cargoHash
        let old_cargo = r#"cargoHash = """#.to_string();

        let new_cargo = r#"cargoHash = """#.to_string();

        new_content = new_content.replace(&old_cargo, &new_cargo);

        fs::write(&package.file_path, new_content)?;

        // Update cargoHash
        Self::update_cargo_or_vendor_hash(package, "cargo", pb)?;

        Ok(UpdateResult {
            success: true,
            old_version: Some(current_rev[..8.min(current_rev.len())].to_string()),
            new_version: Some(latest_rev[..8.min(latest_rev.len())].to_string()),
            message: None,
        })
    }

    pub fn update_git_package(&mut self, package: &Package, pb: Option<&ProgressBar>) -> Result<UpdateResult> {
        let content = fs::read_to_string(&package.file_path)?;

        // Get homepage URL using AST
        let Some(url) = extract_field_from_ast(&package.file_path, "homepage")? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("Could not extract homepage URL".to_string()),
            });
        };

        // Use nurl to get new hash/rev
        let Some(nurl_data) = get_nurl_data(&url, None)? else {
            return Ok(UpdateResult {
                success: false,
                old_version: None,
                new_version: None,
                message: Some("nurl failed".to_string()),
            });
        };

        let new_hash = nurl_data.args.hash;

        let new_rev = nurl_data.args.rev;

        // Get current values using AST
        let current_hash = extract_field_from_ast(&package.file_path, "hash")?;

        let current_rev = extract_field_from_ast(&package.file_path, "rev")?;

        if current_hash == Some(new_hash.clone()) && current_rev == new_rev && !self.force {
            return Ok(UpdateResult {
                success: true,
                old_version: None,
                new_version: None,
                message: Some("Already up to date".to_string()),
            });
        }

        // Update content
        let mut new_content = Self::update_rev_and_hash(
            &content,
            current_rev.as_deref(),
            &new_rev.clone().unwrap_or_default(),
            &new_hash,
            current_hash.as_deref(),
        );

        // Clear cargo/vendor hashes
        // cargoHash is already empty, no need to replace

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
            Self::update_cargo_or_vendor_hash(package, "cargo", pb)?;
        }

        Ok(UpdateResult {
            success: true,
            old_version: current_rev.map(|r| r[..8.min(r.len())].to_string()),
            new_version: new_rev.map(|r| r[..8.min(r.len())].to_string()),
            message: None,
        })
    }

    fn update_rev_and_hash(content: &str, old_rev: Option<&str>, new_rev: &str, new_hash: &str, old_hash: Option<&str>) -> String {
        let mut new_content = content.to_string();

        // Update rev
        if let Some(old_rev) = old_rev {
            if !new_rev.is_empty() {
                new_content = update_attr_value(&new_content, "rev", old_rev, new_rev);

                // Update version if it contains rev
                let ast = rnix::Root::parse(&new_content);

                if let Some(current_version) = find_attr_value(&ast.syntax(), "version") {
                    if current_version.contains(old_rev) {
                        let new_version = current_version.replace(old_rev, new_rev);

                        new_content = update_attr_value(&new_content, "version", &current_version, &new_version);
                    }
                }
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

    fn update_cargo_or_vendor_hash(package: &Package, hash_type: &str, pb: Option<&ProgressBar>) -> Result<()> {
        if let Some(pb) = pb {
            pb.set_message(format!("{}: Building to get new {hash_type}Hash...", package.name));
        } else {
            println!("{}", format!("{}: Building to get new {hash_type}Hash...", package.name).yellow());
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
                    // If no existing hash, look for empty hash
                    let old_pattern = format!(r#"{attr_name} = """#);

                    let new_pattern = format!(r#"{attr_name} = "{new_hash}""#);

                    let new_content = content.replace(&old_pattern, &new_pattern);

                    fs::write(&package.file_path, new_content)?;
                }
            }
        }

        Ok(())
    }
}
