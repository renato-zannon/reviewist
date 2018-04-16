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
use futures::{future, stream, sync::oneshot};
use futures::future::Either;
use notification_stream::NotificationStream;

#[derive(Clone)]
pub struct GithubClient {
    http: Client,
    last_poll_interval: Cell<Option<u64>>,
    notifications_last_modified: Cell<header::HttpDate>,
}

impl GithubClient {
    pub fn new(handle: &Handle) -> Result<GithubClient, Error> {
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
        })
    }

    pub fn poll_review_requests(
        &self,
    ) -> (
        impl Future<Item = Self, Error = Error>,
        impl Stream<Item = PullRequest, Error = Error>,
    ) {
        let pages_stream =
            NotificationStream::new(self.http.clone(), self.notifications_last_modified.get());

        let (sender, receiver) = oneshot::channel();

        let new_client = self.clone();
        let mut new_client_init = Some((sender, new_client));

        let stream = pages_stream
            .map(move |response| {
                match new_client_init.take() {
                    Some((sender, new_client)) => {
                        if let Some(lm) = response.last_modified {
                            new_client.notifications_last_modified.set(lm);
                        }

                        if let Some(p) = response.poll_interval {
                            new_client.last_poll_interval.set(Some(p));
                        }

                        sender.send(new_client).ok();
                    }

                    None => {}
                }

                stream::iter_ok(response.notifications)
            })
            .flatten()
            .filter_map(ReviewRequest::from_notification);

        let pull_requests = notifications_to_pull_requests(self.http.clone(), stream);

        (
            receiver.map_err(Error::from),
            pull_requests,
        )
    }

    pub fn wait_poll_interval(&self) -> impl Future<Item = (), Error = Error> {
        let interval = match self.last_poll_interval.get() {
            Some(interval) => interval,
            None => return Either::A(future::ok(())),
        };

        self.last_poll_interval.set(None);

        eprintln!("Will wait {}s", interval);
        let interval_end = Instant::now() + Duration::from_secs(interval);
        let delay = Delay::new(interval_end).map_err(Error::from);
        Either::B(delay)
    }
}

pub fn notifications_to_pull_requests<S>(
    http: Client,
    reviews: S,
) -> impl Stream<Item = PullRequest, Error = Error>
where
    S: Stream<Item = ReviewRequest, Error = Error>,
{
    reviews
        .map(move |review_request| {
            get_pr_for_review_request(http.clone(), review_request)
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
