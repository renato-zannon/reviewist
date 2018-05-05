extern crate gotham;
extern crate hyper;
extern crate ipc_channel;
extern crate mime;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::env;
use hyper::StatusCode;
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

fn say_hello(state: State) -> (State, hyper::Response) {
    let response_string = String::from(include_str!("../data/notifications.json"));

    let res = create_response(
        &state,
        StatusCode::Ok,
        Some((response_string.into_bytes(), mime::APPLICATION_JSON)),
    );

    (state, res)
}

fn router() -> Router {
    build_simple_router(|route| {
        route.get("/notifications").to(say_hello);
    })
}

pub fn run() {
    let addr = get_open_port();
    let sender = build_sender();

    sender
        .send(Response::Booted { port: addr.clone() })
        .unwrap();

    gotham::start(addr, router());
}

fn get_open_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    return addr;
}

fn build_sender() -> ipc::IpcSender<Response> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let server_path = args.pop().unwrap();

    ipc::IpcSender::connect(server_path).unwrap()
}
