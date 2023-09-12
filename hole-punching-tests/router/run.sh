#!/bin/sh

set -ex

ADDR_EXTERNAL=$(ip -json addr show eth1 | jq '.[0].addr_info[0].local' -r)
SUBNET_INTERNAL=$(ip -json addr show eth0 | jq '.[0].addr_info[0].local + "/" + (.[0].addr_info[0].prefixlen | tostring)' -r)

nft add table ip nat
nft add chain ip nat postrouting { type nat hook postrouting priority 100 \; }
nft add rule ip nat postrouting ip saddr $SUBNET_INTERNAL oifname "eth1" snat $ADDR_EXTERNAL

tc qdisc add dev eth1 root netem delay 50ms

tcpdump -i eth0 -n -w /dump.pcap &

ulogd
