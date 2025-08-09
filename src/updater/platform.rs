use anyhow::Result;
use colored::Colorize;

use crate::clients::PyPiReleaseFile;
use crate::clients::nix::Nix;
use crate::nix::{find_attr_value, find_platform_blocks, find_platform_data_blocks, update_attr_value};

pub fn update_platform_hashes(content: &str, releases: &[PyPiReleaseFile], _source_type: &str) -> Result<String> {
    let mut new_content = content.to_string();

    // Parse the content to find platform data blocks
    let ast = rnix::Root::parse(content);

    let root = ast.syntax();

    // Check for platformData or dists structures
    let platform_blocks = find_platform_data_blocks(&root);

    if platform_blocks.is_empty() {
        // Fall back to the old platform blocks approach
        let platforms = find_platform_blocks(content);

        for (platform_name, platform_block) in platforms {
            // Parse the platform block to find attributes
            let ast = rnix::Root::parse(&platform_block);

            let root = ast.syntax();

            // Get platform and hash from the block
            let platform_attr = find_attr_value(&root, "platform");

            let old_hash = find_attr_value(&root, "hash");

            if let (Some(platform_value), Some(old_hash_value)) = (platform_attr, old_hash) {
                let mut url = None;

                for wheel in releases {
                    if wheel.filename.contains(&platform_value) {
                        url = Some(wheel.url.clone());

                        break;
                    }
                }

                if let Some(url) = url {
                    if let Some(new_hash) = Nix::prefetch_hash(&url)? {
                        // Update hash in platform block
                        let new_platform_block = update_attr_value(&platform_block, "hash", &old_hash_value, &new_hash);

                        new_content = new_content.replace(&platform_block, &new_platform_block);
                    } else {
                        eprintln!("{}", format!("Failed to get hash for {platform_name}").red());
                    }
                } else {
                    eprintln!("{}", format!("No URL found for platform {platform_name}").yellow());
                }
            }
        }

        return Ok(new_content);
    }

    for block in platform_blocks {
        if let Some(platform_value) = block.attributes.get("platform")
            && let Some(old_hash) = block.attributes.get("hash")
        {
            // Find matching release
            let mut url = None;

            for wheel in releases {
                if wheel.filename.contains(platform_value) {
                    url = Some(wheel.url.clone());

                    break;
                }
            }

            if let Some(url) = url {
                println!("Updating hash for platform {}", block.platform_name);

                if let Some(new_hash) = Nix::prefetch_hash(&url)? {
                    new_content = update_attr_value(&new_content, "hash", old_hash, &new_hash);
                } else {
                    eprintln!("{}", format!("Failed to get hash for platform {}", block.platform_name).red());
                }
            } else {
                eprintln!("{}", format!("No wheel found for platform {platform_value}").yellow());
            }
        }
    }

    Ok(new_content)
}

pub fn update_platform_hashes_github(content: &str, release_data: &serde_json::Value) -> Result<String> {
    let mut new_content = content.to_string();

    // Parse the content to find platform data blocks
    let ast = rnix::Root::parse(content);

    let root = ast.syntax();

    // Check for platformData structures
    let platform_blocks = find_platform_data_blocks(&root);

    if platform_blocks.is_empty() {
        // Fall back to looking for individual platform blocks
        let platforms = find_platform_blocks(content);

        for (platform_name, platform_block) in platforms {
            // Parse the platform block to find filename
            let ast = rnix::Root::parse(&platform_block);

            let root = ast.syntax();

            if let Some(filename) = find_attr_value(&root, "filename") {
                let url = format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    release_data["repo"].as_str().unwrap(),
                    release_data["tag"].as_str().unwrap(),
                    filename
                );

                if let Some(new_hash) = Nix::prefetch_hash(&url)? {
                    if let Some(old_hash) = find_attr_value(&root, "hash") {
                        new_content = new_content.replace(&platform_block, &update_attr_value(&platform_block, "hash", &old_hash, &new_hash));
                    }
                } else {
                    eprintln!("{}", format!("Failed to get hash for {platform_name}").red());
                }
            }
        }

        return Ok(new_content);
    }

    // Handle structured platform data
    for block in platform_blocks {
        if let Some(filename) = block.attributes.get("filename")
            && let Some(old_hash) = block.attributes.get("hash")
        {
            let url = format!(
                "https://github.com/{}/releases/download/{}/{}",
                release_data["repo"].as_str().unwrap(),
                release_data["tag"].as_str().unwrap(),
                filename
            );

            // Get new hash
            if let Some(new_hash) = Nix::prefetch_hash(&url)? {
                new_content = update_attr_value(&new_content, "hash", old_hash, &new_hash);
            } else {
                eprintln!("{}", format!("Failed to get hash for {filename}").red());
            }
        }
    }

    Ok(new_content)
}
