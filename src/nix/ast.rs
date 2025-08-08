use anyhow::Result;
use rnix::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct PlatformBlock {
    pub platform_name: String,
    pub attributes: std::collections::HashMap<String, String>,
}

/// Helper to find attribute values in Nix AST
pub fn find_attr_value(node: &SyntaxNode, attr_name: &str) -> Option<String> {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::NODE_ATTR_SET {
            for attr_child in child.children() {
                if attr_child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
                    let mut key = None;

                    let mut value = None;

                    for kv_child in attr_child.children() {
                        match kv_child.kind() {
                            SyntaxKind::NODE_ATTRPATH => {
                                if let Some(ident) = kv_child.first_child()
                                    && ident.text() == attr_name {
                                        key = Some(attr_name);
                                    }
                            }
                            SyntaxKind::NODE_STRING => {
                                value = Some(extract_string_value(&kv_child));
                            }
                            SyntaxKind::NODE_IDENT => {
                                // Handle identifier references like `repo = pname;`
                                // For now, just return the identifier name
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

/// Extract string value from a Nix string node
pub fn extract_string_value(node: &SyntaxNode) -> String {
    let text = node.text().to_string();

    // Remove quotes
    if text.starts_with('"') && text.ends_with('"') {
        text[1..text.len() - 1].to_string()
    } else {
        text
    }
}

/// Check if content contains a specific function call
pub fn contains_function_call(node: &SyntaxNode, function_name: &str) -> bool {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::NODE_APPLY
            && let Some(func) = child.first_child()
                && func.text().to_string().contains(function_name) {
                    return true;
                }
    }

    false
}

/// Extract field from a Nix file using AST
pub fn extract_field_from_ast(path: &Path, field_name: &str) -> Result<Option<String>> {
    let content = fs::read_to_string(path)?;

    let ast = rnix::Root::parse(&content);

    // First try to find as an attribute
    if let Some(value) = find_attr_value(&ast.syntax(), field_name) {
        return Ok(Some(value));
    }

    // If not found, try to find as a let binding or inherit
    Ok(find_let_binding_or_inherit(&ast.syntax(), field_name))
}

/// Find a value from let binding or inherit statement
pub fn find_let_binding_or_inherit(node: &SyntaxNode, binding_name: &str) -> Option<String> {
    for child in node.descendants() {
        // Check for let bindings
        if child.kind() == SyntaxKind::NODE_LET_IN {
            for let_child in child.children() {
                if let_child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                    && let Some(ident) = let_child.first_child()
                        && ident.text() == binding_name {
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

/// Find attribute value within a fetchFromGitHub call
pub fn find_attr_in_fetch_from_github(node: &SyntaxNode, attr_name: &str) -> Option<String> {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::NODE_APPLY
            && let Some(func) = child.first_child()
                && func.text().to_string().contains("fetchFromGitHub") {
                    // Look for the attribute set argument
                    for apply_child in child.children() {
                        if apply_child.kind() == SyntaxKind::NODE_ATTR_SET
                            && let Some(value) = find_attr_value(&apply_child, attr_name) {
                                // If the value is "pname", resolve it from the parent scope
                                if value == "pname" && attr_name == "repo" {
                                    // Look for pname in the parent scope
                                    if let Some(pname) = find_attr_value(node, "pname") {
                                        return Some(pname);
                                    }
                                }

                                return Some(value);
                            }
                    }
                }
    }

    None
}

/// Extract owner/repo from fetchFromGitHub in a Nix file
pub fn extract_github_info(path: &Path) -> Result<(Option<String>, Option<String>)> {
    let content = fs::read_to_string(path)?;

    let ast = rnix::Root::parse(&content);
    let root = ast.syntax();

    let owner = find_attr_in_fetch_from_github(&root, "owner");
    let repo = find_attr_in_fetch_from_github(&root, "repo");

    Ok((owner, repo))
}

/// Find platform blocks in content
pub fn find_platform_blocks(content: &str) -> Vec<(String, String)> {
    let mut platforms = Vec::new();

    let ast = rnix::Root::parse(content);

    let root = ast.syntax();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::NODE_ATTR_SET {
            // Look for pattern like: platform-name = { ... }
            if let Some(parent) = node.parent()
                && parent.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                    && let Some(key_node) = parent.children().find(|n| n.kind() == SyntaxKind::NODE_ATTRPATH) {
                        let platform_name = key_node.text().to_string();

                        if platform_name.contains('-') {
                            let block_text = parent.text().to_string();

                            platforms.push((platform_name, block_text));
                        }
                    }
        }
    }

    platforms
}

/// Update attribute value in content
pub fn update_attr_value(content: &str, attr_name: &str, old_value: &str, new_value: &str) -> String {
    let old_pattern = format!(r#"{attr_name} = "{old_value}""#);
    let new_pattern = format!(r#"{attr_name} = "{new_value}""#);

    content.replace(&old_pattern, &new_pattern)
}

/// Find platform data structures (platformData or dists)
pub fn find_platform_data_blocks(node: &SyntaxNode) -> Vec<PlatformBlock> {
    let mut blocks = Vec::new();

    for child in node.descendants() {
        if child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
            && let Some(attr_path) = child.first_child() {
                let attr_name = attr_path.text().to_string();

                if attr_name == "platformData" || attr_name == "dists" {
                    // Found platform data, now look for the immediate attr set
                    for value_node in child.children() {
                        if value_node.kind() == SyntaxKind::NODE_ATTR_SET {
                            // This is the platformData/dists attr set
                            // Look for individual platform entries (direct children only)
                            for platform_entry in value_node.children() {
                                if platform_entry.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                                    && let Some(platform_name_node) = platform_entry.first_child() {
                                        let platform_name = platform_name_node.text().to_string();

                                        // Extract attributes from this platform's attr set
                                        let mut platform_attrs = HashMap::new();

                                        // Look for the attr set that contains the platform attributes
                                        for platform_value in platform_entry.children() {
                                            if platform_value.kind() == SyntaxKind::NODE_ATTR_SET {
                                                // Find filename, hash, platform attributes
                                                for attr in platform_value.children() {
                                                    if attr.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
                                                        && let Some(attr_name_node) = attr.first_child() {
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
