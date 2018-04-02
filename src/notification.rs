use failure::Error;

#[derive(Debug, Deserialize)]
pub struct Notification {
    reason: String,
    url: String,
    subject: Subject,
    repository: Repository,
}

#[derive(Debug, Deserialize)]
struct Subject {
    #[serde(rename = "type")]
    _type: String,
    title: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct Repository {
    name: String,
}

#[derive(Debug)]
pub struct ReviewRequest {
    pr_title: String,
    pr_number: String,
    repository: String,
    url: String,
}

impl ReviewRequest {
    pub fn from_notification(n: Notification) -> Option<ReviewRequest> {
        if n.reason != "review_requested" || n.subject._type != "PullRequest" {
            return None;
        }

        Some(ReviewRequest {
            pr_title: n.subject.title,
            pr_number: "lol123".to_owned(),
            repository: n.repository.name,
            url: n.subject.url,
        })
    }
}
