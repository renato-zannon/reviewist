#![type_length_limit = "2097152"]

extern crate failure;
extern crate fake_github;
extern crate futures;
extern crate ipc_channel;
extern crate nix;
extern crate reviewist;
#[macro_use]
extern crate slog;
extern crate tokio_core;
extern crate tokio_timer;
extern crate url;

use failure::Error;
use futures::future::{self, Either};
use futures::prelude::*;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio_core::reactor::Core;
use tokio_timer::Delay;

use fake_github::{Message, Response};
use ipc_channel::ipc;
use reviewist::Config;
use std::env;
use url::Url;

#[test]
fn test_smoke() {
    let result = with_fake_server(|server| {
        let mut core = Core::new().expect("failed to start tokio core");

        let future = reviewist::run(Config {
            logger: configure_slog(),
            core: &core,
            github_base: Url::parse(&format!("http://{}/github/", server.address)).unwrap(),
            todoist_base: Url::parse(&format!("http://{}/todoist/", server.address)).unwrap(),
        });
        let limited_future = time_limit(future, 2);

        core.run(limited_future)
    });

    result.unwrap();
}

struct FakeServer {
    receiver: ipc::IpcReceiver<Response>,
    address: std::net::SocketAddr,
}

fn with_fake_server<T>(f: impl FnOnce(FakeServer) -> T) -> T {
    let (server, server_name) = ipc::IpcOneShotServer::<Response>::new().unwrap();

    let mut fake_github = Command::new("cargo")
        .args(&["run", "-p", "fake_github", "--", server_name.as_ref()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start fake github");

    let db_path = new_database();
    env::set_var("DATABASE_URL", db_path.fd_path());
    env::set_var("TODOIST_TOKEN", "lol123");
    env::set_var("GITHUB_TOKEN", "lol123");

    let (receiver, message) = server.accept().unwrap();
    let address = match message {
        Response::Booted { port } => port,
    };

    let result = f(FakeServer { receiver, address });
    fake_github.kill().unwrap();

    result
}

fn time_limit<F>(future: F, seconds: u64) -> impl Future<Item = (), Error = Error>
where
    F: Future<Error = Error>,
{
    let delay = Delay::new(Instant::now() + Duration::from_secs(seconds));

    future.select2(delay).then(|result| match result {
        Ok(_) => future::ok(()),
        Err(Either::B(_)) => future::ok(()),
        Err(Either::A((err, _))) => future::err(err),
    })
}

fn configure_slog() -> slog::Logger {
    slog::Logger::root(slog::Discard, o!())
}

struct DatabasePath {
    path: String,
    fd: std::os::unix::io::RawFd,
}

impl DatabasePath {
    fn fd_path(&self) -> String {
        format!("/proc/self/fd/{}", self.fd)
    }
}

impl Drop for DatabasePath {
    fn drop(&mut self) {
        nix::unistd::close(self.fd).unwrap();
        nix::unistd::unlink(self.path.as_str()).unwrap();
    }
}

fn new_database() -> DatabasePath {
    use nix::unistd::mkstemp;

    let (fd, path) = mkstemp("/tmp/reviewist_test.db.XXXXXX").unwrap();
    let db_path = DatabasePath {
        fd,
        path: path.to_string_lossy().into_owned(),
    };

    Command::new("cargo")
        .env("DATABASE_URL", db_path.fd_path())
        .args(&["run", "--bin", "reviewist_migrate"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("failed to start fake github");

    return db_path;
}
