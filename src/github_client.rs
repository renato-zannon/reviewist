use std::env;
use reqwest::header::{self, Authorization, Headers};
use reqwest::unstable::async::Client;
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

    pub fn current_review_requests(&self) -> impl Stream<Item = ReviewRequest, Error = Error> {
        self.http.get("https://api.github.com/notifications").send().and_then(|mut response| {
            response.json::<Vec<Notification>>()
        }).map(|notifications| {
            let requests = notifications.into_iter().filter_map(ReviewRequest::from_notification);
            stream::iter_ok(requests)
        }).flatten_stream().map_err(Error::from)
    }

    fn get_all_notifications<'a>(&'a self) -> impl Stream<Item = Notification, Error = Error> + 'a {
        let url = Some("https://api.github.com/notifications".to_owned());

        fn make_request<'b>(http: &'b Client) -> impl Future<Item = (Vec<Notification>, Option<String>), Error = Error> + 'b {
            use reqwest::header::RelationType;

            http.get(&page_url).send().and_then(|mut response| {
                let link_header: Option<&header::Link> = response.headers().get();
                let next_url: Option<String> = link_header.and_then(|header| {
                    for value in header.values() {
                        let includes = value.rel().map(|rels| rels.contains(&RelationType::Next));

                        match includes {
                            Some(true) => return Some(value.link().to_owned()),
                            _ => continue,
                        }
                    }

                    return None;
                });

                response.json::<Vec<Notification>>().map(|notifications| {
                    return (notifications, next_url);
                })
            }).map_err(Error::from)
        }

        stream::unfold(url, |maybe_url| {
            let page_url = match maybe_url {
                Some(url) => url,
                None => return None,
            };

            let response = make_request(&self.http);
            return Some(response);
        }).and_then(stream::iter_ok)
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
