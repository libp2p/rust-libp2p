FROM rust:1.38

RUN apt-get update && apt-get install -y protobuf-compiler

RUN cargo install --version 2.8.1 protobuf-codegen
