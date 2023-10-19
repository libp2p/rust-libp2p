# syntax=docker/dockerfile:1.5-labs

FROM --platform=$BUILDPLATFORM rust:1.72.0 as builder

ARG BUILDPLATFORM

# Set the working directory
WORKDIR /workspace

# Copy the Rust project source code, including build.sh, into the image
COPY . /workspace

# Make the build.sh script executable and run it
RUN chmod +x /workspace/build.sh && /workspace/build.sh ${BUILDPLATFORM}

FROM alpine:3
COPY --from=builder /usr/local/bin/hole-punching-tests /usr/bin/hole-punch-client
RUN --mount=type=cache,target=/var/cache/apk apk add bind-tools jq curl tcpdump iproute2-tc

ENV RUST_BACKTRACE=1
