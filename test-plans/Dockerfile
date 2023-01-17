FROM rust:1.65-bullseye as builder
WORKDIR /usr/src/libp2p

# TODO fix this, it breaks reproducibility
RUN apt-get update && apt-get install -y cmake protobuf-compiler

COPY ./ ./

RUN cd ./test-plans/ && cargo build  # Initial build acts as a cache.

ARG BINARY_NAME
RUN cd ./test-plans/ \
    && cargo build --bin=${BINARY_NAME} \
    && mv /usr/src/libp2p/test-plans/target/debug/${BINARY_NAME} /usr/local/bin/testplan

FROM debian:bullseye-slim
COPY --from=builder /usr/local/bin/testplan /usr/local/bin/testplan
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
