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
}
