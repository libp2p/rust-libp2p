// ### Passive View Management

// The passive list is managed cyclically, as per [2]. Periodically, with
// a randomized timer, each node performs a passive list shuffle with one
// of its active neighbors. The purpose of the shuffle is to update the
// passive lists of the nodes involved. The node that initiates the shuffle
// creates an exchange list that contains its id, `k_a` peers from its
// active list and `k_p` peers from its passive list, where `k_a` and
// `k_p` are protocol parameters (unspecified in [2]). It then sends a
// `SHUFFLE` request to a random neighbor, which is propagated with a
// random walk with an associated TTL.  If the TTL is greater than 0 and
// the number of nodes in the receiver's active list is greater than 1,
// then it propagates the request further. Otherwise, it selects nodes
// from its passive list at random, sends back a `SHUFFLEREPLY` and
// replaces them with the shuffle contents. The originating node
// receiving the `SHUFFLEREPLY` also replaces nodes in its passive list
// with the contents of the message. Care should be taken for issues 
// with transitive connectivity due to NAT. If
// a node cannot connect to the originating node for a `SHUFFLEREPLY`,
// then it should not perform the shuffle. Similarly, the originating
// node could time out waiting for a shuffle reply and try with again
// with a lower TTL, until a TTL of zero reuses the connection in the
// case of NATed hosts.

// In addition to shuffling, proximity awareness and leave cleanup
// requires that we compute RTT samples and check connectivity to nodes
// in the passive list.  Periodically, the node selects some nodes from
// its passive list at random and tries to open a connection if it
// doesn't already have one. It then checks that the peer is still
// subscribed to the overlay. If the connection attempt is successful and
// the node is still subscribed to the topic, it then updates the RTT
// estimate for the peer in the list with a ping. Otherwise, it removes
// it from the passive list for cleanup.