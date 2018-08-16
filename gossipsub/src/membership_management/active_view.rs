// ### Active View Management

// Optimizations TODO: The active list is generally managed reactively: failures are detected
// by TCP, either when a message is sent or when the connection is detected
// as closed.

// In addition to the reactive management strategy, the active list has
// stabilization and optimization components that run periodically with a
// randomized timer, and also serve as failure detectors. The
// stabilization component attempts to prune active lists that are larger
// than A, say because of a slew of recent joins, and grow active lists
// that are smaller than A because of some failures or previous inability
// to neighbor with enough nodes.

// When a node detects that its active list is too large, it queries the neighbors
// for their active lists.
// - If some neighbors have more than `C_rand` random neighbors, then links can be dropped
//   with a `DISCONNECT` message until the size of the active list is A again.
// - If the list is still too large, then it checks the active lists for neighbors that
//   are connected with each other. In this case, one of the links can be dropped
//   with a `DISCONNECT` message.
// - If the list is still too large, then we cannot safely drop connections and it will
//   remain that large until the next stabilization period.

// When a node detects that its active list is too small, then it tries
// to open more connections by picking nodes from its passive list, as
// described in the Join section.

// The optimization component tries to optimize the `C_near` connections
// by replacing links with closer nodes. In order to do so, it takes RTT
// samples from active list nodes and maintains a smoothed running
// average. The neighbors are reordered by RTT and the closest ones are
// considered the near nodes. It then checks the RTT samples of passive
// list nodes and selects the closest node.  If the RTT is smaller by a
// factor of alpha than a near neighbor and it has enough random
// neighbors, then it disconnects and adopts the new node from the
// passive list as a neighbor.
