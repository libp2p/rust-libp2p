/ ### Active View Changes

// The active peer list is maintained by the Membership Management protocol:
// nodes may be removed because of failure or overlay reorganization, and new
// nodes may be added to the list because of new connections. The Membership
// Management protocol communicates these changes to the broadcast loop via
// `NeighborUp` and `NeighborDown` notifications.

// When a new node is added to the active list, the broadcast loop receives
// a `NeighborUp` notifications, it simply adds the node to the eager peer
// list. On the other hand, when a node is removed with a `NeighborDown`
// notificaiton, the loop has to consider if the node was an eager or lazy
// peer. If the node was a lazy peer, it doesn't need to do anything as the
// departure does not affect the multicast tree. If the node was an eager peer
// however, the loss of that edge may result in a disconnected tree.

// There are two strategies in reaction to the loss of an eager peer. The
// first one is to do nothing, and wait for lazy push to repair the tree
// naturally with `IHAVE` messages in the next message broadcast. This
// might result in delays propagating the next few messages but is
// advocated by the authors in [1]. TODO: An alternative is to eagerly repair
// the tree by promoting lazy peers to eager with empty `GRAFT` messages
// and let the protocol prune duplicate paths naturally with `PRUNE`
// messages in the next message transmission. This may have a bit of
// bandwidth cost, but it is perhaps more appropriate for applications
// that value latency minimization which is the case for many IPFS
// applications.