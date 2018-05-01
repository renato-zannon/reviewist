use std::env;
use std::time::{Duration, SystemTime};
use reqwest::header::{Authorization, Headers};
use reqwest::unstable::async::Client;
use failure::Error;
use tokio_core::reactor::Handle;
use notification::PullRequest;
use futures::prelude::*;
use futures::{future, stream};
use slog::Logger;

#[derive(Clone)]
pub struct TodoistClient {
    http: Client,
    logger: Logger,
}

#[derive(Serialize)]
struct NewTask {
    content: String,
    due_string: String,
}

const NEW_TASK_URL: &'static str = "https://beta.todoist.com/API/v8/tasks";

impl TodoistClient {
    pub fn new(handle: &Handle, logger: Logger) -> Result<TodoistClient, Error> {
        let todoist_token = env::var("TODOIST_TOKEN")?;
        let client = Client::builder()
            .default_headers(default_headers(todoist_token))
            .timeout(Duration::from_secs(30))
            .build(handle)?;

        Ok(TodoistClient {
            http: client,
            logger,
        })
    }

    pub fn create_task_for_pr(&self, pr: &PullRequest) -> impl Future<Item = (), Error = Error> {
        let new_task = NewTask::for_pull_request(pr);

        self.http
            .post(NEW_TASK_URL)
            .json(&new_task)
            .send()
            .map_err(Error::from)
            .map(|_| ())
    }
}

impl NewTask {
    fn for_pull_request(pr: &PullRequest) -> NewTask {
        let content = format!(
            "{url} ({project}#{number}: {title})",
            url = pr.html_url,
            project = pr.repo(),
            number = pr.number,
            title = pr.title
        );

        NewTask {
            content,
            due_string: "today".to_string(),
        }
    }
}

fn default_headers(todoist_token: String) -> Headers {
    let mut headers = Headers::new();
    let auth_header = Authorization(format!("Bearer {}", todoist_token));
    headers.set(auth_header);

    headers
}
