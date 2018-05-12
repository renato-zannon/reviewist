use failure::Error;
use futures::future::{self, Either};
use futures::prelude::*;
use reqwest::StatusCode;
use reqwest::header;
use reqwest::unstable::async::Response;
use serde_json::{self, Value};
use slog::Logger;

use github::notification::Notification;

pub struct NotificationsResponse {
    pub notifications: Vec<Notification>,
    pub next_page: Option<String>,
    pub last_modified: Option<header::HttpDate>,
    pub poll_interval: Option<u64>,
}

header! { (XPollInterval, "X-Poll-Interval") => [u64] }

pub fn from_http(response: Response, logger: Logger) -> impl Future<Item = NotificationsResponse, Error = Error> {
    match response.status() {
        StatusCode::Ok => Either::A(parse_response(response, logger.clone())),
        StatusCode::NotModified => {
            debug!(logger, "Got 304");
            Either::B(future::ok(not_modified_response(response)))
        }

        _ => Either::B(future::err(format_err!("Unrecognized response: {:?}", response))),
    }
}

fn parse_response(mut response: Response, logger: Logger) -> impl Future<Item = NotificationsResponse, Error = Error> {
    let next_page = next_page_url(&response);
    let last_modified = parse_last_modified(&response);
    let poll_interval = parse_poll_interval(&response);

    let result = response
        .json::<Vec<Value>>()
        .map(move |objects| {
            let notifications = objects
                .into_iter()
                .filter_map(|object| match serde_json::from_value(object) {
                    Ok(notification) => Some(notification),

                    Err(err) => {
                        warn!(logger, "Problem parsing notification"; "error" => %err);
                        return None;
                    }
                })
                .collect();

            NotificationsResponse {
                notifications,
                next_page,
                last_modified,
                poll_interval,
            }
        })
        .map_err(Error::from);

    result
}

fn not_modified_response(response: Response) -> NotificationsResponse {
    NotificationsResponse {
        notifications: vec![],
        next_page: None,
        last_modified: None,
        poll_interval: parse_poll_interval(&response),
    }
}

fn parse_poll_interval(response: &Response) -> Option<u64> {
    response.headers().get::<XPollInterval>().cloned().map(|int| int.0)
}

fn parse_last_modified(response: &Response) -> Option<header::HttpDate> {
    response.headers().get::<header::LastModified>().map(|header| header.0)
}

fn next_page_url(response: &Response) -> Option<String> {
    use reqwest::header::RelationType;

    let link_header: &header::Link = response.headers().get()?;

    let value = link_header.values().iter().find(|value| match value.rel() {
        Some(rels) => rels.contains(&RelationType::Next),
        None => false,
    });

    Some(value?.link().to_owned())
}
