use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use colored::{ColoredString, Colorize};
use git_url_parse::GitUrl;
use rnix::{Parse, Root};
use strum::Display;
use walkdir::WalkDir;

use crate::nix::ast::Ast;
use crate::updater::short_hash;

#[derive(Clone, Copy, Display, PartialEq, Eq)]
pub enum PackageKind {
    PyPi,
    GitHub,
    Cargo,
    Git,
}

pub struct Package {
    pub name: String,
    pub path: PathBuf,
    pub kind: PackageKind,
    pub homepage: GitUrl,
    pub ast: Parse<Root>,

    pub version: String,
    pub nix_hash: String,

    pub result: UpdateResult,
}

impl Package {
    pub fn discover(root: &PathBuf, include: &[String], exclude: &[String]) -> Vec<Package> {
        let mut packages = Vec::new();

        for entry in WalkDir::new(root)
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
                        PackageKind::GitHub
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
                        result: UpdateResult::default(),
                    });
                }
            }
        }

        packages
    }

    /// Format the package name with hyperlink if homepage is available
    pub fn name(&self) -> String {
        format!("\x1B]8;;{}\x1B\\{}\x1B]8;;\x1B\\", &self.homepage.to_string(), &self.name).cyan().to_string()
    }

    /// Get the visual display width of the package name (excluding escape sequences)
    pub fn display_width(&self) -> usize {
        self.name.len()
    }

    /// Common helper to create an Ast from a package
    pub fn ast(&self) -> Ast {
        Ast::from_ast(self.ast.clone())
    }

    /// Common helper to finalize an Ast by writing to file
    pub fn write(&self, ast: &Ast) -> Result<()> {
        Ok(std::fs::write(&self.path, ast.content())?)
    }
}

#[derive(Clone, Copy, Debug, Default, Display, Hash, PartialEq, Eq)]
pub enum UpdateStatus {
    Built,
    Cached,
    Failed,
    Updated,
    UpToDate,
    #[default]
    Unknown,
}

#[derive(Debug, Default)]
pub struct UpdateResult {
    pub status: HashSet<UpdateStatus>,

    pub message: Option<String>,

    pub old_version: Option<String>,
    pub new_version: Option<String>,

    pub old_git_commit: Option<String>,
    pub new_git_commit: Option<String>,
}

impl UpdateResult {
    pub fn status(&self, check: UpdateStatus) -> ColoredString {
        match check {
            _ if self.status.contains(&UpdateStatus::Failed) => "✗".red(),
            UpdateStatus::Built | UpdateStatus::Updated | UpdateStatus::Cached if self.status.contains(&check) => "✓".green(),
            _ => "-".yellow(),
        }
    }

    // pub fn status(&self, check: UpdateStatus) -> ColoredString {
    //     match (self.status, check) {
    //         (UpdateStatus::Failed, _) => "✗".red(),
    //         (_, UpdateStatus::Built | UpdateStatus::Updated | UpdateStatus::Cached) => "✓".green(),
    //         (_, _) => "-".yellow(),
    //     }
    // }

    pub fn failed(&mut self, message: impl Into<String>) -> &mut Self {
        // self.status = UpdateStatus::Failed;
        self.status.clear();
        self.status.insert(UpdateStatus::Failed);

        self.message = Some(message.into());
        self
    }

    pub fn message(&mut self, message: impl Into<String>) -> &mut Self {
        self.message = Some(message.into());
        self
    }

    pub fn success(&mut self) -> &mut Self {
        self.status.insert(UpdateStatus::Updated);
        self
    }

    pub fn up_to_date(&mut self) -> &mut Self {
        self.status.insert(UpdateStatus::UpToDate);
        self.message = Some("Up to date".to_string());
        self
    }

    pub fn version(&mut self, old: String, new: String) -> &mut Self {
        if !old.contains("${") && !old.contains('}') {
            self.old_version = Some(old);
            self.new_version = Some(new);
        }

        self
    }

    pub fn git_commit(&mut self, old: String, new: String) -> &mut Self {
        self.old_git_commit = Some(old);
        self.new_git_commit = Some(new);
        self
    }

    pub fn changes(&self) -> Vec<String> {
        let mut changes = Vec::new();

        if let (Some(o), Some(n)) = (&self.old_version, &self.new_version)
            && o != n
        {
            changes.push(format!("{o} → {n}"));
        }

        if let (Some(o), Some(n)) = (&self.old_git_commit, &self.new_git_commit)
            && o != n
        {
            changes.push(format!("{} → {}", short_hash(o), short_hash(n)));
        }

        changes
    }
}
