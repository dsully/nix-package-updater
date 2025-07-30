use anyhow::Result;
use octocrab::Octocrab;

// GitHub client using octocrab
pub struct GitHubClient {
    client: Octocrab,
    runtime: tokio::runtime::Runtime,
}

impl GitHubClient {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;

        let client = runtime.block_on(async {
            let mut builder = Octocrab::builder();

            // Check for GitHub token in environment
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                builder = builder.personal_token(token);
            }

            builder.build()
        })?;

        Ok(Self { client, runtime })
    }

    pub fn get_latest_release(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        self.runtime.block_on(async {
            match self.client.repos(owner, repo).releases().get_latest().await {
                Ok(release) => Ok(Some(release.tag_name)),
                Err(octocrab::Error::GitHub { source, .. }) if source.status_code == 404 => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    pub fn get_latest_commit(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        self.runtime.block_on(async {
            match self
                .client
                .repos(owner, repo)
                .get_ref(&octocrab::params::repos::Reference::Branch("HEAD".to_string()))
                .await
            {
                Ok(git_ref) => {
                    // The object field contains the SHA in a different structure
                    // For octocrab 0.44, we need to match on the object type
                    match &git_ref.object {
                        octocrab::models::repos::Object::Commit { sha, .. } => Ok(Some(sha.clone())),
                        _ => Ok(None),
                    }
                }
                Err(octocrab::Error::GitHub { source, .. }) if source.status_code == 404 => {
                    // Try main branch
                    match self
                        .client
                        .repos(owner, repo)
                        .get_ref(&octocrab::params::repos::Reference::Branch("main".to_string()))
                        .await
                    {
                        Ok(git_ref) => {
                            // The object field contains the SHA in a different structure
                            // For octocrab 0.44, we need to match on the object type
                            match &git_ref.object {
                                octocrab::models::repos::Object::Commit { sha, .. } => Ok(Some(sha.clone())),
                                _ => Ok(None),
                            }
                        }
                        Err(_) => {
                            // Try master branch
                            match self
                                .client
                                .repos(owner, repo)
                                .get_ref(&octocrab::params::repos::Reference::Branch("master".to_string()))
                                .await
                            {
                                Ok(git_ref) => {
                                    // The object field contains the SHA in a different structure
                                    // For octocrab 0.44, we need to match on the object type
                                    match &git_ref.object {
                                        octocrab::models::repos::Object::Commit { sha, .. } => Ok(Some(sha.clone())),
                                        _ => Ok(None),
                                    }
                                }
                                Err(_) => Ok(None),
                            }
                        }
                    }
                }
                Err(e) => Err(e.into()),
            }
        })
    }
}
