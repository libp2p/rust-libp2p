// ### Broadcast State

// Once it has joined the overlay, the node starts its main broadcast logic
// loop. The loop receives messages to publish from the application, messages
// published from other nodes, and with notifications from the management
// protocol about new active neighbors and disconnections.

// The state of the broadcast loop consists of two sets of peers, the eager
// and lazy lists, with the eager list initialized to the initial neighbors
// and the lazy list empty. The loop also maintains a time-based cache of
// recent messages, together with a queue of lazy message notifications.
// In addition to the cache, it maintains a list of missing messages
// known by lazy gossip but not yet received through the multicast tree.

// ### Message Propagation and Multicast Tree Construction

// When a node publishes a message, it broadcasts a `GOSSIP` message with
// a hopcount of 1 to all its eager peers, adds the message to the cache,
// and adds the message id to the lazy notification queue.

// When a node receives a `GOSSIP` message from a neighbor, first it
// checks its cache to see if it has already seen this message. If the
// message is in the cache, it prunes the edge of the multicast graph by
// sending a `PRUNE` message to the peer, removing the peer from the
// eager list, and adding it to the lazy list.

// If the node hasn't seen the message before, it delivers the message to
// the application and then adds the peer to the eager list and proceeds
// to broadcast. The hopcount is incremented and then the node forwards
// it to its eager peers, excluding the source. It also adds the message
// to the cache, and pushes the message id to the lazy notification queue.

// The loop runs a short periodic timer, with a period in the order of
// 0.1s for gossiping message summaries. Every time it fires, the node
// flushes the lazy notification queue with all the recently received
// message ids in an `IHAVE` message to its lazy peers.  The `IHAVE`
// notifications summarize recent messages the node has seen and have not
// propagated through the eager links.