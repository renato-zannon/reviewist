use std::env;
use std::time::{Duration, SystemTime};
use std::cell::Cell;
use reqwest::header::{self, Authorization, Headers};
use reqwest::unstable::async::Client;
use failure::Error;
use tokio_core::reactor::Handle;
use notification::{Notification, PullRequest, ReviewRequest};
use futures::prelude::*;
use futures::{future, stream};
use notifications_response::NotificationsResponse;

pub struct GithubClient {
    http: Client,
    notifications_last_modified: Cell<header::HttpDate>,
}

impl GithubClient {
    pub fn new(handle: &Handle) -> Result<GithubClient, Error> {
        let github_token = env::var("GITHUB_TOKEN")?;
        let client = Client::builder()
            .default_headers(default_headers(github_token))
            .build(handle)?;

        let base_time = SystemTime::now() - Duration::from_secs(60 * 60 * 48);
        let base_time = header::HttpDate::from(base_time);

        Ok(GithubClient {
            http: client,
            notifications_last_modified: Cell::new(base_time),
        })
    }

    pub fn current_review_requests<'a>(
        &'a self,
    ) -> impl Stream<Item = ReviewRequest, Error = Error> + 'a {
        self.get_all_notifications()
            .filter_map(ReviewRequest::from_notification)
    }

    fn get_all_notifications<'a>(&'a self) -> impl Stream<Item = Notification, Error = Error> + 'a {
        let url = Some("https://api.github.com/notifications".to_owned());

        stream::unfold(url, move |maybe_url| {
            maybe_url.map(|url| self.unfold_page(url))
        }).map(stream::iter_ok)
            .flatten()
    }

    fn unfold_page<'a>(
        &'a self,
        page_url: String,
    ) -> Box<Future<Item = (Vec<Notification>, Option<String>), Error = Error> + 'a> {
        let result = self.get_notifications_page(page_url).map(|response| {
            let next_page = response.next_page.clone();
            (response.notifications, next_page)
        });

        Box::new(result)
    }

    fn get_notifications_page<'b>(
        &'b self,
        page_url: String,
    ) -> impl Future<Item = NotificationsResponse, Error = Error> + 'b {
        let last_modified = self.notifications_last_modified.get();
        let if_modified_since = header::IfModifiedSince(last_modified);

        let request = self.http.get(&page_url).header(if_modified_since).send();

        request
            .map_err(Error::from)
            .and_then(NotificationsResponse::from_response)
    }

    pub fn pull_requests_to_review<'a>(
        &'a self,
    ) -> impl Stream<Item = PullRequest, Error = Error> + 'a {
        self.current_review_requests()
            .map(move |review_request| {
                self.get_pr_for_review_request(review_request)
                    .map(Some)
                    .or_else(|err| {
                        eprintln!("Problem getting pull request: {:?}", err);
                        return future::ok(None);
                    })
            })
            .buffer_unordered(10)
            .filter_map(|pr| pr)
    }

    fn get_pr_for_review_request(
        &self,
        review_request: ReviewRequest,
    ) -> impl Future<Item = PullRequest, Error = Error> {
        self.http
            .get(&review_request.url)
            .send()
            .and_then(|mut response| response.json::<PullRequest>())
            .map_err(Error::from)
    }
}

fn default_headers(github_token: String) -> Headers {
    let mut headers = Headers::new();
    let auth_header = Authorization(format!("token {}", github_token));
    headers.set(auth_header);

    headers
}
