use futures::prelude::*;
use futures::stream;
use failure::Error;
use tokio_retry::{self, Retry, strategy::ExponentialBackoff};
use slog::Logger;
use std::rc::Rc;
use std::cell::Cell;

use github::{GithubClient, PullRequest};

pub fn poll_notifications(
    client: GithubClient,
    logger: Logger,
) -> impl Stream<Item = (PullRequest, Logger), Error = Error> {
    let batch_number = Rc::new(Cell::new(0));

    let unfold_logger = logger.clone();
    let unfold_bn = batch_number.clone();

    stream::unfold(client, move |client| {
        unfold_bn.set(unfold_bn.get() + 1);
        let logger = unfold_logger.new(o!("batch_number" => unfold_bn.get()));

        let mut retry_number = 0;

        let retry_strategy = ExponentialBackoff::from_millis(10).take(5);

        let retry = Retry::spawn(retry_strategy, move || {
            retry_number += 1;
            let logger = logger.new(o!("retry_number" => retry_number));

            get_next_batch(&client, logger)
        });

        let future = retry.map_err(|err| match err {
            tokio_retry::Error::OperationError(e) => e,
            tokio_retry::Error::TimerError(e) => Error::from(e),
        });

        Some(future)
    }).map(move |batch| {
        let logger = logger.new(o!("batch_number" => batch_number.get()));

        batch.map(move |item| (item, logger.clone()))
    })
        .flatten()
}

fn get_next_batch(
    client: &GithubClient,
    logger: Logger,
) -> impl Future<Item = (impl Stream<Item = PullRequest, Error = Error>, GithubClient), Error = Error> {
    let next_review_requests = client.next_review_requests();

    let stream_logger = logger.clone();

    client
        .wait_poll_interval()
        .and_then(move |_| next_review_requests)
        .map(move |(stream, next_client)| {
            let stream = stream.inspect_err(move |err| {
                error!(stream_logger, "Error in notification stream"; "error" => %err);
            });

            (stream, next_client)
        })
        .map_err(move |err| {
            if is_http_incomplete(&err) {
                debug!(logger, "Got early HTTP EOF"; "error" => ?err);
            } else {
                error!(logger, "Error while preparing stream"; "error" => ?err);
            }

            return err;
        })
}

fn is_http_incomplete(err: &Error) -> bool {
    match downcast_to_hyper_error(err) {
        Some(::hyper::Error::Incomplete) => true,
        _ => false,
    }
}

fn downcast_to_hyper_error(err: &Error) -> Option<&::hyper::Error> {
    let reqwest_error = err.downcast_ref::<::reqwest::Error>()?;
    let error_ref = reqwest_error.get_ref()?;

    error_ref.downcast_ref::<::hyper::Error>()
}
