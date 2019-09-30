FROM rust:1.38

RUN apt-get update && apt-get install -y protobuf-compiler

RUN cargo install --version 2.3.0 protobuf-codegen
