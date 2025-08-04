use colored::Colorize;
use std::path::PathBuf;

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

#[derive(Debug)]
pub struct UpdateResult {
    pub success: bool,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub file_path: PathBuf,
    pub kind: PackageKind,
    pub homepage: Option<String>,
}

impl Package {
    /// Format the package name with hyperlink if homepage is available
    pub fn display_name(&self) -> String {
        if let Some(homepage) = &self.homepage {
            hyperlink(homepage, &self.name)
        } else {
            self.name.cyan().to_string()
        }
    }

    /// Get the visual display width of the package name (excluding escape sequences)
    pub fn display_width(&self) -> usize {
        self.name.len()
    }
}

/// Emit an OSC-8 hyperlink escape sequence.
pub fn hyperlink(url: &str, text: &str) -> String {
    format!("\x1B]8;;{url}\x1B\\{text}\x1B]8;;\x1B\\").cyan().to_string()
}
