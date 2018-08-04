// ### Multicast Tree Repair

// When a failure occurs, at least one multicast tree branch is affected,
// as messages are not transmitted by eager push.  The `IHAVE` messages
// exchanged through lazy gossip are used both to recover missing messages
// but also to provide a quick mechanism to heal the multicast tree.

// When a node receives an `IHAVE` message for unknown messages, it
// simply marks the messages as missing and places them to the missing
// message queue. It then starts a timer and waits to receive the message
// with eager push before the timer expires. The timer duration is a
// protocol parameter that should be configured considering the diameter
// of the overlay and the target recovery latency. A more realistic
// implementation is to use a persistent timer heartbeat to check for
// missing messages periodically, marking on first touch and considered
// missing on the second timer touch.

// When a message is detected as missing, the node selects the first
// `IHAVE` announcement it has seen for the missing message and sends a
// `GRAFT` message to the peer, piggybacking other missing messages. The
// `GRAFT` message serves a dual purpose: it triggers the transmission of
// the missing messages and at the same time adds the link to the
// multicast tree, healing it.

// Upon receiving a `GRAFT` message, a node adds the peer to the eager
// list and transmits the missing messages from its cache as `GOSSIP`.
// Note that the message is not removed from the missing list until it is
// received as a response to a `GRAFT`. If the message has not been
// received by the next timer tick, say because the grafted peer has
// also failed, then another graft is attempted and so on, until enough
// ticks have elapsed to consider the message lost.