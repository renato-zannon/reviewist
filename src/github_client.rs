use std::env;
use std::time::{Duration, SystemTime};
use std::cell::Cell;
use reqwest::header::{self, Authorization, Headers};
use reqwest::unstable::async::{Client, Response};
use failure::Error;
use tokio_core::reactor::Handle;
use notification::{Notification, ReviewRequest, PullRequest};
use futures::prelude::*;
use futures::{future, stream};
use futures::future::Either;
use serde_json::{self, Value};

pub struct GithubClient {
    http: Client,
    notifications_last_modified: Cell<header::HttpDate>,
}

struct NotificationsResponse {
    notifications: Vec<Notification>,
    next_page: Option<String>,
    last_modified: header::HttpDate,
    poll_interval: Option<i64>,
}

header! { (XPollInterval, "X-Poll-Interval") => [i64] }

impl NotificationsResponse {
    fn from_response(mut response: Response) -> impl Future<Item = NotificationsResponse, Error = Error> {
        let last_modified = response.headers().get::<header::LastModified>().map(|header| header.0);
        let last_modified = match last_modified {
            Some(date) => date,
            None => return Either::A(future::err(format_err!("Response didn't contain Last-Modified header"))),
        };

        let poll_interval = response.headers().get::<XPollInterval>().cloned().map(|int| int.0);
        let next_page = next_page_url(&response);

        let result = response.json::<Vec<Value>>().map(move |objects| {
            let notifications = objects.into_iter().filter_map(|object| {
                match serde_json::from_value(object) {
                    Ok(notification) => Some(notification),

                    Err(err) => {
                        eprintln!("Problem parsing notification: {:?}", err);
                        return None;
                    },
                }
            }).collect();

            NotificationsResponse {
                notifications,
                next_page,
                last_modified,
                poll_interval
            }
        }).map_err(Error::from);

        Either::B(result)
    }
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
        let last_modified = self.notifications_last_modified.get();
        let if_modified_since = header::IfModifiedSince(last_modified);

        let request = self.http
            .get(&page_url)
            .header(if_modified_since)
            .send();

        request.and_then(|mut response| {
            let next_url = next_page_url(&response);

            response.json::<Vec<Value>>().map(|objects| {
                let notifications = objects.into_iter().filter_map(|object| {
                    match serde_json::from_value(object) {
                        Ok(notification) => Some(notification),

                        Err(err) => {
                            eprintln!("Problem parsing notification: {:?}", err);
                            return None;
                        },
                    }
                }).collect();

                return (notifications, next_url);
            })
        }).map_err(Error::from)
    }

    pub fn pull_requests_to_review<'a>(&'a self) -> impl Stream<Item = PullRequest, Error = Error> + 'a {
        self.current_review_requests()
            .map(move |review_request| {
                self.get_pr_for_review_request(review_request).map(Some).or_else(|err| {
                    eprintln!("Problem getting pull request: {:?}", err);
                    return future::ok(None);
                })
            })
            .buffer_unordered(10)
            .filter_map(|pr| pr)
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
