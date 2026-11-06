use git_url_parse::GitUrl;
use git_url_parse::types::provider::GenericProvider;
use octocrab::Octocrab;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use rootcause::Result;
use serde::Deserialize;


#[derive(Debug, Deserialize)]
struct CargoToml {
    package: CargoPackage,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    version: String,
}

pub struct GitHubClient {
    client: Octocrab,
    runtime: tokio::runtime::Runtime,
}

impl GitHubClient {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;

        let client = runtime.block_on(async {
            let mut builder = Octocrab::builder();

            // Avoid GitHub rate limits.
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                builder = builder.personal_token(token);
            }

            builder.build()
        })?;

        Ok(Self { client, runtime })
    }

    fn owner_and_repo_from_url(url: &GitUrl) -> Result<(String, String)> {
        let provider: GenericProvider = url.provider_info()?;

        Ok((provider.owner().clone(), provider.repo().clone()))
    }

    pub fn latest_release(&self, url: &GitUrl) -> Result<Option<String>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        self.runtime.block_on(async {
            match self.client.repos(owner, repo).releases().get_latest().await {
                Ok(release) => Ok(Some(release.tag_name)),
                Err(octocrab::Error::GitHub { source, .. }) if source.status_code == 404 => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Download a release asset through the API, which also works for private repositories.
    pub fn release_asset(&self, url: &GitUrl, tag: &str, name: &str) -> Result<Option<Vec<u8>>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        let release = self.runtime.block_on(self.client.repos(owner, repo).releases().get_by_tag(tag))?;

        let Some(asset) = release.assets.into_iter().find(|asset| asset.name == name) else {
            return Ok(None);
        };

        // Not octocrab: reqwest strips Authorization on the cross-host redirect to the signed asset URL.
        let mut request = reqwest::blocking::Client::builder()
            .user_agent(format!("nix-updater/{}", env!("CARGO_PKG_VERSION")))
            .build()?
            .get(asset.url.as_str())
            .header(reqwest::header::ACCEPT, "application/octet-stream");

        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request.bearer_auth(token);
        }

        Ok(Some(request.send()?.error_for_status()?.bytes()?.to_vec()))
    }

    #[allow(dead_code)]
    pub fn latest_tag(&self, url: &GitUrl) -> Result<Option<(String, String)>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        self.runtime.block_on(async {
            // Get all tags sorted by commit date
            let tags = self.client.repos(owner, repo).list_tags().send().await?;

            if let Some(tag) = tags.items.first() {
                // Return both tag name and commit SHA
                Ok(Some((tag.name.clone(), tag.commit.sha.clone())))
            } else {
                Ok(None)
            }
        })
    }

    pub fn latest_commit(&self, url: &GitUrl) -> Result<Option<String>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        // `HEAD` resolves to the default branch; the sha media type returns only the commit id.
        let headers = HeaderMap::from_iter([(ACCEPT, HeaderValue::from_static("application/vnd.github.sha"))]);

        self.runtime.block_on(async {
            match self.client._get_with_headers(format!("/repos/{owner}/{repo}/commits/HEAD"), Some(headers)).await {
                Ok(response) if response.status().is_success() => Ok(Some(self.client.body_to_string(response).await?.trim().to_string())),
                _ => Ok(None),
            }
        })
    }

    /// Get version from Cargo.toml at a specific commit
    pub fn cargo_version(&self, url: &GitUrl, commit: &str) -> Result<Option<String>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        self.runtime.block_on(async {
            match self.client.repos(&owner, &repo).get_content().path("Cargo.toml").r#ref(commit).send().await {
                Ok(content) => {
                    if let Some(item) = content.items.first()
                        && let Some(decoded) = item.decoded_content()
                        && let Ok(cargo_toml) = toml::from_str::<CargoToml>(&decoded)
                    {
                        return Ok(Some(cargo_toml.package.version));
                    }
                    Ok(None)
                }
                Err(_) => Ok(None),
            }
        })
    }
}
