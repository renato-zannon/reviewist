extern crate gotham;
extern crate hyper;
extern crate ipc_channel;
extern crate mime;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::env;
use hyper::StatusCode;
use gotham::http::response::create_response;
use gotham::state::State;
use ipc_channel::ipc;
use std::net::SocketAddr;

pub enum Message {
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    Booted { port: SocketAddr },
}

/// Create a `Handler` which is invoked when responding to a `Request`.
///
/// How does a function become a `Handler`?.
/// We've simply implemented the `Handler` trait, for functions that match the signature used here,
/// within Gotham itself.
fn say_hello(state: State) -> (State, hyper::Response) {
    let res = create_response(
        &state,
        StatusCode::Ok,
        Some((String::from("{}").into_bytes(), mime::TEXT_PLAIN)),
    );

    (state, res)
}

/// Start a server and call the `Handler` we've defined above for each `Request` we receive.
pub fn run() {
    let mut args: Vec<String> = env::args().skip(1).collect();

    let server_path = args.pop().unwrap();
    let addr = get_open_port();

    let sender = ipc::IpcSender::<Response>::connect(server_path).unwrap();
    sender
        .send(Response::Booted { port: addr.clone() })
        .unwrap();

    gotham::start(addr, || Ok(say_hello))
}

fn get_open_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    return addr;
}
