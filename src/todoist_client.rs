use failure::Error;
use futures::future;
use futures::prelude::*;
use reqwest::header::{Authorization, Headers};
use reqwest::unstable::async::Client;
use slog::Logger;
use std::env;
use std::time::Duration;
use url::Url;

use Config;
use github::PullRequest;

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
        let logger = self.logger.clone();

        let request = self.http.post(new_task_url).json(&new_task).send();
        request.then(move |response| match response {
            Ok(ok_response) => {
                if ok_response.status().is_success() {
                    return future::ok(());
                }

                error!(logger, "Error while creating todoist task"; "response" => ?ok_response);
                return future::err(format_err!(
                    "Error while creating todoist task. response: {:?}",
                    ok_response
                ));
            }

            Err(err) => {
                let err = Error::from(err);
                error!(logger, "Error while creating todoist task"; "error" => %err);
                return future::err(err);
            }
        })
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
