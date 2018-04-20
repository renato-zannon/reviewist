FROM ekidd/rust-musl-builder:beta as builder

WORKDIR /home/rust
ENV USER=rust

RUN cargo new --bin app
COPY Cargo.toml Cargo.lock /home/rust/app/
WORKDIR /home/rust/app

RUN cargo build --release

COPY src /home/rust/app/src
RUN sudo chown -R rust:rust ./ && touch src/main.rs
RUN cargo build --release && strip target/x86_64-unknown-linux-musl/release/reviewist

FROM alpine:latest
RUN apk --no-cache add ca-certificates
WORKDIR /root/
COPY --from=builder /home/rust/app/target/x86_64-unknown-linux-musl/release/reviewist /usr/local/bin

CMD ["/usr/local/bin/reviewist"]