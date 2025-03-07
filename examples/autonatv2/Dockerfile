FROM rust:1.83-alpine as builder

RUN apk add musl-dev

WORKDIR /workspace
COPY . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package autonatv2 --bin autonatv2_server -F jaeger

RUN --mount=type=cache,target=./target \
    mv ./target/release/autonatv2_server /usr/local/bin/autonatv2_server

FROM alpine:latest

COPY --from=builder /usr/local/bin/autonatv2_server /app/autonatv2_server

EXPOSE 4884

ENTRYPOINT [ "/app/autonatv2_server", "-l", "4884" ]
