FROM ubuntu:jammy

ARG DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get -y install iproute2 nftables ulogd2 jq

# Debugging tools
RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get -y install tcpdump iputils-ping

COPY *.sh /scripts/
RUN chmod +x /scripts/*.sh
COPY ulogd.conf /etc/ulogd.conf

ENTRYPOINT ["./scripts/run.sh"]
