FROM ubuntu:latest
COPY target/release/interop-tests /usr/local/bin/testplan
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
