// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::message_proto::{circuit_relay, CircuitRelay};
use crate::protocol::{send_read, SendReadError, SendReadFuture};
use futures::future::BoxFuture;
use futures::prelude::*;
use futures_codec::Framed;
use libp2p_core::{upgrade, Multiaddr, PeerId};
use prost::Message;
use std::iter;
use unsigned_varint::codec::UviBytes;

/// Ask a remote to act as a relay.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*,
/// this struct is the message that the *source* sends to the *relay* at initialization. The
/// parameters passed to `RelayProxyRequest::new()` are the information of the *destination*
/// (not the information of the *relay*).
///
/// The upgrade should be performed on a substream to the *relay*.
///
/// If the upgrade succeeds, the substream is returned and is now a brand new connection pointing
/// to the *destination*.
// TODO: debug
pub struct RelayProxyRequest<TUserData> {
    /// The message to send to the relay. Pre-computed.
    message: Vec<u8>,
    /// User data that is passed back to the user.
    user_data: TUserData,
}

impl<TUserData> RelayProxyRequest<TUserData> {
    /// Builds a request for the target to act as a relay to a third party.
    ///
    /// The `user_data` is passed back in the result.
    pub fn new(
        dest_id: PeerId,
        dest_addresses: impl IntoIterator<Item = Multiaddr>,
        user_data: TUserData,
    ) -> Self {
        let message = CircuitRelay {
            r#type: Some(circuit_relay::Type::Hop.into()),
            src_peer: None,
            dst_peer: Some(circuit_relay::Peer {
                id: dest_id.as_bytes().to_vec(),
                addrs: dest_addresses.into_iter().map(|a| a.to_vec()).collect(),
            }),
            code: None,
        };
        let mut encoded = Vec::new();
        // TODO: Handle failure?
        message
            .encode(&mut encoded)
            .expect("all the mandatory fields are always filled; QED");

        RelayProxyRequest {
            message: encoded,
            user_data,
        }
    }
}

impl<TUserData> upgrade::UpgradeInfo for RelayProxyRequest<TUserData> {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/libp2p/relay/circuit/0.1.0")
    }
}

impl<TSubstream, TUserData> upgrade::OutboundUpgrade<TSubstream> for RelayProxyRequest<TUserData>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    TUserData: Send + 'static,
{
    type Output = (TSubstream, TUserData);
    type Error = SendReadError;
    type Future = BoxFuture<'static, Result<(TSubstream, TUserData), SendReadError>>;

    fn upgrade_outbound(self, substream: TSubstream, _: Self::Info) -> Self::Future {
        // TODO: Needed?
        let RelayProxyRequest { message, user_data } = self;

        let codec = UviBytes::default();
        // TODO: Do we need this?
        // codec.set_max_len(self.max_packet_size);

        let mut substream = Framed::new(substream, codec);

        async move {
            substream.send(std::io::Cursor::new(message)).await.unwrap();
            let msg = substream.next().await.unwrap().unwrap();

            let msg =std::io::Cursor::new(msg);
            let CircuitRelay {
                r#type,
                src_peer,
                dst_peer,
                code,
            } = CircuitRelay::decode(msg).unwrap();

            if !matches!(circuit_relay::Type::from_i32(r#type.unwrap()).unwrap(), circuit_relay::Type::Status) {
                panic!("expected status");
            }

            if !matches!(circuit_relay::Status::from_i32(code.unwrap()).unwrap(), circuit_relay::Status::Success) {
                panic!("expected success");
            }

            Ok((substream.into_inner(), user_data))
        }
        .boxed()
    }
}
