Implementation of the `/ipfs/id/1.0.0` protocol. Allows a node A to query another node B which
information B knows about A. Also includes the addresses B is listening on.

When two nodes connect to each other, the listening half sends a message to the dialing half,
indicating the information, and then the protocol stops.

# Usage

Both low-level and high-level usages are available.

## High-level usage through the `IdentifyTransport` struct

This crate provides the `IdentifyTransport` struct, which wraps around a `Transport` and an
implementation of `Peerstore`. `IdentifyTransport` is itself a transport that accepts
multiaddresses of the form `/p2p/...` or `/ipfs/...`.

> **Note**: All the documentation refers to `/p2p/...`, however `/ipfs/...` is also supported.

If you dial a multiaddr of the form `/p2p/...`, then the `IdentifyTransport` will look into
the `Peerstore` for any known multiaddress for this peer and try to dial them using the
underlying transport. If you dial any other multiaddr, then it will dial this multiaddr using
the underlying transport, then negotiate the *identify* protocol with the remote in order to
obtain its ID, then add it to the peerstore, and finally dial the same multiaddr again and
return the connection.

Listening doesn't support multiaddresses of the form `/p2p/...` (because that wouldn't make
sense). Any address passed to `listen_on` will be passed directly to the underlying transport.

Whenever a remote connects to us, either through listening or through `next_incoming`, the
`IdentifyTransport` dials back the remote, upgrades the connection to the *identify* protocol
in order to obtain the ID of the remote, stores the information in the peerstore, and finally
only returns the connection. From the exterior, the multiaddress of the remote is of the form
`/p2p/...`. If the remote doesn't support the *identify* protocol, then the socket is closed.

Because of the behaviour of `IdentifyProtocol`, it is recommended to build it on top of a
`ConnectionReuse`.

## Low-level usage through the `IdentifyProtocolConfig` struct

The `IdentifyProtocolConfig` struct implements the `ConnectionUpgrade` trait. Using it will
negotiate the *identify* protocol.

The output of the upgrade is a `IdentifyOutput`. If we are the dialer, then `IdentifyOutput`
will contain the information sent by the remote. If we are the listener, then it will contain
a `IdentifySender` struct that can be used to transmit back to the remote the information about
it.
