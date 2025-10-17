use anyhow::Result;
use reqwest::blocking::Client;

pub struct NpmClient {
    client: Client,
}

impl NpmClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(format!("nix-updater/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("Couldn't build a client for npm"),
        }
    }

    pub fn download_package_lock(&self, url: &str) -> Result<Option<String>> {
        match self.client.get(url).send() {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(Some(response.text()?))
                } else if response.status().as_u16() == 404 {
                    Ok(None)
                } else {
                    anyhow::bail!("Failed to download package-lock.json: status {}", response.status())
                }
            }
            Err(e) => anyhow::bail!("Failed to download package-lock.json: {e}"),
        }
    }
}
