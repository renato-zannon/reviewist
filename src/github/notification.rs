use chrono::prelude::*;

#[derive(Debug, Deserialize)]
pub struct Notification {
    pub reason: String,
    pub subject: Subject,
    pub repository: Repository,
}

#[derive(Debug, Deserialize)]
pub struct Subject {
    #[serde(rename = "type")]
    pub _type: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Repository {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ReviewRequest {
    pub pr_title: String,
    pub repository: String,
    pub url: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub html_url: String,

    pub created_at: DateTime<Local>,
    pub merged_at: Option<DateTime<Local>>,
    pub closed_at: Option<DateTime<Local>>,

    base: PullRequestBase,
}

#[derive(Deserialize, Debug, Clone)]
struct PullRequestBase {
    repo: Repository,
}

impl PullRequest {
    pub fn is_open(&self) -> bool {
        self.merged_at.is_none() && self.closed_at.is_none()
    }

    pub fn repo(&self) -> &str {
        &self.base.repo.name
    }
}

impl ReviewRequest {
    pub fn from_notification(n: Notification) -> Option<ReviewRequest> {
        if n.reason != "review_requested" || n.subject._type != "PullRequest" {
            return None;
        }

        Some(ReviewRequest {
            pr_title: n.subject.title,
            repository: n.repository.name,
            url: n.subject.url,
        })
    }
}
