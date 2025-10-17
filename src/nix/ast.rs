use std::collections::HashMap;
use std::fs;
use std::process::Command;

use anyhow::Result;
use colored::Colorize;
use indicatif::ProgressBar;
use rnix::{Parse, Root, SyntaxKind, SyntaxNode};

use crate::package::Package;

#[derive(Debug)]
pub struct PlatformBlock {
    pub platform_name: String,
    pub attributes: std::collections::HashMap<String, String>,
}

/// Extract string value from a Nix string node
fn extract_string_value(node: &SyntaxNode) -> String {
    node.text().to_string().replace('"', "")
}

/// AST Updater that maintains the parse tree and applies updates
pub struct Ast {
    content: String,
    ast: Parse<Root>,
}

impl Ast {
    pub fn from_ast(ast: Parse<Root>) -> Self {
        let content = ast.tree().to_string();
        Self { content, ast }
    }

    /// Check if content contains a specific function call
    pub fn contains_function_call(node: &SyntaxNode, function_name: &str) -> bool {
        for child in node.descendants() {
            if child.kind() == SyntaxKind::NODE_APPLY
                && let Some(func) = child.first_child()
                && func.text().to_string().contains(function_name)
            {
                return true;
            }
        }

        false
    }

    /// Set an attribute value using precise AST-guided replacement
    pub fn set(&mut self, attr_name: &str, old_value: &str, new_value: &str) -> Result<()> {
        // Find the exact location of the attribute in the AST
        for child in self.ast.syntax().descendants() {
            if child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
                let mut found_attr = false;
                let mut string_node: Option<SyntaxNode> = None;

                for attr_child in child.children() {
                    match attr_child.kind() {
                        SyntaxKind::NODE_ATTRPATH => {
                            if let Some(ident) = attr_child.first_child()
                                && ident.text() == attr_name
                            {
                                found_attr = true;
                            }
                        }
                        SyntaxKind::NODE_STRING => {
                            if found_attr && extract_string_value(&attr_child) == old_value {
                                //
                                // Skip updating strings with interpolation: (${...})
                                let content = attr_child.text().to_string();

                                if content.contains("${") && content.contains('}') {
                                    return Ok(());
                                }

                                string_node = Some(attr_child);
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(node) = string_node {
                    // Get the exact text range and replace it
                    let range = node.text_range();
                    let start = usize::from(range.start());
                    let end = usize::from(range.end());

                    // Sigh. rnix doesn't use the rowan cursor API.
                    let new_string = format!("\"{new_value}\"");
                    self.content.replace_range(start..end, &new_string);

                    // Re-parse to keep AST in sync
                    self.ast = rnix::Root::parse(&self.content);
                    return Ok(());
                }
            }
        }

        anyhow::bail!("Attribute '{attr_name}' with value '{old_value}' not found")
    }

    /// Get the current content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Get an attribute value from the AST
    pub fn get(&self, field_name: &str) -> Option<String> {
        // First try to find as an attribute
        if let Some(value) = self.get_internal(field_name) {
            return Some(value);
        }

        // If not found, try to find as a let binding or inherit
        self.get_from_let_or_inherit(field_name)
    }

    /// Helper to get attribute values in Nix AST
    fn get_internal(&self, attr_name: &str) -> Option<String> {
        for child in self.ast.syntax().descendants() {
            if child.kind() == SyntaxKind::NODE_ATTR_SET {
                for attr_child in child.children() {
                    if attr_child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
                        let mut key = None;
                        let mut value = None;

                        for kv_child in attr_child.children() {
                            match kv_child.kind() {
                                SyntaxKind::NODE_ATTRPATH => {
                                    if let Some(ident) = kv_child.first_child()
                                        && ident.text() == attr_name
                                    {
                                        key = Some(attr_name);
                                    }
                                }
                                SyntaxKind::NODE_STRING => {
                                    value = Some(extract_string_value(&kv_child));
                                }
                                SyntaxKind::NODE_IDENT => {
                                    // Handle identifier references like `repo = pname;`
                                    value = Some(kv_child.text().to_string());
                                }
                                _ => {}
                            }
                        }

                        if key.is_some() && value.is_some() {
                            return value;
                        }
                    }
                }
            }
        }

        None
    }

    /// Get a value from let binding or inherit statement
    fn get_from_let_or_inherit(&self, binding_name: &str) -> Option<String> {
        for child in self.ast.syntax().descendants() {
            // Check for let bindings
            if child.kind() == SyntaxKind::NODE_LET_IN {
                for let_child in child.children() {
                    if let_child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                        && let Some(ident) = let_child.first_child()
                        && ident.text() == binding_name
                    {
                        // Get the value after the = sign
                        for value_child in let_child.children() {
                            if value_child.kind() == SyntaxKind::NODE_STRING {
                                return Some(extract_string_value(&value_child));
                            }
                        }
                    }
                }
            }

            // Check for inherit statements
            if child.kind() == SyntaxKind::NODE_INHERIT {
                for inherit_child in child.children() {
                    if inherit_child.kind() == SyntaxKind::NODE_IDENT && inherit_child.text() == binding_name {
                        // For inherit, we need to look for the actual value elsewhere
                        // This is a simplified version - inherit can be complex
                        return None;
                    }
                }
            }
        }

        None
    }

    /// Get platform data structures (platformData or dists)
    pub fn platforms(&self) -> Vec<PlatformBlock> {
        let mut blocks = Vec::new();

        for child in self.ast.syntax().descendants() {
            if child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                && let Some(attr_path) = child.first_child()
            {
                let attr_name = attr_path.text().to_string();

                if attr_name == "platformData" || attr_name == "dists" {
                    // Found platform data, now look for the immediate attr set
                    for value_node in child.children() {
                        if value_node.kind() == SyntaxKind::NODE_ATTR_SET {
                            // This is the platformData/dists attr set
                            // Look for individual platform entries (direct children only)
                            for platform_entry in value_node.children() {
                                if platform_entry.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                                    && let Some(platform_name_node) = platform_entry.first_child()
                                {
                                    let platform_name = platform_name_node.text().to_string();

                                    // Extract attributes from this platform's attr set
                                    let mut platform_attrs = HashMap::new();

                                    // Look for the attr set that contains the platform attributes
                                    for platform_value in platform_entry.children() {
                                        if platform_value.kind() == SyntaxKind::NODE_ATTR_SET {
                                            // Find filename, hash, platform attributes
                                            for attr in platform_value.children() {
                                                if attr.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                                                    && let Some(attr_name_node) = attr.first_child()
                                                {
                                                    let attr_name = attr_name_node.text().to_string();

                                                    // Get the value of this attribute
                                                    for attr_value in attr.children() {
                                                        if attr_value.kind() == SyntaxKind::NODE_STRING {
                                                            let value = extract_string_value(&attr_value);

                                                            platform_attrs.insert(attr_name.clone(), value);

                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if !platform_attrs.is_empty() {
                                        blocks.push(PlatformBlock {
                                            platform_name: platform_name.trim_matches('"').to_string(),
                                            attributes: platform_attrs,
                                        });
                                    }
                                }
                            }

                            break; // Don't look deeper
                        }
                    }
                }
            }
        }

        blocks
    }

    /// Update git revision and hash attributes
    pub fn update_git(&mut self, old_rev: Option<&str>, new_rev: &str, new_hash: &str, old_hash: Option<&str>) -> Result<()> {
        // Update rev first
        if let Some(old_rev) = old_rev
            && !new_rev.is_empty()
        {
            self.set("rev", old_rev, new_rev)?;

            // Update version if it contains the old rev
            if let Some(current_version) = self.get("version")
                && current_version.contains(old_rev)
            {
                let new_version = current_version.replace(old_rev, new_rev);
                self.set("version", &current_version, &new_version)?;
            }
        }

        // Update hash
        let old_hash_value = if let Some(h) = old_hash { h.to_string() } else { self.get("hash").unwrap_or_default() };

        if !old_hash_value.is_empty() && !new_hash.is_empty() {
            self.set("hash", &old_hash_value, new_hash)?;
        }

        Ok(())
    }

    /// Update vendor hash by building the package and extracting the hash from error output
    pub fn update_vendor(&mut self, package: &Package, hash_type: &str, pb: Option<&ProgressBar>) -> Result<()> {
        //
        if let Some(pb) = pb {
            pb.set_message(format!("{}: Building to get new {hash_type}Hash...", package.name()));
        } else {
            println!("{}", format!("{}: Building to get new {hash_type}Hash...", package.name()).yellow());
        }

        // Write out the current content so "nix build" can work with the latest changes
        fs::write(&package.path, self.content())?;

        let output = Command::new("nix").args(["build", &format!(".#{}", package.name), "--no-link"]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            if let Some(new_hash) = stderr.lines().find_map(|l| Some(l.trim().split_once("got:")?.1.trim().to_string())) {
                let attr_name = format!("{hash_type}Hash");

                if let Some(old_hash) = self.get(&attr_name) {
                    self.set(&attr_name, &old_hash, &new_hash)?;
                    return Ok(());
                }

                // Handle case where hash is empty or doesn't exist
                self.set(&attr_name, "", &new_hash)?;
            }
        }

        Ok(())
    }
}
