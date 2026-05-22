use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use colored::{ColoredString, Colorize};
use git_url_parse::GitUrl;
use rnix::{Parse, Root};
use rootcause::Result;
use strum::Display;
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::nix::ast::Ast;
use crate::updater::short_hash;

#[derive(Clone, Copy, Display, PartialEq, Eq)]
pub enum PackageKind {
    PyPi,
    GitHub,
    Cargo,
    Npm,
    Go,
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
    pub fn discover(root: &Path, include: &[String], exclude: &[String]) -> Vec<Package> {
        let mut packages = Vec::new();

        for entry in WalkDir::new(root)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "nix") && e.file_type().is_file())
        {
            let path = entry.path();

            let Ok(content) = fs::read_to_string(path) else {
                warn!(path = %path.display(), "Could not read file");
                continue;
            };

            let ast = rnix::Root::parse(&content);
            let root_syntax = ast.syntax();

            let updater = Ast::from_ast(ast.clone());

            let Some(pname) = updater.get("pname") else {
                continue;
            };

            // Apply package filter if specified
            if !include.is_empty() && !include.iter().any(|pkg| pname.contains(pkg)) {
                continue;
            }

            // Skip excluded packages
            if exclude.iter().any(|e| e == &pname) {
                continue;
            }

            // Skip packages not supported on the current platform
            if !Self::supported_on_current_platform(&updater) {
                info!(package = %pname, "Skipping: not supported on current platform");
                continue;
            }

            // Determine package type by checking content
            let package_type = Self::detect_package_kind(&root_syntax, &content);

            let Some(homepage_str) = updater.get("homepage") else {
                warn!(package = %pname, "Skipping: missing 'homepage' attribute");
                continue;
            };

            let Ok(homepage) = GitUrl::parse(&homepage_str) else {
                warn!(package = %pname, url = %homepage_str, "Skipping: invalid homepage URL");
                continue;
            };

            // Optional for fetchGit
            let nix_hash = updater.get("hash").unwrap_or_default();

            let Some(version) = updater.get("version") else {
                warn!(package = %pname, "Skipping: missing 'version' attribute");
                continue;
            };

            packages.push(Self {
                name: pname,
                path: path.to_path_buf(),
                kind: package_type,
                homepage,
                nix_hash,
                version,
                ast: ast.clone(),
                result: UpdateResult::default(),
            });
        }

        packages
    }

    fn detect_package_kind(root: &rnix::SyntaxNode, content: &str) -> PackageKind {
        if Ast::contains_function_call(root, "fetchPypi") {
            PackageKind::PyPi
        } else if Ast::contains_function_call(root, "rustPlatform.buildRustPackage") {
            PackageKind::Cargo
        } else if Ast::contains_function_call(root, "buildNpmPackage") {
            PackageKind::Npm
        } else if Ast::contains_function_call(root, "buildGoModule") {
            PackageKind::Go
        } else if content.contains("github.com") && content.contains("releases") && content.contains("download") {
            PackageKind::GitHub
        } else {
            PackageKind::Git
        }
    }

    fn supported_on_current_platform(ast: &Ast) -> bool {
        let Some(platform) = ast.meta_platforms() else {
            return true;
        };

        match platform.as_str() {
            "linux" => cfg!(target_os = "linux"),
            "darwin" => cfg!(target_os = "macos"),
            // unix and all match everything
            _ => true,
        }
    }

    /// Format the package name with hyperlink if homepage is available
    pub fn name(&self) -> String {
        format!("\x1B]8;;{}\x1B\\{}\x1B]8;;\x1B\\", &self.homepage.to_string(), &self.name).cyan().to_string()
    }

    /// Get the visual display width of the package name (excluding escape sequences)
    pub fn display_width(&self) -> usize {
        self.name.len()
    }

    pub fn ast(&self) -> Ast {
        Ast::from_ast(self.ast.clone())
    }

    pub fn write(&self, ast: &Ast) -> Result<()> {
        Ok(std::fs::write(&self.path, ast.content())?)
    }

    pub fn is_up_to_date(&self) -> bool {
        self.result.status.contains(&UpdateStatus::UpToDate)
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

    pub changes: Vec<String>,
}

impl UpdateResult {
    pub fn status(&self, check: UpdateStatus) -> ColoredString {
        match check {
            _ if self.status.contains(&UpdateStatus::Failed) => "✗".red(),
            UpdateStatus::Built | UpdateStatus::Updated | UpdateStatus::Cached if self.status.contains(&check) => "✓".green(),
            _ => "-".yellow(),
        }
    }

    pub fn failed(&mut self, message: impl Into<String>) -> &mut Self {
        self.status.clear();
        self.status.insert(UpdateStatus::Failed);

        self.message = Some(message.into());
        self
    }

    pub fn message(&mut self, message: impl Into<String>) -> &mut Self {
        self.message = Some(message.into());
        self
    }

    pub fn up_to_date(&mut self) -> &mut Self {
        self.status.insert(UpdateStatus::UpToDate);
        self.message = Some("Up to date".to_string());
        self
    }

    pub fn git_commit(&mut self, old: Option<&str>, new: Option<&str>) -> &mut Self {
        //
        if let (Some(o), Some(n)) = (old, new)
            && o != n
        {
            self.status.insert(UpdateStatus::Updated);

            self.changes.push(format!("{} → {}", short_hash(o), short_hash(n)));

            self.old_git_commit = Some(o.to_string());
            self.new_git_commit = Some(n.to_string());
        }

        self
    }

    pub fn version(&mut self, old: Option<&str>, new: Option<&str>) -> &mut Self {
        //
        if let (Some(o), Some(n)) = (old, new)
            && o != n
            && (!o.contains("${") && !o.contains('}'))
        {
            self.status.insert(UpdateStatus::Updated);

            self.changes.push(format!("{o} → {n}"));

            self.old_version = old.map(String::from);
            self.new_version = new.map(String::from);
        }

        self
    }
}
