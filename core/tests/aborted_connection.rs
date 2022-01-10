// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use std::task::Context;

use libp2p_core::{
    connection::{
        pool::{ConnectionLimits, Pool, PoolConfig},
        ConnectionHandler,
    },
    transport::dummy::DummyTransport,
    Multiaddr, PeerId,
};

mod util;

#[test]
fn aborted_connection() {
    let pool = Pool::new(
        PeerId::random(),
        PoolConfig::default(),
        ConnectionLimits::default(),
    );
    let target_peer = PeerId::random();
    pool.add_outgoing(
        DummyTransport::default(),
        ["/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap()],
        Some(target_peer),
        util::TestHandler(),
    );
    pool.disconnect(&target_peer);
    let (cx, handle) = Context::new();
    pool.poll(cx);
}
