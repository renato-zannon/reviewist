extern crate dotenv;
extern crate env_logger;
#[macro_use]
extern crate failure;
extern crate futures;
#[macro_use]
extern crate hyper;
extern crate openssl_probe;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_core;
extern crate tokio_retry;
extern crate tokio_timer;

#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

mod github_client;
mod notification;
mod notification_stream;

use dotenv::dotenv;
use failure::Error;

use tokio_core::reactor;
use tokio_retry::{Retry, strategy::ExponentialBackoff};
use futures::prelude::*;
use futures::future;

fn main() {
    let logger = configure_slog();
    env_logger::init();
    openssl_probe::init_ssl_cert_env_vars();

    let err = match run(logger.clone()) {
        Ok(_) => return,
        Err(err) => err,
    };

    if let Some(bt) = err.cause().backtrace() {
        error!(logger, "backtrace" => bt);
    } else {
        error!(logger, "cause" => ?err.cause());
    }
}

fn run(logger: slog::Logger) -> Result<(), Error> {
    dotenv().ok();

    let mut core = reactor::Core::new()?;
    let client = github_client::GithubClient::new(&core.handle())?;

    let future = {
        let mut batch_number = 0;
        let logger = &logger;

        future::loop_fn(client, move |client| {
            batch_number += 1;

            let get_batch = move || {
                let (next_client, current_batch) = client.poll_review_requests();
                let batch_logger = logger.new(o!("batch_number" => batch_number));

                client.wait_poll_interval().and_then(move |_| {
                    let err_batch_logger = batch_logger.clone();

                    current_batch.inspect_err(move |err| {
                        error!(err_batch_logger, "Error in PullRequest batch"; "error" => %err);
                    }).for_each(move |pull_request| {
                        info!(batch_logger, "PR received"; "pull_request" => ?pull_request);
                        future::ok(())
                    })
                }).and_then(move |_| next_client)
            };

            let retry_strategy = ExponentialBackoff::from_millis(10).take(5);
            let future = Retry::spawn(retry_strategy, get_batch).map_err(|err| {
                match err {
                    tokio_retry::Error::OperationError(e) => e,
                    tokio_retry::Error::TimerError(e) => Error::from(e)
                }
            });

            future
                .and_then(move |client| future::ok(future::Loop::Continue(client)))
        })
    };

    core.run(future)
}

fn configure_slog() -> slog::Logger {
    use slog::Drain;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
}
