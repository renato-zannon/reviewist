use failure::Error;

#[derive(Debug, Deserialize)]
pub struct Notification {
    pub reason: String,
    pub url: String,
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

#[derive(Debug, Deserialize)]
pub struct Repository {
    pub name: String,
}

#[derive(Debug)]
pub struct ReviewRequest {
    pub pr_title: String,
    pub repository: String,
    pub url: String,
}

#[derive(Deserialize, Debug)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub html_url: String,
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
