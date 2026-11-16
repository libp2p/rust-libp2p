// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{Either, FutureExt, Select, select},
    prelude::*,
};
use futures_timer::Delay;
use libp2p_identity::PeerId;

use super::{Connection, connecting::remote_peer_id};
use crate::{ConnectionError, Error};

/// Future that drives an inbound QUIC connection until its handshake completes.
pub enum Incoming {
    /// The connection has not been accepted yet, quinn has no connection object
    /// until [`quinn::Incoming::accept`] runs.
    Accepting {
        incoming: Box<Option<quinn::Incoming>>,
        timeout: Duration,
    },
    /// The connection exists; its handshake is under way.
    Handshaking {
        connecting: Select<quinn::Connecting, Delay>,
    },
}

impl Incoming {
    pub(crate) fn new(incoming: quinn::Incoming, timeout: Duration) -> Self {
        Self::Accepting {
            incoming: Box::new(Some(incoming)),
            timeout,
        }
    }
}

impl Future for Incoming {
    type Output = Result<(PeerId, Connection), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        loop {
            match this {
                Self::Accepting { incoming, timeout } => {
                    let accept = incoming
                        .take()
                        .ok_or(Error::InvalidState("inbound connection already accepted"))?;
                    let connection = accept.accept().map_err(ConnectionError)?;
                    *this = Self::Handshaking {
                        connecting: select(connection, Delay::new(*timeout)),
                    };
                }

                Self::Handshaking { connecting } => {
                    let connection = match futures::ready!(connecting.poll_unpin(cx)) {
                        Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
                        Either::Left((connection, _)) => connection.map_err(ConnectionError)?,
                    };
                    let peer_id = remote_peer_id(&connection)?;
                    let muxer = Connection::new(connection);
                    return Poll::Ready(Ok((peer_id, muxer)));
                }
            }
        }
    }
}
