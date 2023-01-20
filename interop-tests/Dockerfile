FROM ubuntu:latest
COPY target/release/ping /usr/local/bin/testplan
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
