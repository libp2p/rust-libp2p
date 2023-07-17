#!/bin/bash

set -ex

ip netns del listener_ns || true
ip netns del dialer_ns || true
ip netns del relay_ns || true
ip link del listen_pub_veth || true
ip link del dialer_pub_veth || true
ip link del relay_pub_veth || true

# Create new network namespaces
ip netns add listener_ns
ip netns add dialer_ns
ip netns add relay_ns

# Create veth pairs
ip link add listen_pub_veth type veth peer listener_veth netns listener_ns
ip link add dialer_pub_veth type veth peer dialer_veth netns dialer_ns
ip link add relay_pub_veth type veth peer relay_veth netns relay_ns

# Bring up veths
ip link set up dev listen_pub_veth
ip link set up dev dialer_pub_veth
ip link set up dev relay_pub_veth
ip netns exec listener_ns ip link set up dev listener_veth
ip netns exec dialer_ns ip link set up dev dialer_veth
ip netns exec relay_ns ip link set up dev relay_veth

# Assign IP addresses to veths in namespaces and enable NAT via iptables
ip addr add 10.0.0.1 dev relay_pub_veth
ip netns exec relay_ns ip addr add 192.168.254.1/24 dev relay_veth

ip addr add 10.0.0.2 dev listen_pub_veth
ip netns exec listener_ns ip addr add 192.168.1.1/24 dev listener_veth

ip addr add 10.0.0.3 dev dialer_pub_veth
ip netns exec dialer_ns ip addr add 172.16.0.1/24 dev dialer_veth

# Set routes correctly for traffic to be able to leave the network namespaces
ip netns exec relay_ns ip route add 10.0.0.0/24 via 192.168.254.1 dev relay_veth
ip netns exec listener_ns ip route add 10.0.0.0/24 via 192.168.1.1 dev listener_veth
ip netns exec dialer_ns ip route add 10.0.0.0/24 via 172.16.0.1 dev dialer_veth

# Enable NAT
ip netns exec relay_ns nft add table ip nat
ip netns exec dialer_ns nft add table ip nat
ip netns exec listener_ns nft add table ip nat

ip netns exec relay_ns nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }
ip netns exec dialer_ns nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }
ip netns exec listener_ns nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }

ip netns exec relay_ns nft add rule ip nat postrouting oifname "relay_veth" snat 10.0.0.1
ip netns exec dialer_ns nft add rule ip nat postrouting oifname "dialer_veth" snat 10.0.0.2
ip netns exec listener_ns nft add rule ip nat postrouting oifname "listener_veth" snat 10.0.0.3

# Enable IP forwarding
echo 1 > /proc/sys/net/ipv4/ip_forward
ip netns exec dialer_ns bash -c 'echo 1 > /proc/sys/net/ipv4/ip_forward'
ip netns exec relay_ns bash -c 'echo 1 > /proc/sys/net/ipv4/ip_forward'
ip netns exec listener_ns bash -c 'echo 1 > /proc/sys/net/ipv4/ip_forward'
