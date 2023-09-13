FROM debian:12-slim

ARG DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get -y install iproute2 nftables jq tcpdump

COPY *.sh /scripts/
RUN chmod +x /scripts/*.sh

ENTRYPOINT ["./scripts/run.sh"]
