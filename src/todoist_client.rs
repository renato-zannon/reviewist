use std::env;
use std::time::Duration;
use reqwest::header::{Authorization, Headers};
use reqwest::unstable::async::Client;
use failure::Error;
use futures::prelude::*;
use slog::Logger;
use url::Url;

use github::PullRequest;
use Config;

#[derive(Clone)]
pub struct TodoistClient {
    http: Client,
    logger: Logger,
    host: Url,
}

#[derive(Serialize)]
struct NewTask {
    content: String,
    due_string: String,
}

impl TodoistClient {
    pub fn new(config: &Config) -> Result<TodoistClient, Error> {
        let todoist_token = env::var("TODOIST_TOKEN")?;
        let client = Client::builder()
            .default_headers(default_headers(todoist_token))
            .timeout(Duration::from_secs(30))
            .build(&config.core.handle())?;

        Ok(TodoistClient {
            http: client,
            host: config.todoist_base.clone(),
            logger: config.logger.clone(),
        })
    }

    pub fn create_task_for_pr(&self, pr: &PullRequest) -> impl Future<Item = (), Error = Error> {
        let new_task = NewTask::for_pull_request(pr);
        let new_task_url = self.host.join("/API/v8/tasks").unwrap();

        self.http
            .post(new_task_url)
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
