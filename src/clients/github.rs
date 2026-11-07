use std::sync::OnceLock;

use figment::Figment;
use figment::providers::{Format, Toml};
use git_url_parse::GitUrl;
use git_url_parse::types::provider::GenericProvider;
use octocrab::Octocrab;
use reqwest::header::{ACCEPT, ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH};
use rootcause::{Result, report};
use serde::Deserialize;

use crate::clients::etag_cache::{Entry, EtagCache};
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
    cache: EtagCache,
}

static CLIENT: OnceLock<std::result::Result<GitHubClient, String>> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

impl GitHubClient {
    /// One client for the whole process: building it loads the native root certificates, which
    /// is slow and serialized on macOS when `SSL_CERT_FILE` is unset.
    pub fn shared() -> Result<&'static Self> {
        CLIENT.get_or_init(|| Self::new().map_err(|e| e.to_string())).as_ref().map_err(|e| report!("{e}"))
    }

    /// Persist the ETag cache if the shared client was used.
    pub fn save_cache() -> Result<()> {
        match CLIENT.get() {
            Some(Ok(client)) => client.cache.save(),
            _ => Ok(()),
        }
    }

    fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;

        let client = runtime.block_on(async {
            let mut builder = Octocrab::builder();

            // Avoid GitHub rate limits.
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                builder = builder.personal_token(token);
            }

            builder.build()
        })?;

        Ok(Self {
            client,
            runtime,
            cache: EtagCache::load(EtagCache::default_path()),
        })
    }

    fn owner_and_repo_from_url(url: &GitUrl) -> Result<(String, String)> {
        let provider: GenericProvider = url.provider_info()?;

        Ok((provider.owner().clone(), provider.repo().clone()))
    }

    /// GET `route`, revalidating the body cached under `key` with `If-None-Match`. 404 and 409 (empty repository) are `None`.
    async fn get_cached(&self, route: String, key: String, accept: Option<&'static str>) -> Result<Option<String>> {
        let cached = self.cache.get(&key);

        let mut headers = HeaderMap::new();
        if let Some(accept) = accept {
            headers.insert(ACCEPT, HeaderValue::from_static(accept));
        }
        if let Some(etag) = cached.as_ref().and_then(|entry| HeaderValue::from_str(&entry.etag).ok()) {
            headers.insert(IF_NONE_MATCH, etag);
        }

        let response = self.client._get_with_headers(route, Some(headers)).await?;

        match response.status().as_u16() {
            304 if let Some(entry) = cached => Ok(Some(entry.body)),
            200..=299 => {
                let etag = response.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
                let body = self.client.body_to_string(response).await?;

                if let Some(etag) = etag {
                    self.cache.insert(key, Entry { etag, body: body.clone() });
                }

                Ok(Some(body))
            }
            404 | 409 => Ok(None),
            _ => match octocrab::map_github_error(response).await {
                Err(octocrab::Error::GitHub { source, .. }) => Err(report!("GitHub {}: {}", source.status_code, source.message)),
                other => other.map(|_| None).map_err(Into::into),
            },
        }
    }

    pub fn latest_release(&self, url: &GitUrl) -> Result<Option<String>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        self.runtime.block_on(async {
            let route = format!("/repos/{owner}/{repo}/releases/latest");
            let Some(body) = self.get_cached(route.clone(), route, None).await? else {
                return Ok(None);
            };

            Ok(Some(serde_json::from_str::<Release>(&body)?.tag_name))
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
        let route = format!("/repos/{owner}/{repo}/commits/HEAD");

        self.runtime.block_on(async {
            Ok(self
                .get_cached(route.clone(), route, Some("application/vnd.github.sha"))
                .await?
                .map(|sha| sha.trim().to_string()))
        })
    }

    /// Get version from Cargo.toml at a specific commit
    pub fn cargo_version(&self, url: &GitUrl, commit: &str) -> Result<Option<String>> {
        let (owner, repo) = Self::owner_and_repo_from_url(url)?;

        let route = format!("/repos/{owner}/{repo}/contents/Cargo.toml?ref={commit}");
        // One entry per repository, not per commit, so the cache stays bounded.
        let key = format!("/repos/{owner}/{repo}/contents/Cargo.toml");

        self.runtime.block_on(async {
            let Some(manifest) = self.get_cached(route, key, Some("application/vnd.github.raw")).await? else {
                return Ok(None);
            };

            Ok(Figment::from(Toml::string(&manifest)).extract::<CargoToml>().ok().map(|toml| toml.package.version))
        })
    }
}

#[cfg(test)]
mod tests {
    use figment::Figment;
    use figment::providers::{Format, Toml};

    use super::CargoToml;

    #[test]
    fn parses_package_version_from_cargo_toml() {
        let manifest = "[package]\nname = \"x\"\nversion = \"1.2.3\"\nedition = \"2024\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n\n[profile.release]\nlto = true\n";

        let parsed: CargoToml = Figment::from(Toml::string(manifest)).extract().unwrap();

        assert_eq!(parsed.package.version, "1.2.3");
    }
}
