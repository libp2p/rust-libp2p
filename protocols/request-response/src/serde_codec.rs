// Copyright 2020 Parity Technologies (UK) Ltd.
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

use async_trait::async_trait;
use futures::prelude::*;
use std::{
    io, marker::PhantomData,
    task::{Context, Poll},
};
use serde::{Serialize, de::DeserializeOwned};
use crate::{
    Behaviour as RequestResponseBehaviour,
    Codec, Config, ProtocolSupport, Handler,
    Event, RequestId, ResponseChannel,
};
use libp2p_identity::PeerId;
use libp2p_core::{
    Endpoint, Multiaddr,
    upgrade::{read_length_prefixed, write_length_prefixed},
};
use libp2p_swarm::{
    StreamProtocol,
    behaviour::{FromSwarm},
    ConnectionDenied, ConnectionId, NetworkBehaviour, PollParameters, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};

const REQUEST_MAXIMUM: usize = 1_000_000;
const RESPONSE_MAXIMUM: usize = 500_000_000;

#[derive(Debug, Clone)]
enum SerdeFormat {
    JSON,
    CBOR,
}

#[derive(Debug, Clone)]
pub struct SerdeCodec<Req, Resp> {
    format: SerdeFormat,
    phantom: PhantomData<(Req, Resp)>,
}

pub struct Behaviour<Req, Resp>
    where
        Req: Send + Clone + Serialize + DeserializeOwned + 'static,
        Resp: Send + Clone + Serialize + DeserializeOwned + 'static,
{
    original: RequestResponseBehaviour<SerdeCodec<Req, Resp>>,
    phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> SerdeCodec<Req, Resp>
    where
        Req: Serialize + DeserializeOwned,
        Resp: Serialize + DeserializeOwned,
{
    pub(crate) fn json() -> Self {
        SerdeCodec { format: SerdeFormat::JSON, phantom: PhantomData }
    }

    pub(crate) fn cbor() -> Self {
        SerdeCodec { format: SerdeFormat::CBOR, phantom: PhantomData }
    }

    fn val_to_vec<S: Serialize>(&self, val: &S) -> io::Result<Vec<u8>> {
        match self.format {
            SerdeFormat::JSON => {
                serde_json::to_vec(val)
                    .map_err(|_| io::ErrorKind::Other.into())
            },
            SerdeFormat::CBOR => {
                serde_cbor::to_vec(val)
                    .map_err(|_| io::ErrorKind::Other.into())
            },
        }
    }

    fn val_from_slice<D: DeserializeOwned>(&self, vec: &[u8]) -> io::Result<D> {
        match self.format {
            SerdeFormat::JSON => {
                serde_json::from_slice(vec)
                    .map_err(|_| io::ErrorKind::Other.into())
            },
            SerdeFormat::CBOR => {
                serde_cbor::from_slice(vec)
                    .map_err(|_| io::ErrorKind::Other.into())
            },
        }
    }
}

#[async_trait]
impl<Req, Resp> Codec for SerdeCodec<Req, Resp>
    where
        Req: Send + Clone + Serialize + DeserializeOwned,
        Resp: Send + Clone + Serialize + DeserializeOwned,
{
    type Protocol = StreamProtocol;
    type Request = Req;
    type Response = Resp;

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Req>
        where
            T: AsyncRead + Unpin + Send
    {
        let vec = read_length_prefixed(io, REQUEST_MAXIMUM).await?;

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        self.val_from_slice(vec.as_slice())
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
        where
            T: AsyncRead + Unpin + Send
    {
        let vec = read_length_prefixed(io, RESPONSE_MAXIMUM).await?;

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        self.val_from_slice(vec.as_slice())
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send
    {
        let data = self.val_to_vec(&req)?;
        write_length_prefixed(io, data).await?;
        io.close().await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send
    {
        let data = self.val_to_vec(&res)?;
        write_length_prefixed(io, data).await?;
        io.close().await?;

        Ok(())
    }
}

impl<Req, Resp> Behaviour<Req, Resp>
    where
        Req: Send + Clone + Serialize + DeserializeOwned,
        Resp: Send + Clone + Serialize + DeserializeOwned,
{
    pub(crate) fn new<I>(
        codec: SerdeCodec<Req, Resp>,
        protocols: I,
        cfg: Config) -> Behaviour<Req, Resp>
        where
            I: IntoIterator<Item=(<SerdeCodec<Req, Resp> as Codec>::Protocol, ProtocolSupport)>,
    {
        Behaviour {
            original: RequestResponseBehaviour::new(codec, protocols, cfg),
            phantom: PhantomData,
        }
    }


    /// Initiates sending a request.
    ///
    /// If the targeted peer is currently not connected, a dialing
    /// attempt is initiated and the request is sent as soon as a
    /// connection is established.
    ///
    /// > **Note**: In order for such a dialing attempt to succeed,
    /// > the `RequestResonse` protocol must either be embedded
    /// > in another `NetworkBehaviour` that provides peer and
    /// > address discovery, or known addresses of peers must be
    /// > managed via [`Behaviour::add_address`] and
    /// > [`Behaviour::remove_address`].
    pub fn send_request(&mut self, peer: &PeerId, request: Req) -> RequestId {
        self.original.send_request(peer, request)
    }

    /// Initiates sending a response to an inbound request.
    ///
    /// If the [`ResponseChannel`] is already closed due to a timeout or the
    /// connection being closed, the response is returned as an `Err` for
    /// further handling. Once the response has been successfully sent on the
    /// corresponding connection, [`Event::ResponseSent`] is
    /// emitted. In all other cases [`Event::InboundFailure`]
    /// will be or has been emitted.
    ///
    /// The provided `ResponseChannel` is obtained from an inbound
    /// [`Message::Request`].
    pub fn send_response(
        &mut self,
        ch: ResponseChannel<Resp>,
        rs: Resp,
    ) -> Result<(), Resp> {
        ch.sender.send(rs)
    }

    /// Adds a known address for a peer that can be used for
    /// dialing attempts by the `Swarm`, i.e. is returned
    /// by [`NetworkBehaviour::handle_pending_outbound_connection`].
    ///
    /// Addresses added in this way are only removed by `remove_address`.
    pub fn add_address(&mut self, peer: &PeerId, address: Multiaddr) {
        self.original.add_address(peer, address)
    }

    /// Removes an address of a peer previously added via `add_address`.
    pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) {
        self.original.remove_address(peer, address)
    }

    /// Checks whether a peer is currently connected.
    pub fn is_connected(&self, peer: &PeerId) -> bool {
        self.original.is_connected(peer)
    }

    /// Checks whether an outbound request to the peer with the provided
    /// [`PeerId`] initiated by [`Behaviour::send_request`] is still
    /// pending, i.e. waiting for a response.
    pub fn is_pending_outbound(&self, peer: &PeerId, request_id: &RequestId) -> bool {
        self.original.is_pending_outbound(peer, request_id)
    }

    /// Checks whether an inbound request from the peer with the provided
    /// [`PeerId`] is still pending, i.e. waiting for a response by the local
    /// node through [`Behaviour::send_response`].
    pub fn is_pending_inbound(&self, peer: &PeerId, request_id: &RequestId) -> bool {
        self.original.is_pending_inbound(peer, request_id)
    }
}

impl<Req, Resp> NetworkBehaviour for Behaviour<Req, Resp>
where
    Req: Send + Clone + Serialize + DeserializeOwned,
    Resp: Send + Clone + Serialize + DeserializeOwned,
{
    type ConnectionHandler = Handler<SerdeCodec<Req, Resp>>;
    type OutEvent = Event<Req, Resp>;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.original.handle_established_inbound_connection(
            _connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.original.handle_established_outbound_connection(
            _connection_id,
            peer,
            addr,
            role_override,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        self.original.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        _event: THandlerOutEvent<Self>,
    ) {
        self.original.on_connection_handler_event(
            _peer_id,
            _connection_id,
            _event,
        );
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        self.original.poll(cx, params)
    }
}


