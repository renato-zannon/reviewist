use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use failure::Error;
use futures::future::{self, poll_fn};
use futures::prelude::*;
use futures::sync::oneshot;
use std::env;

use super::schema::review_requests;
use slog::Logger;
use tokio;
use tokio_threadpool::blocking;

use github::PullRequest;
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
    pub fn record_in_task(
        &self,
        pr: PullRequest,
        logger: Logger,
    ) -> impl Future<Item = Option<PullRequest>, Error = Error> {
        let (sender, receiver) = oneshot::channel();

        let future = self.record_review_request(pr).then(move |maybe_result| {
            match maybe_result {
                Ok(Some(pr)) => {
                    info!(logger, "PR received"; "pull_request" => ?pr);
                    sender.send(Some(pr)).ok();
                }

                Err(err) => {
                    error!(logger, "Error while recording review request"; "err" => %err);
                    sender.send(None).ok();
                }

                _ => {
                    sender.send(None).ok();
                }
            };

            Ok(())
        });

        tokio::spawn(future);
        receiver.map_err(Error::from)
    }

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
    use super::schema::review_requests::dsl::*;
    use diesel::dsl::exists;
    use diesel::{insert_into, select};

    let existing_rq = review_requests.filter(
        project
            .eq(&new_request.project)
            .and(pr_number.eq(&new_request.pr_number)),
    );

    let rq_exists = select(exists(existing_rq)).get_result(conn).map_err(Error::from)?;

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
