extern crate fake_github;
extern crate ipc_channel;
extern crate nix;
extern crate reviewist;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;
extern crate tokio_core;
extern crate url;

use std::process::Command;
use tokio_core::reactor::Core;
use reviewist::Config;
use url::Url;
use std::env;
use ipc_channel::ipc;
use fake_github::{Message, Response};

#[test]
fn test_smoke() {
    let result = with_fake_server(|server| {
        let mut core = Core::new().expect("failed to start tokio core");

        let future = reviewist::run(Config {
            logger: configure_slog(),
            core: &core,
            github_base: Url::parse(&format!("http://{}/github/", server.address)).unwrap(),
            todoist_base: Url::parse("http://example.com").unwrap(),
        });

        core.run(future)
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

fn configure_slog() -> slog::Logger {
    use slog::Drain;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
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
        .status()
        .expect("failed to start fake github");

    return db_path;
}
