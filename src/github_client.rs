use std::env;
use std::time::{Duration, Instant, SystemTime};
use std::cell::Cell;
use reqwest::header::{self, Authorization, Headers};
use reqwest::unstable::async::Client;
use failure::Error;
use tokio_core::reactor::Handle;
use tokio_timer::Delay;
use notification::{PullRequest, ReviewRequest};
use futures::prelude::*;
use futures::{future, stream};
use futures::future::Either;
use notification_stream;
use slog::Logger;

#[derive(Clone)]
pub struct GithubClient {
    http: Client,
    last_poll_interval: Cell<Option<u64>>,
    notifications_last_modified: Cell<header::HttpDate>,
    logger: Logger,
}

impl GithubClient {
    pub fn new(handle: &Handle, logger: Logger) -> Result<GithubClient, Error> {
        let github_token = env::var("GITHUB_TOKEN")?;
        let client = Client::builder()
            .default_headers(default_headers(github_token))
            .timeout(Duration::from_secs(30))
            .build(handle)?;

        let base_time = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 7);
        let base_time = header::HttpDate::from(base_time);

        Ok(GithubClient {
            http: client,
            last_poll_interval: Cell::new(None),
            notifications_last_modified: Cell::new(base_time),
            logger,
        })
    }

    pub fn next_review_requests(
        &self,
    ) -> impl Future<Item = (impl Stream<Item = PullRequest, Error = Error>, Self), Error = Error> {
        let pages_stream = notification_stream::new(
            self.http.clone(),
            self.notifications_last_modified.get(),
            self.logger.clone(),
        );

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
                        error!(
                            logger,
                            "Response didn't have first page - unable to get metadata"
                        );
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
        let delay = Delay::new(interval_end)
            .map_err(Error::from)
            .inspect(move |_| {
                debug!(logger, "Finished polling wait interval"; "length" => interval);
            });

        Either::B(delay)
    }

    pub fn clear_poll_interval(&self) {
        self.last_poll_interval.set(None);
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
