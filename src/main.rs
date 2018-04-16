extern crate dotenv;
#[macro_use]
extern crate failure;
extern crate futures;
#[macro_use]
extern crate hyper;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_core;
extern crate tokio_timer;
extern crate env_logger;
extern crate openssl_probe;

mod github_client;
mod notification;
mod notification_stream;

use dotenv::dotenv;
use failure::Error;

use tokio_core::reactor;
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
    dotenv().map_err(|err| format_err!(".env error: {:?}", err))?;

    let mut core = reactor::Core::new()?;
    let client = github_client::GithubClient::new(&core.handle())?;

    let future = {
        future::loop_fn(client, |client| {
            let (next_client, current_batch) = client.poll_review_requests();

            let get_batch = client
                .wait_poll_interval()
                .and_then(move |_| {
                    current_batch.for_each(|pull_request| {
                        println!("- {:?}", pull_request);
                        future::ok(())
                    })
                });

            get_batch
                .and_then(move |_| next_client)
                .and_then(move |client| future::ok(future::Loop::Continue(client)))
        })
    };

    core.run(future)
}
