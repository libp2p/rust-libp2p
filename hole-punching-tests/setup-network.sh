#!/bin/bash

set -ex

# Create new network namespaces
ip netns add relay_ns
ip netns add listener_ns
ip netns add dialer_ns

# Create a new bridge
brctl addbr the_internet

# Set up the bridge
ip link set up dev the_internet
ip addr add 10.0.0.254/24 dev the_internet

# Create veth pairs
ip link add relay_pub_veth type veth peer name relay_veth
ip link add listen_pub_veth type veth peer name listener_veth
ip link add dialer_pub_veth type veth peer name dialer_veth

# Attach one side of veth pairs to the internet
brctl addif the_internet relay_pub_veth
brctl addif the_internet listen_pub_veth
brctl addif the_internet dialer_pub_veth

# Bring up veths
ip link set up dev relay_pub_veth
ip link set up dev listen_pub_veth
ip link set up dev dialer_pub_veth

# Move the other side of veth pairs to network namespaces
ip link set relay_veth netns relay_ns
ip link set listener_veth netns listener_ns
ip link set dialer_veth netns dialer_ns

# Bring up veths in namespaces
ip netns exec relay_ns ip link set up dev relay_veth
ip netns exec listener_ns ip link set up dev listener_veth
ip netns exec dialer_ns ip link set up dev dialer_veth

# Assign IP addresses to veths in namespaces
ip addr add 10.0.0.1/24 dev relay_pub_veth
ip netns exec relay_ns ip addr add 192.168.1.1/24 dev relay_veth

ip addr add 10.0.0.2/24 dev listen_pub_veth
ip netns exec listener_ns ip addr add 192.168.254.1/24 dev listener_veth

ip addr add 10.0.0.3/24 dev dialer_pub_veth
ip netns exec dialer_ns ip addr add 172.16.0.1/24 dev dialer_veth

# Enable IP forwarding
echo 1 > /proc/sys/net/ipv4/ip_forward
ip netns exec dialer_ns bash -c 'echo 1 > /proc/sys/net/ipv4/ip_forward'
ip netns exec listener_ns bash -c 'echo 1 > /proc/sys/net/ipv4/ip_forward'

## Start firewalld
#systemctl start firewalld
#
## Add firewalld rules
#firewall-cmd --permanent --direct --passthrough ipv4 -t nat -I POSTROUTING -o listener_veth -j MASQUERADE
#firewall-cmd --permanent --direct --passthrough ipv4 -t nat -I POSTROUTING -o dialer_veth -j MASQUERADE
#firewall-cmd --permanent --direct --add-rule ipv4 nat POSTROUTING 0 -s 192.168.2.0/24 -d 192.168.1.0/24 -j SNAT --to-source 192.168.1.2
#firewall-cmd --permanent --direct --add-rule ipv4 nat POSTROUTING 0 -s 192.168.3.0/24 -d 192.168.1.0/24 -j SNAT --to-source 192.168.1.2
#firewall-cmd --permanent --direct --add-rule ipv4 nat POSTROUTING 0 -s 192.168.1.0/24 -d 192.168.2.0/24 -j SNAT --to-source 192.168.2.2
#firewall-cmd --permanent --direct --add-rule ipv4 nat POSTROUTING 0 -s 192.168.3.0/24 -d 192.168.2.0/24 -j SNAT --to-source 192.168.2.2
#firewall-cmd --permanent --direct --add-rule ipv4 nat POSTROUTING 0 -s 192.168.1.0/24 -d 192.168.3.0/24 -j SNAT --to-source 192.168.3.2
#firewall-cmd --permanent --direct --add-rule ipv4 nat POSTROUTING 0 -s 192.168.2.0/24 -d 192.168.3.0/24 -j SNAT --to-source 192.168.3.2
#
## Reload firewalld rules
#firewall-cmd --reload
#
## Set up routes in the relay namespace
#ip netns exec relay_ns ip route add 192.168.2.0/24 via 192.168.1.1
#ip netns exec relay_ns ip route add 192.168.3.0/24 via 192.168.1.1
#
## Set up routes in namespaces
#ip netns exec listener_ns ip route add 192.168.1.0/24 via 192.168.2.1
#ip netns exec listener_ns ip route add 192.168.3.0/24 via 192.168.2.1
#ip netns exec dialer_ns ip route add 192.168.1.0/24 via 192.168.3.1
#ip netns exec dialer_ns ip route add 192.168.2.0/24 via 192.168.3.1
