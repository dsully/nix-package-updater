#[derive(Debug, Clone)]
pub struct Config {
    pub cachix_name: String,
    pub build_timeout: u64,
    pub excluded_packages: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cachix_name: "dsully".to_string(),
            build_timeout: 3600,
            excluded_packages: vec!["toml-fmt-common".to_string(), "chromium".to_string()],
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(name) = std::env::var("NIX_UPDATER_CACHIX_NAME") {
            config.cachix_name = name;
        }

        if let Ok(timeout) = std::env::var("NIX_UPDATER_BUILD_TIMEOUT") {
            if let Ok(timeout) = timeout.parse() {
                config.build_timeout = timeout;
            }
        }

        config
    }

    pub fn is_excluded(&self, package_name: &str) -> bool {
        self.excluded_packages.iter().any(|excluded| excluded == package_name)
    }

    pub fn load() -> Self {
        Self::from_env()
    }
}
