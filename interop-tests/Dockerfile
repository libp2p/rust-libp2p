FROM ubuntu:22.04
ARG TEST_BINARY
COPY $TEST_BINARY /usr/local/bin/testplan
LABEL org.opencontainers.image.source https://github.com/libp2p/rust-libp2p
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
