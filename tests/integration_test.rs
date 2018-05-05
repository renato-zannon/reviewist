extern crate fake_github;
extern crate ipc_channel;
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
    let (server, server_name) = ipc::IpcOneShotServer::<Response>::new().unwrap();

    let mut fake_github = Command::new("cargo")
        .args(&["run", "-p", "fake_github", "--", server_name.as_ref()])
        .spawn()
        .expect("failed to start fake github");
    let mut core = Core::new().expect("failed to start tokio core");

    env::set_var("DATABASE_URL", "db/reviewist_test.db");
    env::set_var("TODOIST_TOKEN", "lol123");
    env::set_var("GITHUB_TOKEN", "lol123");

    let (receiver, message) = server.accept().unwrap();
    let address = match message {
        Response::Booted { port } => port,
        _ => panic!("WTF?"),
    };

    let future = reviewist::run(Config {
        logger: configure_slog(),
        core: &core,
        github_base: Url::parse(&format!("http://{}", address)).unwrap(),
        todoist_base: Url::parse("http://example.com").unwrap(),
    });

    let result = core.run(future);
    fake_github.kill().unwrap();
    result.unwrap();
}

fn configure_slog() -> slog::Logger {
    use slog::Drain;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
}
