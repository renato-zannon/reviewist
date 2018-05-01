extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
extern crate dotenv;
#[macro_use]
extern crate failure;

use dotenv::dotenv;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use failure::Error;
use std::env;

embed_migrations!();

fn main() {
    dotenv().ok();

    let err = match run() {
        Ok(_) => return,
        Err(err) => err,
    };

    if let Some(bt) = err.cause().backtrace() {
        eprintln!("critical error: backtrace={:?}", bt);
    } else {
        eprintln!("critical error: cause={:?}", err.cause());
    }
}

fn run() -> Result<(), Error> {
    let connection = establish_connection()?;

    embedded_migrations::run_with_output(&connection, &mut std::io::stdout())?;

    Ok(())
}

fn establish_connection() -> Result<SqliteConnection, Error> {
    let database_url = env::var("DATABASE_URL").map_err(|_| format_err!("DATABASE_URL must be set"))?;

    SqliteConnection::establish(&database_url)
        .map_err(move |err| format_err!("Error while connecting to {}: {}", database_url, err))
}
