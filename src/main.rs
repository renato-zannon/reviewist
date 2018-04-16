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
    env_logger::init();
    openssl_probe::init_ssl_cert_env_vars();

    let err = match run() {
        Ok(_) => return,
        Err(err) => err,
    };

    if let Some(bt) = err.cause().backtrace() {
        eprintln!("{}", bt);
    } else {
        eprintln!("{:?}", err.cause());
    }
}

fn run() -> Result<(), Error> {
    dotenv().ok();

    let mut core = reactor::Core::new()?;
    let client = github_client::GithubClient::new(&core.handle())?;

    let future = {
        future::loop_fn(client, |client| {
            let get_batch = move || {
                let (next_client, current_batch) = client.poll_review_requests();

                client.wait_poll_interval().and_then(move |_| {
                    current_batch.inspect_err(|err| {
                        eprintln!("Error: {}", err);
                    }).for_each(|pull_request| {
                        println!("- {:?}", pull_request);
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
