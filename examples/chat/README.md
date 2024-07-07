## Description

A basic chat application with logs demonstrating libp2p and the gossipsub protocol combined with mDNS for the discovery of peers to gossip with.
It showcases how peers can connect, discover each other using mDNS, and engage in real-time chat sessions.

## Usage

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

When a new peer is discovered through mDNS, it can join the conversation, and all peers will receive messages sent by that peer.
If a participant exits the application using `Ctrl-c` or any other method, the remaining peers will receive an mDNS expired event and remove the expired peer from their list of known peers.

## Conclusion

This chat application demonstrates the usage of **libp2p** and the gossipsub protocol for building a decentralized chat system.
By leveraging mDNS for peer discovery, users can easily connect with other peers and engage in real-time conversations.
The example provides a starting point for developing more sophisticated chat applications using **libp2p** and exploring the capabilities of decentralized communication.
