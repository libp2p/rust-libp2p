// ## Protocol Messages

// A quick summary of referenced protocol messages and their payload.
// All messages are assumed to be enclosed in a suitable envelope and have
// a source and monotonic sequence id.

// in gerbil
// ```
// ;; Initial node discovery
// GETNODES {}

// NODES {
//  peers []peer.ID
//  ttl int
// }

// ;; Topic querying (membership check for passive view management)
// GETTOPICS {}

// TOPICS {
//  topics []topic.ID
// }

// ;; Membership Management protocol
// JOIN {
//  peer peer.ID
//  ttl int
// }

// FORWARDJOIN {
//  peer peer.ID
//  ttl int
// }

// NEIGHBOR {
//  peers []peer.ID
// }

// DISCONNECT {}

// LEAVE {
//  source peer.ID
//  ttl int
// }

// SHUFFLE {
//  peer peer.ID
//  peers []peer.ID
//  ttl int
// }

// SHUFFLEREPLY {
//  peers []peer.ID
// }

// ;; Broadcast protocol
// GOSSIP {
//  source peer.ID
//  hops int
//  msg []bytes
// }

// IHAVE {
//  summary []MessageSummary
// }

// MessageSummary {
//  id message.ID
//  hops int
// }

// PRUNE {}

// GRAFT {
//  msgs []message.ID
// }

// ```