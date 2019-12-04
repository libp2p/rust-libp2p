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

//! Protocol negotiation strategies for the peer acting as the dialer.

use crate::protocol::{Protocol, ProtocolError, Message, Version};
use futures::prelude::*;
use log::debug;
use std::convert::TryFrom;
use crate::{Negotiated, NegotiationError};

/// Returns a `Future` that negotiates a protocol on the given I/O stream
/// for a peer acting as the _dialer_ (or _initiator_).
///
/// This function is given an I/O stream and a list of protocols and returns a
/// computation that performs the protocol negotiation with the remote. The
/// returned `Future` resolves with the name of the negotiated protocol and
/// a [`Negotiated`] I/O stream.
///
/// The chosen message flow for protocol negotiation depends on the numbers
/// of supported protocols given. That is, this function delegates to
/// [`dialer_select_proto_serial`] or [`dialer_select_proto_parallel`]
/// based on the number of protocols given. The number of protocols is
/// determined through the `size_hint` of the given iterator and thus
/// an inaccurate size estimate may result in a suboptimal choice.
///
/// Within the scope of this library, a dialer always commits to a specific
/// multistream-select protocol [`Version`], whereas a listener always supports
/// all versions supported by this library. Frictionless multistream-select
/// protocol upgrades may thus proceed by deployments with updated listeners,
/// eventually followed by deployments of dialers choosing the newer protocol.
pub async fn dialer_select_proto<R, I>(
    inner: R,
    protocols: I,
    version: Version
) -> Result<(I::Item, Negotiated<R>), NegotiationError>
where
    R: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let iter = protocols.into_iter();
    // We choose between the "serial" and "parallel" strategies based on the number of protocols.
    if iter.size_hint().1.map(|n| n <= 3).unwrap_or(false) {
        dialer_select_proto_serial(inner, iter, version).await
    } else {
        dialer_select_proto_parallel(inner, iter, version).await
    }
}

/// Returns a `Future` that negotiates a protocol on the given I/O stream.
///
/// Just like [`dialer_select_proto`] but always using an iterative message flow,
/// trying the given list of supported protocols one-by-one.
///
/// This strategy is preferable if the dialer only supports a few protocols.
pub async fn dialer_select_proto_serial<R, I>(
    mut io: R,
    protocols: I,
    version: Version
) -> Result<(I::Item, Negotiated<R>), NegotiationError>
where
    R: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let mut protocols = protocols.into_iter().peekable();

    Message::Header(version).encode(&mut io).await?;

    loop {
        let protocol_raw = protocols.next().ok_or(NegotiationError::Failed)?;
        let protocol = Protocol::try_from(protocol_raw.as_ref())?;

        Message::Protocol(protocol.clone()).encode(&mut io).await?;
        debug!("Dialer: Proposed protocol: {}", protocol);

        if protocols.peek().is_some() {
            io.flush().await?;
        } else {
            match version {
                Version::V1 => io.flush().await?,
                Version::V1Lazy => {
                    debug!("Dialer: Expecting proposed protocol: {}", protocol);
                    let io = Negotiated::expecting(io, protocol, version);
                    return Ok((protocol_raw, io));
                }
            }
        }

        loop {
            match Message::decode(&mut io).await? {
                Message::Header(v) if v == version => continue,
                Message::Protocol(ref p) if p.as_ref() == protocol_raw.as_ref() => {
                    debug!("Dialer: Received confirmation for protocol: {}", p);
                    return Ok((protocol_raw, Negotiated::completed(io)))
                }
                Message::NotAvailable => {
                    debug!("Dialer: Received rejection of protocol: {}",
                        String::from_utf8_lossy(protocol.as_ref()));
                    break;
                }
                _ => return Err(ProtocolError::InvalidMessage.into())
            }
        }
    }
}

/// Returns a `Future` that negotiates a protocol on the given I/O stream.
///
/// Just like [`dialer_select_proto`] but always using a message flow that first
/// requests all supported protocols from the remote, selecting the first
/// protocol from the given list of supported protocols that is supported
/// by the remote.
///
/// This strategy may be beneficial if the dialer supports many protocols
/// and it is unclear whether the remote supports one of the first few.
pub async fn dialer_select_proto_parallel<R, I>(
    mut io: R,
    protocols: I,
    version: Version
) -> Result<(I::Item, Negotiated<R>), NegotiationError>
where
    R: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    Message::Header(version).encode(&mut io).await?;
    Message::ListProtocols.encode(&mut io).await?;
    io.flush().await?;
    debug!("Dialer: Requested supported protocols.");

    let protocol = loop {
        match Message::decode(&mut io).await? {
            Message::Header(v) if v == version => {},
            Message::Protocols(supported) => {
                let protocol = protocols
                    .into_iter()
                    .find(|p| supported.iter().any(|s| s.as_ref() == p.as_ref()))
                    .ok_or(NegotiationError::Failed)?;
                debug!("Dialer: Found supported protocol: {}",
                    String::from_utf8_lossy(protocol.as_ref()));
                break protocol
            }
            _ => return Err(ProtocolError::InvalidMessage.into())
        }
    };

    let proto_encoded = Protocol::try_from(protocol.as_ref())?;
    Message::Protocol(proto_encoded.clone()).encode(&mut io).await?;
    debug!("Dialer: Expecting proposed protocol: {}", proto_encoded);

    let io = Negotiated::expecting(io, proto_encoded, version);
    Ok((protocol, io))
}
