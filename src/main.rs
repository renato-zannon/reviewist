extern crate dotenv;
extern crate env_logger;
extern crate failure;
extern crate futures;
extern crate openssl_probe;
extern crate reviewist;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;
extern crate tokio_core;

use dotenv::dotenv;
use failure::Error;
use slog::Drain;
use tokio_core::reactor::Core as TokioCore;

use reviewist::Config;

fn main() {
    let logger = configure_slog();
    env_logger::init();
    openssl_probe::init_ssl_cert_env_vars();
    dotenv().ok();

    let result = TokioCore::new().map_err(Error::from).and_then(|mut core| {
        let future = reviewist::run(Config::defaults(logger.clone(), &core));

        core.run(future)
    });

    let err = match result {
        Ok(_) => return,
        Err(err) => err,
    };

    if let Some(bt) = err.cause().backtrace() {
        error!(logger, "critical error"; "backtrace" => %bt);
    } else {
        error!(logger, "critical error"; "cause" => ?err.cause());
    }
}

fn configure_slog() -> slog::Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
}
