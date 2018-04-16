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
    ) -> impl Future<Item = NotificationsResponse, Error = Error> {
        match response.status() {
            StatusCode::Ok => Either::A(parse_response(response)),
            StatusCode::NotModified => {
                eprintln!("Got 304");
                Either::B(future::ok(not_modified_response(response)))
            },

            _ => Either::B(future::err(format_err!("Unrecognized response: {:?}", response))),
        }
    }
}

fn parse_response(mut response: Response) -> impl Future<Item = NotificationsResponse, Error = Error> {
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
                        eprintln!("Problem parsing notification: {:?}", err);
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

pub struct NotificationStream {
    inner: Box<Stream<Item = NotificationsResponse, Error = Error> + 'static>,
}

impl NotificationStream {
    pub fn new(client: reqwestClient, last_modified: header::HttpDate) -> NotificationStream {
        let url = Some("https://api.github.com/notifications?all=true".to_owned());

        let stream = stream::unfold(url, move |maybe_url| {
            let url = maybe_url?;

            let result = get_notifications_page(client.clone(), last_modified, url).map(move |response| {
                let next_page = response.next_page.clone();
                (response, next_page)
            });

            // TODO: Remove boxing once https://github.com/rust-lang/rust/issues/49685 is solved
            let result: Box<Future<Item = _, Error = _>> = Box::new(result);
            Some(result)
        });

        NotificationStream { inner: Box::new(stream) }
    }
}

impl Stream for NotificationStream {
    type Item = NotificationsResponse;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<NotificationsResponse>, Error> {
        self.inner.poll()
    }
}

fn get_notifications_page(client: reqwestClient, last_modified: header::HttpDate, page_url: String) -> impl Future<Item = NotificationsResponse, Error = Error> {
    eprintln!("Fetching {} with IMS {}", page_url, last_modified);
    let if_modified_since = header::IfModifiedSince(last_modified);

    let request = client.get(&page_url).header(if_modified_since).send();
    request.map_err(Error::from).and_then(NotificationsResponse::from_response)
}
