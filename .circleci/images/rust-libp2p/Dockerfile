FROM rust

# Required for the network simulation tests
RUN apt-get update && apt-get install -yq libpcap-dev libclang-dev

RUN mkdir /app
WORKDIR /app
COPY . /app
