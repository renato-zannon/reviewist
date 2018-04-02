extern crate dotenv;
#[macro_use]
extern crate failure;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde;

mod notification;

use std::env;
use dotenv::dotenv;
use failure::Error;
use reqwest::header;

use notification::{Notification, ReviewRequest};

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
    let github_token = env::var("GITHUB_TOKEN")?;

    let client = reqwest::Client::new();
    let mut response = client.get("https://api.github.com/notifications")
        .header(header::Authorization(format!("token {}", github_token)))
        .send()?;

    let notifications: Vec<Notification> = response.json()?;
    let reviews = notifications.into_iter().filter_map(ReviewRequest::from_notification);

    for review in reviews {
        println!("- {:?}", review);
    }

    Ok(())
}
