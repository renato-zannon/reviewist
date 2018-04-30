use reqwest::unstable::async::Response;
use reqwest::unstable::async::Client as reqwestClient;
use reqwest::StatusCode;
use serde_json::{self, Value};
use failure::Error;
use futures::prelude::*;
use notification::Notification;
use reqwest::header;
use futures::{future, stream};
use futures::future::Either;
use futures::stream::Stream;
use slog::Logger;

pub struct NotificationsResponse {
    pub notifications: Vec<Notification>,
    pub next_page: Option<String>,
    pub last_modified: Option<header::HttpDate>,
    pub poll_interval: Option<u64>,
}

header! { (XPollInterval, "X-Poll-Interval") => [u64] }

impl NotificationsResponse {
    pub fn from_response(
        response: Response,
        logger: Logger,
    ) -> impl Future<Item = NotificationsResponse, Error = Error> {
        match response.status() {
            StatusCode::Ok => Either::A(parse_response(response, logger.clone())),
            StatusCode::NotModified => {
                debug!(logger, "Got 304");
                Either::B(future::ok(not_modified_response(response)))
            }

            _ => Either::B(future::err(format_err!(
                "Unrecognized response: {:?}",
                response
            ))),
        }
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
    response
        .headers()
        .get::<XPollInterval>()
        .cloned()
        .map(|int| int.0)
}

fn parse_last_modified(response: &Response) -> Option<header::HttpDate> {
    response
        .headers()
        .get::<header::LastModified>()
        .map(|header| header.0)
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

pub fn new(
    client: reqwestClient,
    last_modified: header::HttpDate,
    logger: Logger,
) -> impl Stream<Item = NotificationsResponse, Error = Error> {
    let url = Some("https://api.github.com/notifications".to_owned());

    stream::unfold(url, move |maybe_url| {
        let url = maybe_url?;

        let result = get_notifications_page(client.clone(), last_modified, url, logger.clone());

        let result = result.map(move |response| {
            let next_page = response.next_page.clone();
            (response, next_page)
        });

        Some(result)
    })
}

fn get_notifications_page(
    client: reqwestClient,
    last_modified: header::HttpDate,
    page_url: String,
    logger: Logger,
) -> impl Future<Item = NotificationsResponse, Error = Error> {
    let logger = logger.new(o!("url" => page_url.to_string(), "last_modified" => last_modified.to_string()));
    debug!(logger, "Fetching notifications");

    let if_modified_since = header::IfModifiedSince(last_modified);

    let request = client.get(&page_url).header(if_modified_since).send();
    request
        .map_err(Error::from)
        .and_then(move |response| NotificationsResponse::from_response(response, logger))
}
