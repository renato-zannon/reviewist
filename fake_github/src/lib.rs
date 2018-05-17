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

use hyper::StatusCode;
use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use ipc_channel::ipc;

use gotham::http::response::create_response;
use gotham::router::Router;
use gotham::router::builder::*;
use gotham::state::State;

#[derive(Serialize, Deserialize, Debug)]
pub enum Message {
    GetTaskCount,
    AddReviewRequest,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Booted {
        port: SocketAddr,
        sender: ipc::IpcSender<Message>,
    },
    TaskCountResponse(usize),
}

lazy_static! {
    static ref ADDR: SocketAddr = get_open_port();
}

header! { (XPollInterval, "X-Poll-Interval") => [u64] }

fn notifications(state: State) -> (State, hyper::Response) {
    let count = REVIEW_REQUEST_COUNT.load(Ordering::Relaxed);

    let pull_requests: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            let pr_url = format!("http://{}/github/pull_requests/{}", &*ADDR, i);

            json!({
                "reason": "review_requested",
                "subject": {
                    "title": "Some important PR",
                    "url": pr_url,
                    "type": "PullRequest"
                },

                "repository": {
                    "name": "reviewist",
                }
            })
        })
        .collect();

    let response_body = serde_json::to_vec(&serde_json::Value::Array(pull_requests)).unwrap();

    let mut res = create_response(&state, StatusCode::Ok, Some((response_body, mime::APPLICATION_JSON)));

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

    let res = create_response(&state, StatusCode::Ok, Some((response_body, mime::APPLICATION_JSON)));

    (state, res)
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
struct PullRequestParams {
    id: i32,
}

lazy_static! {
    static ref TASK_COUNT: AtomicUsize = AtomicUsize::new(0);
    static ref REVIEW_REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);
}

fn create_task(state: State) -> (State, hyper::Response) {
    TASK_COUNT.fetch_add(1, Ordering::Relaxed);

    let res = create_response(&state, StatusCode::Ok, Some((b"OK".to_vec(), mime::TEXT_PLAIN)));
    (state, res)
}

fn router() -> Router {
    build_simple_router(|route| {
        route.get("/github/notifications").to(notifications);

        route
            .get("/github/pull_requests/:id")
            .with_path_extractor::<PullRequestParams>()
            .to(get_pull_request);

        route.post("/todoist/API/v8/tasks").to(create_task);
    })
}

pub fn run() {
    bootstrap_channels();

    gotham::start(ADDR.clone(), router());
}

fn bootstrap_channels() -> Option<()> {
    let response_sender = build_response_sender()?;
    let message_sender = build_message_sender(response_sender.clone())?;

    response_sender
        .send(Response::Booted {
            port: ADDR.clone(),
            sender: message_sender,
        })
        .unwrap();

    Some(())
}

fn build_message_sender(response_sender: ipc::IpcSender<Response>) -> Option<ipc::IpcSender<Message>> {
    let (message_sender, message_receiver) = ipc::channel().ok()?;
    thread::spawn(move || process_messages(message_receiver, response_sender));

    Some(message_sender)
}

fn process_messages(receiver: ipc::IpcReceiver<Message>, sender: ipc::IpcSender<Response>) {
    while let Ok(message) = receiver.recv() {
        match message {
            Message::GetTaskCount => {
                let value = TASK_COUNT.load(Ordering::Relaxed);
                sender.send(Response::TaskCountResponse(value)).ok();
            }

            Message::AddReviewRequest => {
                REVIEW_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn build_response_sender() -> Option<ipc::IpcSender<Response>> {
    let server_path: String = env::args().skip(1).next()?;

    ipc::IpcSender::connect(server_path).ok()
}

fn get_open_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    return addr;
}
