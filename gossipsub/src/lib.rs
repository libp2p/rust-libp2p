// This is free and unencumbered software released into the public domain.

// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.

// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.

// For more information, please refer to <http://unlicense.org/>

// WIP!

extern crate bs58;
extern crate futures;
extern crate libp2p_core;
extern crate libp2p_floodsub;
extern crate libp2p_kad;
extern crate libp2p_ping;
extern crate libp2p_secio;
extern crate libp2p_tcp_transport;
extern crate protobuf;
extern crate time;
extern crate tokio_core;
extern crate tokio_current_thread; 

mod rpc_proto;
mod topic;
mod constants;

pub mod membership_management;

// ## Differences from Plumtree/HyParView

// There are some noteworthy differences in the protocol described and
// the published Plumtree/HyParView protocols. There might be some more
// differences in minor details, but this document is written from a
// practical implementer's point of view.

// Membership Management protocol:
// - The node views are managed with proximity awareness. The HyParView protocol
//   has no provisions for proximity, these come from GoCast's implementation
//   of proximity aware overlays; but note that we don't use UDP for RTT measurements
//   and the increased `C_rand` to increase fault-tolerance at the price of some optimization.
// - Joining nodes don't get to get all A connections by kicking out extant nodes,
//   as this would result in overlay instability in periods of high churn. Instead, nodes
//   ensure that the first few links are created even if they oversubscribe their fanout, but they
//   don't go out of their way to create remaining links beyond the necessary `C_rand` links.
//   Nodes later bring the active list to balance with a stabilization protocol.
//   Also noteworthy is that only `C_rand` `JOIN` messages are propagated with a random walk; the
//   remaining joins are considered near joins and handled with normal `NEIGHBOR` requests.
//   In short, the Join protocol is reworked, with the influence of GoCast.
// - There is no active view stabilization/optimization protocol in HyParView. This is very
//   much influenced from GoCast, where the protocol allows oversubscribing and later drops
//   extraneous connections and replaces nodes for proximity optimization.
// - `NEIGHBOR` messages play a dual role in the proposed protocol implementation, as they can
//   be used for establishing active links and retrieving membership lists.
// - There is no connectivity check in HyParView and retires with reduced TTLs, but this
//   is incredibly important in a world full of NAT.
// - There is no `LEAVE` provision in HyParView.

// Broadcast protocol:
// - `IHAVE` messages are aggregated and lazily pushed via a background timer. Plumtree eagerly
//   pushes `IHAVE` messages, which is wasteful and loses the opportunity for aggregation.
//   The authors do suggest lazy aggregation as a possible optimization nonetheless.
// - `GRAFT` messages similarly aggregate multiple message requests.
// - Missing messages and overlay repair are managed by a single background timer instead of
//   of creating timers left and right for every missing message; that's impractical from an
//   implementation point of view, at least in Go.
// - There is no provision for eager overlay repair on `NeighborDown` messages in Plumtree."
