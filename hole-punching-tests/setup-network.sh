#!/bin/bash

set -ex

ip netns del ns_internet || true
ip netns del ns_dialer_rt || true
ip netns del ns_dialer_host || true

# Setup internet namespace.
# - We use a bridge to route traffic between all devices in this network.
# - We define 1.1.1.0/24 as the "internet" subnet.
ip netns add ns_internet
ip -n ns_internet link add internet type bridge
ip -n ns_internet link set up dev internet
ip -n ns_internet addr add 1.1.1.1/24 dev internet

# Setup the dialer network.
# - The `_rt` simulates the router device, connecting the dialer to the "internet".
# - The `_host` simulates the host on which the dialer is running which would be a different, physical device in a real network.
# - We use a bridge to simulate the LAN of the dialer.

ip netns add ns_dialer_rt # A router device, connecting the dialer to the "internet"
ip netns add ns_dialer_host # The host on which the dialer is running

ip -n ns_dialer_rt link add dialer_lan type bridge
ip -n ns_dialer_rt link set up dev dialer_lan
ip -n ns_dialer_rt addr add 192.168.0.1/24 dev dialer_lan

# A cable connecting the dialer to the internet.
# We can use the same name for both ends of the cable because they are in different network namespaces.
# Plus, a cable attached to a bridge does not have a separate IP address. This simulates the bridge assigning an IP to the "device", i.e. the other end of the cable.
ip link add dialer_pub_eth netns ns_dialer_rt type veth peer dialer_pub_eth netns ns_internet
ip -n ns_internet link set dialer_pub_eth master internet
ip -n ns_internet link set up dev dialer_pub_eth
ip -n ns_dialer_rt addr add 1.1.1.2/24 dev dialer_pub_eth
ip -n ns_dialer_rt link set up dev dialer_pub_eth

# A cable connecting the dialer host to the dialer router
ip link add dial_local_eth netns ns_dialer_host type veth peer dial_local_eth netns ns_dialer_rt
ip -n ns_dialer_rt link set dial_local_eth master dialer_lan
ip -n ns_dialer_rt link set up dev dial_local_eth
ip -n ns_dialer_host addr add 192.168.0.2/24 dev dial_local_eth
ip -n ns_dialer_host link set up dev dial_local_eth
ip -n ns_dialer_host route add default via 192.168.0.2 dev dial_local_eth

ip netns exec ns_dialer_rt nft add table ip nat
ip netns exec ns_dialer_rt nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }
ip netns exec ns_dialer_rt nft add rule ip nat postrouting oifname "dialer_pub_eth" masquerade

#ip link add dialer_local_eth netns ns_dialer_rt type veth peer dialer_pub_eth netns internet_ns # A cable connecting the dialer to the internet
#
#ip netns add ns_listener_rt # The router device, connecting the listener to the "internet"
#ip netns add ns_listener_host # The host on which the listener is running
#
#ip -n ns_listener_rt link add listener_network type bridge # The local network of the listener
#ip -n ns_listener_rt link set up dev listener_network
#ip link add listener_local_eth netns ns_listener_rt type veth peer listener_pub_eth netns internet_ns # A cable connecting the listener to the internet
#ip -n internet_ns link set listener_pub_eth master internet_network
#
#ip netns del listener_ns || true
#ip netns del dialer_ns || true
#ip netns del relay_ns || true
#ip netns del internet_ns || true
#ip link del listen_pub_veth || true
#ip link del dialer_pub_veth || true
#ip link del relay_pub_veth || true
#
## Create new network namespaces
#ip netns add listener_ns
#ip netns add dialer_ns
#ip netns add relay_ns
#ip netns add internet_ns
#
## Create veth pairs
#ip link add listen_br_veth netns internet_ns type veth peer listener_veth netns listener_ns
#ip link add dialer_br_veth netns internet_ns type veth peer dialer_veth netns dialer_ns
#ip link add relay_br_veth netns internet_ns type veth peer relay_veth netns relay_ns
#
#ip link add listen_br type bridge
#ip link add dialer_br type bridge
#ip link add relay_br type bridge
#
## Bring up veths and loopback interfaces
#ip netns exec internet_ns ip link set up dev listen_pub_veth
#ip netns exec internet_ns ip link set up dev dialer_pub_veth
#ip netns exec internet_ns ip link set up dev relay_pub_veth
#ip netns exec internet_ns ip link set up dev lo
#
#ip netns exec listener_ns ip link set up dev listener_veth
#ip netns exec listener_ns ip link set up dev lo
#
#ip netns exec dialer_ns ip link set up dev dialer_veth
#ip netns exec dialer_ns ip link set up dev lo
#
#ip netns exec relay_ns ip link set up dev relay_veth
#ip netns exec relay_ns ip link set up dev lo
#
## Assign IP addresses to veths in namespaces
#ip netns exec internet_ns ip addr add 10.0.1.1/24 dev relay_pub_veth
#ip netns exec relay_ns ip addr add 10.0.1.2/24 dev relay_veth
#
#ip netns exec internet_ns ip addr add 10.0.2.1/24 dev listen_pub_veth
#ip netns exec listener_ns ip addr add 10.0.2.2/24 dev listener_veth
#
#ip netns exec internet_ns ip addr add 10.0.3.1/24 dev dialer_pub_veth
#ip netns exec dialer_ns ip addr add 10.0.3.2/24 dev dialer_veth
#
## Set routes correctly for traffic to be able to leave the network namespaces
#ip netns exec relay_ns route add -net 0.0.0.0/0 gw 10.0.1.1
#ip netns exec listener_ns route add -net 0.0.0.0/0 gw 10.0.2.1
#ip netns exec dialer_ns route add -net 0.0.0.0/0 gw 10.0.3.1
#
##ip netns exec internet_ns iptables -t nat -A POSTROUTING -s 10.0.1.0/24 -j MASQUERADE
##ip netns exec internet_ns iptables -t nat -A POSTROUTING -s 10.0.2.0/24 -j MASQUERADE
##ip netns exec internet_ns iptables -t nat -A POSTROUTING -s 10.0.3.0/24 -j MASQUERADE
#
### Enable NAT
##ip netns exec relay_ns nft add table ip nat
##ip netns exec dialer_ns nft add table ip nat
##ip netns exec listener_ns nft add table ip nat
##
##ip netns exec relay_ns nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }
##ip netns exec dialer_ns nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }
##ip netns exec listener_ns nft add chain ip nat postrouting { type nat hook postrouting priority srcnat \; meta nftrace set 1 \; }
##
##ip netns exec relay_ns nft add rule ip nat postrouting oifname "relay_veth" snat 10.0.1.1
##ip netns exec listener_ns nft add rule ip nat postrouting oifname "listener_veth" snat 10.0.2.1
##ip netns exec dialer_ns nft add rule ip nat postrouting oifname "dialer_veth" snat 10.0.3.1
#
#
## Enable IP forwarding
#ip netns exec internet_ns echo 1 > /proc/sys/net/ipv4/ip_forward
