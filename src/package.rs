use std::fs;
use std::path::PathBuf;

use colored::Colorize;
use git_url_parse::GitUrl;
use rnix::{Parse, Root};
use walkdir::WalkDir;

use crate::nix::ast::Ast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    PyPi,
    GitHubRelease,
    Cargo,
    Git,
}

impl std::fmt::Display for PackageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageKind::PyPi => write!(f, "PyPI"),
            PackageKind::GitHubRelease => write!(f, "GitHub Release"),
            PackageKind::Cargo => write!(f, "Cargo"),
            PackageKind::Git => write!(f, "Git"),
        }
    }
}

#[derive(Debug, Default)]
pub struct UpdateResult {
    pub updated: bool,
    pub built: bool,
    pub message: Option<String>,

    pub old_version: Option<String>,
    pub new_version: Option<String>,

    pub old_git_commit: Option<String>,
    pub new_git_commit: Option<String>,
}

impl UpdateResult {
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            updated: false,
            message: Some(message.into()),
            ..Default::default()
        }
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self {
            updated: true,
            message: Some(message.into()),
            ..Default::default()
        }
    }

    pub fn success() -> Self {
        Self {
            updated: true,
            ..Default::default()
        }
    }

    pub fn up_to_date() -> Self {
        Self {
            updated: true,
            message: Some("Already up to date".to_string()),
            ..Default::default()
        }
    }

    pub fn version(mut self, old: String, new: String) -> Self {
        if !old.contains("${") && !old.contains('}') {
            self.old_version = Some(old);
            self.new_version = Some(new);
        }

        self
    }

    pub fn git_commit(mut self, old: String, new: String) -> Self {
        self.old_git_commit = Some(old);
        self.new_git_commit = Some(new);
        self
    }
}

pub struct Package {
    pub name: String,
    pub path: PathBuf,
    pub kind: PackageKind,
    pub homepage: GitUrl,
    pub ast: Parse<Root>,

    pub version: String,
    pub nix_hash: String,
}

impl Package {
    pub fn discover(include: &[String], exclude: &[String]) -> Vec<Package> {
        let mut packages = Vec::new();

        for entry in WalkDir::new("packages/")
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "nix") && e.file_type().is_file())
        {
            let path = entry.path();

            if let Ok(content) = fs::read_to_string(path) {
                //
                let ast = rnix::Root::parse(&content);
                let root = ast.syntax();

                let updater = Ast::from_ast(ast.clone());
                if let Some(pname) = updater.get("pname") {
                    //
                    // Apply package filter if specified
                    if !include.is_empty() && !include.iter().any(|pkg| pname.contains(pkg)) {
                        continue;
                    }

                    // Skip excluded packages
                    if exclude.iter().any(|e| e == &pname) {
                        continue;
                    }

                    // Determine package type by checking content
                    let package_type = if Ast::contains_function_call(&root, "fetchPypi") {
                        PackageKind::PyPi
                    } else if Ast::contains_function_call(&root, "rustPlatform.buildRustPackage") {
                        PackageKind::Cargo
                    } else if content.contains("github.com") && content.contains("releases") && content.contains("download") {
                        PackageKind::GitHubRelease
                    } else {
                        PackageKind::Git
                    };

                    let homepage = updater
                        .get("homepage")
                        .unwrap_or_else(|| panic!("Failed to find 'homepage' attribute in: {}", path.display()));

                    packages.push(Self {
                        name: pname,
                        path: path.to_path_buf(),
                        kind: package_type,
                        homepage: GitUrl::parse(&homepage).expect("Failed to parse homepage URL"),
                        nix_hash: updater.get("hash").unwrap_or_else(|| panic!("Failed to find 'hash' attribute in: {}", path.display())),
                        version: updater
                            .get("version")
                            .unwrap_or_else(|| panic!("Failed to find 'version' attribute in: {}", path.display())),
                        ast: ast.clone(),
                    });
                }
            }
        }

        packages
    }

    /// Format the package name with hyperlink if homepage is available
    pub fn display_name(&self) -> String {
        format!("\x1B]8;;{}\x1B\\{}\x1B]8;;\x1B\\", &self.homepage.to_string(), &self.name).cyan().to_string()
    }

    /// Get the visual display width of the package name (excluding escape sequences)
    pub fn display_width(&self) -> usize {
        self.name.len()
    }
}
