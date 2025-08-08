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

    pub fn latest_release(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        self.runtime.block_on(async {
            match self.client.repos(owner, repo).releases().get_latest().await {
                Ok(release) => Ok(Some(release.tag_name)),
                Err(octocrab::Error::GitHub { source, .. }) if source.status_code == 404 => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    #[allow(dead_code)]
    pub fn latest_tag(&self, owner: &str, repo: &str) -> Result<Option<(String, String)>> {
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

    pub fn latest_commit(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        self.runtime.block_on(async {
            // First try to get the default branch
            if let Ok(repo_info) = self.client.repos(owner, repo).get().await {
                let default_branch = repo_info.default_branch.as_deref().unwrap_or("main");

                // Get the commit SHA for the default branch
                match self
                    .client
                    .repos(owner, repo)
                    .get_ref(&octocrab::params::repos::Reference::Branch(default_branch.to_string()))
                    .await
                {
                    Ok(git_ref) => match &git_ref.object {
                        octocrab::models::repos::Object::Commit { sha, .. } => Ok(Some(sha.clone())),
                        _ => Ok(None),
                    },
                    Err(_) => Ok(None),
                }
            } else {
                // Fallback: try common branch names
                for branch in &["main", "master"] {
                    let Ok(git_ref) = self
                        .client
                        .repos(owner, repo)
                        .get_ref(&octocrab::params::repos::Reference::Branch((*branch).to_string()))
                        .await
                    else {
                        continue;
                    };

                    if let octocrab::models::repos::Object::Commit { sha, .. } = &git_ref.object {
                        return Ok(Some(sha.clone()));
                    }
                }
                Ok(None)
            }
        })
    }
}
