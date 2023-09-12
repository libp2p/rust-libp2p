#!/bin/sh

set -ex

# The route only needs to be added on the client side.
# This is realistic: Servers on the public don't need to know anything about 
# client's network configuration.
if [ -n "$ROUTER" ]; then
    echo "Adding route"
    ip route add $INTERNET via $ROUTER dev eth0
fi

tcpdump -i eth0 -n -w /dump.pcap &

$COMPONENT
