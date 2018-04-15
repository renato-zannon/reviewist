use reqwest::unstable::async::Response;
use serde_json::{self, Value};
use failure::Error;
use futures::prelude::*;
use futures::future::Either;
use notification::Notification;
use reqwest::header;
use futures::future;

pub struct NotificationsResponse {
    pub notifications: Vec<Notification>,
    pub next_page: Option<String>,
    pub last_modified: header::HttpDate,
    pub poll_interval: Option<i64>,
}

header! { (XPollInterval, "X-Poll-Interval") => [i64] }

impl NotificationsResponse {
    pub fn from_response(
        mut response: Response,
    ) -> impl Future<Item = NotificationsResponse, Error = Error> {
        let last_modified = response
            .headers()
            .get::<header::LastModified>()
            .map(|header| header.0);
        let last_modified = match last_modified {
            Some(date) => date,
            None => {
                return Either::A(future::err(format_err!(
                    "Response didn't contain Last-Modified header"
                )))
            }
        };

        let poll_interval = response
            .headers()
            .get::<XPollInterval>()
            .cloned()
            .map(|int| int.0);
        let next_page = next_page_url(&response);

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

        Either::B(result)
    }
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
