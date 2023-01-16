FROM rust:1.65-bullseye as builder
WORKDIR /usr/src/testplan

# TODO fix this, it breaks reproducibility
RUN apt-get update && apt-get install -y cmake protobuf-compiler

RUN mkdir -p ./plan/src
COPY ./src/main.rs ./plan/src/main.rs
COPY ./Cargo.toml ./plan/Cargo.toml
COPY ./Cargo.lock ./plan/Cargo.lock

RUN cd ./plan/ && cargo build  # Initial build acts as a cache.

ARG GIT_TARGET=""
RUN if [ ! -z "${GIT_TARGET}" ]; then sed -i "s,^git.*,git = \"https://${GIT_TARGET}\"," ./plan/Cargo.toml; fi

ARG GIT_REF=""
RUN if [ "master" = "${GIT_REF}" ]; then sed -i "s/^rev.*/branch= \"master\"/" ./plan/Cargo.toml; elif [ ! -z "${GIT_REF}" ]; then sed -i "s/^rev.*/rev = \"${GIT_REF}\"/" ./plan/Cargo.toml; fi

COPY ./src/bin/ ./plan/src/bin/

# Build the requested binary: Cargo will update lockfile on changed manifest (i.e. if one of the above `sed`s patched it).
ARG BINARY_NAME
RUN cd ./plan/ \
    && cargo build --bin=${BINARY_NAME} \
    && mv /usr/src/testplan/plan/target/debug/${BINARY_NAME} /usr/local/bin/testplan

FROM debian:bullseye-slim
COPY --from=builder /usr/local/bin/testplan /usr/local/bin/testplan
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
