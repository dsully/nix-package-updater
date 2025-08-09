use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PyPiProjectResponse {
    pub info: PyPiProjectInfo,
    pub releases: std::collections::HashMap<String, Vec<PyPiReleaseFile>>,
}

#[derive(Debug, Deserialize)]
pub struct PyPiProjectInfo {
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct PyPiReleaseFile {
    pub filename: String,
    pub url: String,
}

pub struct PyPiClient {
    client: Client,
}

impl PyPiClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(format!("nix-updater/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("Couldn't build a client for PyPi"),
        }
    }

    pub fn project(&self, name: &str) -> Result<Option<PyPiProjectResponse>> {
        let url = format!("https://pypi.org/pypi/{name}/json");

        match self.client.get(&url).send() {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(Some(response.json()?))
                } else if response.status().as_u16() == 404 {
                    Ok(None)
                } else {
                    anyhow::bail!("PyPI API returned status: {}", response.status())
                }
            }
            Err(e) => anyhow::bail!("Failed to fetch PyPI data: {e}"),
        }
    }
}
