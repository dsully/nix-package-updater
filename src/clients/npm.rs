use reqwest::blocking::Client;
use rootcause::{Result, bail};
use serde::Deserialize;

#[derive(Deserialize)]
struct RegistryVersion {
    version: String,
}

pub struct NpmClient {
    client: Client,
}

impl NpmClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(format!("nix-updater/{}", env!("CARGO_PKG_VERSION")))
                .build()?,
        })
    }

    pub fn download_package_lock(&self, url: &str) -> Result<Option<String>> {
        match self.client.get(url).send() {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(Some(response.text()?))
                } else if response.status().as_u16() == 404 {
                    Ok(None)
                } else {
                    bail!("Failed to download package-lock.json: status {}", response.status())
                }
            }
            Err(e) => bail!("Failed to download package-lock.json: {e}"),
        }
    }

    /// The `latest` dist-tag version of `name` on the npm registry; scoped names need no escaping.
    pub fn latest_version(&self, name: &str) -> Result<Option<String>> {
        let url = format!("https://registry.npmjs.org/{name}/latest");

        match self.client.get(&url).send() {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(Some(response.json::<RegistryVersion>()?.version))
                } else if response.status().as_u16() == 404 {
                    Ok(None)
                } else {
                    bail!("npm registry returned status: {}", response.status())
                }
            }
            Err(e) => bail!("Failed to fetch npm registry data: {e}"),
        }
    }
}
