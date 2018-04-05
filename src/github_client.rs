use std::env;
use reqwest::header::{self, Authorization, Headers};
use reqwest::unstable::async::{Client, Response};
use failure::Error;
use tokio_core::reactor::Handle;
use notification::{Notification, ReviewRequest, PullRequest};
use futures::prelude::*;
use futures::{future, stream};
use futures::future::Either;

pub struct GithubClient {
    http: Client,
}

impl GithubClient {
    pub fn new(handle: &Handle) -> Result<GithubClient, Error> {
        let github_token = env::var("GITHUB_TOKEN")?;
        let client = Client::builder()
            .default_headers(default_headers(github_token))
            .build(handle)?;

        Ok(GithubClient { http: client })
    }

    pub fn http_client(&self) -> &Client {
        &self.http
    }

    pub fn current_review_requests<'a>(&'a self) -> impl Stream<Item = ReviewRequest, Error = Error> + 'a {
        self.get_all_notifications().filter_map(ReviewRequest::from_notification)
    }

    fn get_all_notifications<'a>(&'a self) -> impl Stream<Item = Notification, Error = Error> + 'a {
        let url = Some("https://api.github.com/notifications".to_owned());

        stream::unfold(url, move |maybe_url| {
            maybe_url.map(|page_url| self.get_notifications_page(page_url))
        }).map(stream::iter_ok).flatten()
    }

    fn get_notifications_page<'b>(&'b self, page_url: String) -> impl Future<Item = (Vec<Notification>, Option<String>), Error = Error> + 'b {
        self.http.get(&page_url).send().and_then(|mut response| {
            let next_url = next_page_url(&response);

            response.json::<Vec<Notification>>().map(|notifications| {
                return (notifications, next_url);
            })
        }).map_err(Error::from)
    }

    pub fn pull_requests_to_review<'a>(&'a self) -> impl Stream<Item = PullRequest, Error = Error> + 'a {
        self.current_review_requests()
            .map(move |review_request| self.get_pr_for_review_request(review_request))
            .buffer_unordered(10)
    }

    fn get_pr_for_review_request(&self, review_request: ReviewRequest) -> impl Future<Item = PullRequest, Error = Error> {
        self.http.get(&review_request.url).send().and_then(|mut response| {
            response.json::<PullRequest>()
        }).map_err(Error::from)
    }
}

fn default_headers(github_token: String) -> Headers {
    let mut headers = Headers::new();
    let auth_header = Authorization(format!("token {}", github_token));
    headers.set(auth_header);

    headers
}

fn next_page_url(response: &Response) -> Option<String> {
    use reqwest::header::RelationType;

    let link_header: &header::Link = response.headers().get()?;

    let value = link_header.values().iter().find(|value| {
        match value.rel() {
            Some(rels) => rels.contains(&RelationType::Next),
            None => false,
        }
    });

    Some(value?.link().to_owned())
}
