FROM ubuntu:latest
COPY target/release/ping /usr/local/bin/testplan
LABEL org.opencontainers.image.source https://github.com/thomaseizinger/rust-libp2p
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
