FROM rust

RUN apt-get update && apt-get install -yq libpcap-dev

RUN mkdir /app
WORKDIR /app
COPY . /app
