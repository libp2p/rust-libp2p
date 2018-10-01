// Copyright 2018 Parity Technologies (UK) Ltd.
//
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

use futures::{future, future::Loop as FutLoop, prelude::*};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{negotiate, ConnectionUpgrade, Endpoint};

/// Looping connection upgrade.
///
/// Applies a modifier around a `ConnectionUpgrade`.
/// The `ConnectionUpgrade` is expected to produce a `Loop`. If upgrading produces
/// `Loop::Continue`, then the protocol will be negotiated again on the returned stream.
/// If upgrading produces `Loop::Break`, then the loop will stop.
///
/// This is useful for upgrades that produce a stream over which you want to negotiate a protocol.
///
/// Note that there is a maximum number of looping after which a runtime error is produced, in
/// order to avoid DoS attacks if your code happens to be wrong.
#[inline]
pub fn loop_upg<U>(inner: U) -> LoopUpg<U> {
    LoopUpg { inner }
}

/// Maximum number of loops after which a runtime error is produced.
pub const MAX_LOOPS: u32 = 64;

/// See the documentation of `loop_upg`.
pub enum Loop<State, Socket, Final> {
    /// Looping should continue. `Socket` must implement `AsyncRead` and `AsyncWrite`, and will
    /// be used to continue negotiating a protocol. `State` is passed around and can contain
    /// anything.
    Continue(State, Socket),
    /// Stop looping. `Final` is the output of the `loop_upg`.
    Break(Final),
}

/// Looping connection upgrade.
///
/// See the documentation of `loop_upg`.
#[derive(Debug, Copy, Clone)]
pub struct LoopUpg<Inner> {
    inner: Inner,
}

// TODO: 'static :-/
impl<State, Socket, Inner, Out> ConnectionUpgrade<(State, Socket)>
    for LoopUpg<Inner>
where
    State: Send + 'static,
    Socket: AsyncRead + AsyncWrite + Send + 'static,
    Inner: ConnectionUpgrade<
            (State, Socket),
            Output = Loop<State, Socket, Out>,
        > + Clone
        + Send
        + 'static,
    Inner::NamesIter: Clone + Send + 'static,
    Inner::UpgradeIdentifier: Send,
    Inner::Future: Send,
    Out: Send + 'static,
{
    type NamesIter = Inner::NamesIter;
    type UpgradeIdentifier = Inner::UpgradeIdentifier;

    fn protocol_names(&self) -> Self::NamesIter {
        self.inner.protocol_names()
    }

    type Output = Out;
    type Future = Box<Future<Item = Out, Error = IoError> + Send>;

    fn upgrade(
        self,
        (state, socket): (State, Socket),
        id: Self::UpgradeIdentifier,
        endpoint: Endpoint,
    ) -> Self::Future {
        let inner = self.inner;

        let fut = future::loop_fn(
            (state, socket, id, MAX_LOOPS),
            move |(state, socket, id, loops_remaining)| {
                // When we enter a recursion of the `loop_fn`, a protocol has already been
                // negotiated. So what we have to do is upgrade then negotiate the next protocol
                // (if necessary), and then only continue iteration in the `future::loop_fn`.
                let inner = inner.clone();
                inner
                    .clone()
                    .upgrade((state, socket), id, endpoint)
                    .and_then(move |loop_out| match loop_out {
                        Loop::Continue(state, socket) => {
                            // Produce an error if we reached the recursion limit.
                            if loops_remaining == 0 {
                                return future::Either::B(future::err(IoError::new(
                                    IoErrorKind::Other,
                                    "protocol negotiation maximum recursion limit reached",
                                )));
                            }

                            let nego = negotiate(socket, &inner, endpoint);
                            let fut = nego.map(move |(id, socket)| {
                                FutLoop::Continue((
                                    state,
                                    socket,
                                    id,
                                    loops_remaining - 1,
                                ))
                            });
                            future::Either::A(fut)
                        }
                        Loop::Break(fin) => {
                            future::Either::B(future::ok(FutLoop::Break(fin)))
                        }
                    })
            },
        );

        Box::new(fut) as Box<_>
    }
}
