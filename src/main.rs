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
mod notification;
mod notification_stream;
mod notifications_polling;
mod schema;

use dotenv::dotenv;
use failure::Error;

use tokio_core::reactor;
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
        error!(logger, "critical error"; "backtrace" => %bt);
    } else {
        error!(logger, "critical error"; "cause" => ?err.cause());
    }
}

fn run(logger: slog::Logger) -> Result<(), Error> {
    dotenv().ok();
    let handler = review_handler::new()?;

    let mut core = reactor::Core::new()?;
    let client = github_client::GithubClient::new(&core.handle(), logger.clone())?;

    let future = notifications_polling::poll_notifications(client, logger).for_each(move |(pull_request, logger)| {
        let record_logger = logger.new(o!("pull_request" => pull_request.number));

        if !pull_request.is_open() {
            debug!(record_logger, "Skipping closed pull request");
            return future::ok(());
        }

        tokio::spawn(
            handler
                .record_review_request(pull_request.clone())
                .map_err(move |err| {
                    error!(record_logger, "Error while recording review request"; "err" => %err);
                }),
        );

        info!(logger, "PR received"; "pull_request" => ?pull_request);
        future::ok(())
    });

    core.run(future)
}

fn configure_slog() -> slog::Logger {
    use slog::Drain;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
}

mod review_handler {
    use failure::Error;
    use diesel::prelude::*;
    use diesel::sqlite::SqliteConnection;
    use std::env;
    use futures::prelude::*;
    use futures::future::{self, poll_fn};

    use tokio_threadpool::blocking;
    use super::schema::review_requests;

    use notification::PullRequest;
    use std::sync::{Arc, Mutex};

    #[derive(Insertable)]
    #[table_name = "review_requests"]
    pub struct NewReviewRequest {
        project: String,
        pr_number: String,
        pr_url: String,
    }

    #[derive(Clone)]
    pub struct ReviewHandler {
        connection: Arc<Mutex<SqliteConnection>>,
    }

    impl ReviewHandler {
        pub fn record_review_request(&self, pr: PullRequest) -> impl Future<Item = (), Error = Error> {
            let new_request = NewReviewRequest {
                project: pr.repo().to_string(),
                pr_url: pr.html_url,
                pr_number: pr.number.to_string(),
            };
            let conn = self.connection.clone();

            poll_fn(move || {
                blocking(|| {
                    use diesel::insert_into;
                    use super::schema::review_requests::dsl::*;

                    let conn = conn.lock().unwrap();

                    insert_into(review_requests)
                        .values(&new_request)
                        .execute(&*conn)
                        .map_err(Error::from)
                })
            }).then(|res| {
                let result = match res {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(err)) => Err(err),
                    Err(_) => Err(format_err!("Error while scheduling work")),
                };

                future::result(result)
            })
        }
    }

    pub fn new() -> Result<ReviewHandler, Error> {
        let connection = establish_connection()?;
        Ok(ReviewHandler {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn establish_connection() -> Result<SqliteConnection, Error> {
        let database_url = env::var("DATABASE_URL").map_err(|_| format_err!("DATABASE_URL must be set"))?;

        SqliteConnection::establish(&database_url)
            .map_err(move |err| format_err!("Error while connecting to {}: {}", database_url, err))
    }
}
