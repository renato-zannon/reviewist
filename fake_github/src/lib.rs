extern crate gotham;
#[macro_use]
extern crate gotham_derive;
#[macro_use]
extern crate hyper;
extern crate ipc_channel;
#[macro_use]
extern crate lazy_static;
extern crate mime;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;

use std::env;
use hyper::StatusCode;
use hyper::Uri;
use std::net::SocketAddr;

use ipc_channel::ipc;

use gotham::http::response::create_response;
use gotham::state::State;
use gotham::router::Router;
use gotham::router::builder::*;

pub enum Message {
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    Booted { port: SocketAddr },
}

lazy_static! {
    static ref ADDR: SocketAddr = get_open_port();
}

header! { (XPollInterval, "X-Poll-Interval") => [u64] }

fn notifications(state: State) -> (State, hyper::Response) {
    let pr_url = format!("http://{}/github/pull_requests/1", &*ADDR);

    let response_json = json!([
        {
            "reason": "review_requested",
            "subject": {
                "title": "Some important PR",
                "url": pr_url,
                "type": "PullRequest"
            },

            "repository": {
                "name": "reviewist",
            }
        }
    ]);
    let response_body = serde_json::to_vec(&response_json).unwrap();

    let mut res = create_response(
        &state,
        StatusCode::Ok,
        Some((response_body, mime::APPLICATION_JSON)),
    );

    res.headers_mut().set(XPollInterval(1));

    (state, res)
}

fn get_pull_request(state: State) -> (State, hyper::Response) {
    let response_body = {
        let PullRequestParams { id, .. } = state.borrow();

        let response_json = json!({
            "number": id,
            "title": "Some important PR",
            "html_url": "https://example.com",

            "created_at": "2018-01-01T00:00:00Z",
            "merged_at": null,
            "closed_at": null,
            "base": {
                "repo": {
                    "name": "reviewist",
                },
            },
        });

        serde_json::to_vec(&response_json).unwrap()
    };

    let res = create_response(
        &state,
        StatusCode::Ok,
        Some((response_body, mime::APPLICATION_JSON)),
    );

    (state, res)
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
struct PullRequestParams {
    id: i32,
}

fn router() -> Router {
    build_simple_router(|route| {
        route.get("/github/notifications").to(notifications);

        route
            .get("/github/pull_requests/:id")
            .with_path_extractor::<PullRequestParams>()
            .to(get_pull_request);
    })
}

pub fn run() {
    if let Some(sender) = build_sender() {
        sender
            .send(Response::Booted { port: ADDR.clone() })
            .unwrap();
    }

    gotham::start(ADDR.clone(), router());
}

fn get_open_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    return addr;
}

fn build_sender() -> Option<ipc::IpcSender<Response>> {
    let server_path: String = env::args().skip(1).next()?;

    ipc::IpcSender::connect(server_path).ok()
}
