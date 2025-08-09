use anyhow::Result;
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub cachix_name: String,
    pub excluded_packages: Vec<String>,
}

impl Config {
    pub fn is_excluded(&self, package_name: &str) -> bool {
        self.excluded_packages.iter().any(|excluded| excluded == package_name)
    }

    pub fn load() -> Result<Self> {
        let mut figment = Figment::new().join(("cachix_name", "dsully".to_string()));

        let strategy = choose_base_strategy().expect("Unable to find base strategy");
        let config_path = strategy.config_dir().join("nix-updater").join("config.toml");

        if config_path.exists() {
            figment = figment.merge(Toml::file(config_path));
        }

        Ok(figment.merge(Env::prefixed("NIX_UPDATER_").split("_")).extract()?)
    }
}
