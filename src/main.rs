extern crate chrono;
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
extern crate tokio;
extern crate tokio_core;
extern crate tokio_retry;
extern crate tokio_threadpool;
extern crate tokio_timer;

#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

#[macro_use]
extern crate diesel;

mod github_client;
mod todoist_client;
mod notification;
mod notifications_response;
mod notifications_polling;
mod review_handler;
mod schema;

use dotenv::dotenv;
use failure::Error;

use tokio_core::reactor;
use futures::prelude::*;
use futures::future::{self, Either};

fn main() {
    let logger = configure_slog();
    env_logger::init();
    openssl_probe::init_ssl_cert_env_vars();

    let err = match run(logger.clone()) {
        Ok(_) => return,
        Err(err) => err,
    };

    if let Some(bt) = err.cause().backtrace() {
        error!(logger, "critical error"; "backtrace" => %bt);
    } else {
        error!(logger, "critical error"; "cause" => ?err.cause());
    }
}

fn run(logger: slog::Logger) -> Result<(), Error> {
    dotenv().ok();

    let mut core = reactor::Core::new()?;
    let github_client = github_client::GithubClient::new(&core.handle(), logger.clone())?;
    let todoist_client = todoist_client::TodoistClient::new(&core.handle(), logger.clone())?;

    let handler = review_handler::new()?;

    let f = github_client
        .into_notifications_polling()
        .for_each(move |(pull_request, logger)| {
            let record_logger = logger.new(o!("pull_request" => pull_request.number));

            if !pull_request.is_open() {
                debug!(record_logger, "Skipping closed pull request");
                return Either::A(future::ok(()));
            }

            let todoist_client = todoist_client.clone();
            let result = handler
                .record_in_task(pull_request, record_logger)
                .and_then(move |maybe_pr| match maybe_pr {
                    Some(pr) => Either::A(todoist_client.create_task_for_pr(&pr)),

                    None => Either::B(future::ok(())),
                });

            Either::B(result)
        });

    core.run(f)
}

fn configure_slog() -> slog::Logger {
    use slog::Drain;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
}
