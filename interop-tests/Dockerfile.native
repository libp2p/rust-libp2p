# syntax=docker/dockerfile:1.5-labs
FROM rust:1.67.0 as builder

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package interop-tests --bin native_ping

RUN --mount=type=cache,target=./target \
    mv ./target/release/native_ping /usr/local/bin/testplan

FROM gcr.io/distroless/cc
COPY --from=builder /usr/local/bin/testplan /usr/local/bin/testplan
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
