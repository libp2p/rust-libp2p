## Description

A basic chat application with logs demonstrating libp2p and the gossipsub protocol combined with mDNS for the discovery of peers to gossip with. It showcases how peers can connect, discover each other using mDNS, and engage in real-time chat sessions.

## Tutorial

1. Using two terminal windows, start two instances, typing the following in each:
	```sh
	cargo run
	```

2. Mutual mDNS discovery may take a few seconds. When each peer does discover the other
it will print a message like:
	```sh
	mDNS discovered a new peer: {peerId}
	```

3. Type a message and hit return: the message is sent and printed in the other terminal.

4. Close with `Ctrl-c`. You can open more terminal windows and add more peers using the same line above.

Once an additional peer is mDNS discovered it can participate in the conversation and all peers will receive messages sent from it.

If a participant exits (`Ctrl-c` or otherwise) the other peers will receive an mDNS expired event and remove the expired peer from the list of known peers.
