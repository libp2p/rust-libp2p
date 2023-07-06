# Start from Ubuntu
#FROM rust:1.70-slim as builder
#
#WORKDIR /workspace
#ADD . .
#
#RUN --mount=type=cache,target=./target \
#    --mount=type=cache,target=/usr/local/cargo/registry \
#    cargo build --package hole-punching-tests && cp target/debug/client /usr/local/bin/client && cp target/debug/relay /usr/local/bin/relay

FROM debian:12

RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get install -y systemd iproute2 nftables iputils-ping

#COPY --from=builder /usr/local/bin/client /usr/local/bin/
#COPY --from=builder /usr/local/bin/relay /usr/local/bin/

ADD ./target/debug/client /usr/local/bin/
ADD ./target/debug/relay /usr/local/bin/

ADD ./hole-punching-tests/setup-network.sh /usr/local/bin/

COPY ./hole-punching-tests/relay.service /etc/systemd/system/
COPY ./hole-punching-tests/setup-network.service /etc/systemd/system/
COPY ./hole-punching-tests/listener.service /etc/systemd/system/

RUN systemctl enable nftables.service
RUN systemctl enable setup-network.service
RUN systemctl enable relay.service
RUN systemctl enable listener.service

RUN chmod +x /usr/local/bin/setup-network.sh

CMD ["/usr/lib/systemd/systemd"]
