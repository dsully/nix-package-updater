use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CrateResponse {
    #[serde(rename = "crate")]
    pub crate_data: CrateInfo,
}

#[derive(Debug, Deserialize)]
pub struct CrateInfo {
    pub max_version: String,
}

pub struct CratesIoClient {
    client: Client,
}

impl CratesIoClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(format!("nix-updater/{}", env!("CARGO_PKG_VERSION")))
                .build()?,
        })
    }

    pub fn crate_info(&self, name: &str) -> Result<Option<CrateResponse>> {
        let url = format!("https://crates.io/api/v1/crates/{name}");

        match self.client.get(&url).send() {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(Some(response.json()?))
                } else if response.status().as_u16() == 404 {
                    Ok(None)
                } else {
                    anyhow::bail!("crates.io API returned status: {}", response.status())
                }
            }
            Err(e) => anyhow::bail!("Failed to fetch crates.io data: {e}"),
        }
    }
}
