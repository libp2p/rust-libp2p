// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Protocol negotiation strategies for the peer acting as the listener
//! in a multistream-select protocol negotiation.

use futures::prelude::*;
use crate::protocol::{Protocol, ProtocolError, Message, Version};
use log::{debug, warn};
use smallvec::SmallVec;
use std::convert::TryFrom;
use crate::{Negotiated, NegotiationError};

/// Returns a `Future` that negotiates a protocol on the given I/O stream
/// for a peer acting as the _listener_ (or _responder_).
///
/// This function is given an I/O stream and a list of protocols and returns a
/// computation that performs the protocol negotiation with the remote. The
/// returned `Future` resolves with the name of the negotiated protocol and
/// a [`Negotiated`] I/O stream.
pub async fn listener_select_proto<R, I>(
    io: R,
    protocols: I,
) -> Result<(I::Item, Negotiated<R>), NegotiationError>
where
    R: AsyncRead + AsyncWrite + Unpin,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let protocols = protocols.into_iter().filter_map(|n|
        match Protocol::try_from(n.as_ref()) {
            Ok(p) => Some((n, p)),
            Err(e) => {
                warn!("Listener: Ignoring invalid protocol: {} due to {}",
                      String::from_utf8_lossy(n.as_ref()), e);
                None
            }
        })
        .collect::<SmallVec<[_; 8]>>();

    let version = match Message::decode(&mut io).await? {
        Message::Header(version) => version,
        _ => return Err(ProtocolError::InvalidMessage.into()),
    };

    Message::Header(version).encode(&mut io).await?;
    if let Version::V1 = version {
        io.flush().await?;
    }

    loop {
        match Message::decode(&mut io).await? {
            Message::ListProtocols => {
                let supported = protocols.iter().map(|(_, p)| p.clone()).collect();
                Message::Protocols(supported).encode(&mut io).await?;
            }
            Message::Protocol(p) => {
                let proto_pos = protocols.iter().position(|(_, proto)| &p == proto);

                let proto_pos = if let Some(proto_pos) = proto_pos {
                    proto_pos
                } else {
                    debug!("Listener: rejecting protocol: {}",
                        String::from_utf8_lossy(p.as_ref()));
                    Message::NotAvailable.encode(&mut io).await?;
                    continue;
                };

                let (raw_protocol, protocol) = protocols.remove(proto_pos);

                debug!("Listener: confirming protocol: {}", p);
                Message::Protocol(protocol).encode(&mut io).await?;
                debug!("Listener: sent confirmed protocol: {}",
                    String::from_utf8_lossy(raw_protocol.as_ref()));
                return Ok((raw_protocol, Negotiated::completed(io)));
            }
            _ => return Err(ProtocolError::InvalidMessage.into()),
        }
    }
}
