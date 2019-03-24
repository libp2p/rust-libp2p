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

use crate::message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Type};
use crate::protocol::{send_read, SendReadError, SendReadFuture};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use protobuf::Message as _;
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};

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
        let mut msg = CircuitRelay::new();
        msg.set_field_type(CircuitRelay_Type::HOP);

        let mut dest = CircuitRelay_Peer::new();
        dest.set_id(dest_id.as_bytes().to_vec());
        for a in dest_addresses {
            dest.mut_addrs().push(a.to_vec())
        }
        msg.set_dstPeer(dest);

        RelayProxyRequest {
            message: msg
                .write_to_bytes()
                .expect("all the mandatory fields are always filled; QED"),
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
    TSubstream: AsyncRead + AsyncWrite,
{
    type Output = (upgrade::Negotiated<TSubstream>, TUserData);
    type Error = SendReadError;
    type Future = SendReadFuture<TSubstream, TUserData>;

    fn upgrade_outbound(
        self,
        substream: upgrade::Negotiated<TSubstream>,
        _: Self::Info,
    ) -> Self::Future {
        send_read(substream, self.message, self.user_data)
    }
}
