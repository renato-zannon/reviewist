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

use notification::PullRequest;
use review_handler::ReviewHandler;

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

    let mut core = reactor::Core::new()?;
    let client = github_client::GithubClient::new(&core.handle(), logger.clone())?;
    let handler = review_handler::new()?;

    let f = notifications_polling::poll_notifications(client, logger).for_each(move |(pull_request, logger)| {
        let record_logger = logger.new(o!("pull_request" => pull_request.number));

        if !pull_request.is_open() {
            debug!(record_logger, "Skipping closed pull request");
            return future::ok(());
        }

        let handler_result = handle_pr(handler.clone(), pull_request, record_logger);
        tokio::spawn(handler_result);
        future::ok(())
    });

    core.run(f)
}

fn handle_pr(handler: ReviewHandler, pr: PullRequest, logger: slog::Logger) -> impl Future<Item = (), Error = ()> {
    let err_logger = logger.clone();

    handler
        .record_review_request(pr)
        .map_err(move |err| {
            error!(err_logger, "Error while recording review request"; "err" => %err);
        })
        .and_then(move |maybe_result| {
            let pull_request = match maybe_result {
                None => return future::ok(()),
                Some(res) => res,
            };

            info!(logger, "PR received"; "pull_request" => ?pull_request);
            future::ok(())
        })
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
        pr_title: String,
    }

    #[derive(Clone)]
    pub struct ReviewHandler {
        connection: Arc<Mutex<SqliteConnection>>,
    }

    impl ReviewHandler {
        pub fn record_review_request(&self, pr: PullRequest) -> impl Future<Item = Option<PullRequest>, Error = Error> {
            let new_request = NewReviewRequest {
                project: pr.repo().to_string(),
                pr_url: pr.html_url.to_string(),
                pr_number: pr.number.to_string(),
                pr_title: pr.title.to_string(),
            };
            let conn = self.connection.clone();
            let perform_insert = move || insert_review_request(&new_request, &*conn.lock().unwrap());

            poll_fn(move || blocking(&perform_insert)).then(|res| {
                let result = match res {
                    Ok(Ok(true)) => Ok(Some(pr)),
                    Ok(Ok(false)) => Ok(None),
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

    fn insert_review_request(new_request: &NewReviewRequest, conn: &SqliteConnection) -> Result<bool, Error> {
        use diesel::{insert_into, select};
        use diesel::dsl::exists;
        use super::schema::review_requests::dsl::*;

        let existing_rq = review_requests.filter(
            project
                .eq(&new_request.project)
                .and(pr_number.eq(&new_request.pr_number)),
        );

        let rq_exists = select(exists(existing_rq))
            .get_result(conn)
            .map_err(Error::from)?;

        if rq_exists {
            return Ok(false);
        }

        insert_into(review_requests)
            .values(new_request)
            .execute(conn)
            .map(|_| true)
            .map_err(Error::from)
    }

    fn establish_connection() -> Result<SqliteConnection, Error> {
        let database_url = env::var("DATABASE_URL").map_err(|_| format_err!("DATABASE_URL must be set"))?;

        SqliteConnection::establish(&database_url)
            .map_err(move |err| format_err!("Error while connecting to {}: {}", database_url, err))
    }
}
