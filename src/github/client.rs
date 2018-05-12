use std::cell::Cell;
use std::env;
use std::time::{Duration, Instant, SystemTime};

use failure::Error;
use futures::future::Either;
use futures::prelude::*;
use futures::{future, stream};
use reqwest::header::{self, Authorization, Headers};
use reqwest::unstable::async::Client;
use slog::Logger;
use tokio_timer::Delay;
use url::Url;

use github::notification::{PullRequest, ReviewRequest};
use github::notifications_polling;
use github::notifications_response::{self, NotificationsResponse};

use Config;

#[derive(Clone)]
pub struct GithubClient {
    http: Client,
    last_poll_interval: Cell<Option<u64>>,
    notifications_last_modified: Cell<header::HttpDate>,
    logger: Logger,
    host: Url,
}

pub fn new(config: &Config) -> Result<GithubClient, Error> {
    let github_token = env::var("GITHUB_TOKEN")?;
    let client = Client::builder()
        .default_headers(default_headers(github_token))
        .timeout(Duration::from_secs(30))
        .build(&config.core.handle())?;

    let base_time = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 7);
    let base_time = header::HttpDate::from(base_time);

    Ok(GithubClient {
        http: client,
        last_poll_interval: Cell::new(None),
        notifications_last_modified: Cell::new(base_time),
        logger: config.logger.clone(),
        host: config.github_base.clone(),
    })
}

impl GithubClient {
    pub fn into_pull_request_stream(self) -> impl Stream<Item = (PullRequest, Logger), Error = Error> {
        let logger = self.logger.clone();
        notifications_polling::poll_notifications(self, logger)
    }

    pub fn next_review_requests(
        &self,
    ) -> impl Future<Item = (impl Stream<Item = PullRequest, Error = Error>, Self), Error = Error> {
        let pages_stream = self.current_notifications();

        let new_client = self.clone();
        let http = self.http.clone();
        let logger = self.logger.clone();

        pages_stream
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(move |(maybe_page, next_stream)| {
                let response = match maybe_page {
                    Some(page) => page,
                    None => {
                        error!(logger, "Response didn't have first page - unable to get metadata");
                        return future::err(format_err!("Response has 0 pages"));
                    }
                };

                if let Some(lm) = response.last_modified {
                    new_client.notifications_last_modified.set(lm);
                }

                if let Some(p) = response.poll_interval {
                    new_client.last_poll_interval.set(Some(p));
                }

                let complete_stream = stream::once(Ok(response))
                    .chain(next_stream)
                    .map(|response| stream::iter_ok(response.notifications))
                    .flatten()
                    .filter_map(ReviewRequest::from_notification);

                let pull_requests = notifications_to_pull_requests(http, complete_stream, logger.clone());

                future::ok((pull_requests, new_client))
            })
    }

    pub fn wait_poll_interval(&self) -> impl Future<Item = (), Error = Error> {
        let interval = match self.last_poll_interval.get() {
            Some(interval) => interval,
            None => return Either::A(future::ok(())),
        };

        let logger = self.logger.clone();

        debug!(logger, "Start polling wait interval"; "length" => interval);
        let interval_end = Instant::now() + Duration::from_secs(interval);
        let delay = Delay::new(interval_end).map_err(Error::from).inspect(move |_| {
            debug!(logger, "Finished polling wait interval"; "length" => interval);
        });

        Either::B(delay)
    }

    fn current_notifications(&self) -> impl Stream<Item = NotificationsResponse, Error = Error> {
        let notifications_url = self.host.join("notifications?all=true").unwrap();
        let url = Some(notifications_url.into_string());

        let last_modified = self.notifications_last_modified.get();
        let logger = self.logger.clone();
        let client = self.http.clone();

        stream::unfold(url, move |maybe_url| {
            let url = maybe_url?;

            let result = get_notifications_page(client.clone(), last_modified, url, logger.clone());

            let result = result.map(move |response| {
                let next_page = response.next_page.clone();
                (response, next_page)
            });

            Some(result)
        })
    }
}

pub fn notifications_to_pull_requests<S>(
    http: Client,
    reviews: S,
    logger: Logger,
) -> impl Stream<Item = PullRequest, Error = Error>
where
    S: Stream<Item = ReviewRequest, Error = Error>,
{
    reviews
        .map(move |review_request| {
            let logger = logger.clone();

            get_pr_for_review_request(http.clone(), review_request)
                .map(Some)
                .or_else(move |err| {
                    warn!(logger, "Problem getting pull request"; "error" => %err);
                    return future::ok(None);
                })
        })
        .buffer_unordered(10)
        .filter_map(|pr| pr)
}

fn get_pr_for_review_request(
    http: Client,
    review_request: ReviewRequest,
) -> impl Future<Item = PullRequest, Error = Error> {
    http.get(&review_request.url)
        .send()
        .and_then(|mut response| response.json::<PullRequest>())
        .map_err(Error::from)
}

fn default_headers(github_token: String) -> Headers {
    let mut headers = Headers::new();
    let auth_header = Authorization(format!("token {}", github_token));
    headers.set(auth_header);

    headers
}

fn get_notifications_page(
    client: Client,
    last_modified: header::HttpDate,
    page_url: String,
    logger: Logger,
) -> impl Future<Item = NotificationsResponse, Error = Error> {
    let logger = logger.new(o!("url" => page_url.to_string(), "last_modified" => last_modified.to_string()));
    debug!(logger, "Fetching notifications");

    let if_modified_since = header::IfModifiedSince(last_modified);

    let request = client.get(&page_url).header(if_modified_since).send();
    request
        .map_err(Error::from)
        .and_then(move |response| notifications_response::from_http(response, logger))
}
