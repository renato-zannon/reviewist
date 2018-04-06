extern crate dotenv;
#[macro_use]
extern crate failure;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde;
extern crate tokio_core;
extern crate tokio_timer;
extern crate futures;
#[macro_use]
extern crate hyper;

mod notification;
mod github_client;

use dotenv::dotenv;
use failure::Error;

use notification::{Notification, ReviewRequest};
use tokio_core::reactor;
use futures::prelude::*;
use futures::future;

fn main() {
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

    let reviews = client.pull_requests_to_review().for_each(|pull_request| {
        println!("- {:?}", pull_request);
        future::ok(())
    });

    core.run(reviews)
}
