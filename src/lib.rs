extern crate chrono;
#[macro_use]
extern crate diesel;
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
#[macro_use]
extern crate slog;
extern crate tokio;
extern crate tokio_core;
extern crate tokio_retry;
extern crate tokio_threadpool;
extern crate tokio_timer;

mod github;
mod todoist_client;
mod review_handler;
mod schema;

use failure::Error;
use futures::future::{self, Either};
use futures::prelude::*;
use tokio_core::reactor::Core as TokioCore;

use github::GithubClient;
use review_handler::ReviewHandler;
use todoist_client::TodoistClient;

pub struct Config<'a> {
    pub logger: slog::Logger,
    pub core: &'a TokioCore,
}

impl<'a> Config<'a> {
    pub fn defaults(logger: slog::Logger, core: &TokioCore) -> Config {
        Config { logger, core }
    }
}

pub fn run(config: Config) -> impl Future<Item = (), Error = Error> {
    macro_rules! early_error {
        ($e:expr) => (
            match $e {
                Ok(res) => res,
                Err(err) => return Either::A(future::err(Error::from(err)))
            }
        )
    }

    let Config { core, logger } = config;

    let main_future = build_main_future(State {
        github_client: early_error!(github::new_client(&core.handle(), logger.clone())),
        handler: early_error!(review_handler::new()),
        todoist_client: early_error!(todoist_client::TodoistClient::new(
            &core.handle(),
            logger.clone()
        )),
    });

    Either::B(main_future)
}

struct State {
    github_client: GithubClient,
    todoist_client: TodoistClient,
    handler: ReviewHandler,
}

fn build_main_future(state: State) -> impl Future<Item = (), Error = Error> {
    let State {
        github_client,
        todoist_client,
        handler,
    } = state;

    let stream = github_client.into_pull_request_stream();

    stream.for_each(move |(pull_request, logger)| {
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
    })
}
