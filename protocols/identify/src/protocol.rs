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

use bytes::{Bytes, BytesMut};
use futures::{future::{self, FutureResult}, Async, AsyncSink, Future, Poll, Sink, Stream};
use libp2p_core::{
    Multiaddr, PublicKey,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo}
};
use log::{debug, trace};
use protobuf::Message as ProtobufMessage;
use protobuf::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::RepeatedField;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use structs_proto;
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

/// Configuration for an upgrade to the identity protocol.
#[derive(Debug, Clone)]
pub struct IdentifyProtocolConfig;

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    /// Information about the remote.
    pub info: IdentifyInfo,
    /// Address the remote sees for us.
    pub observed_addr: Multiaddr,

    _priv: ()
}

/// Object used to send back information to the client.
pub struct IdentifySender<T> {
    inner: Framed<T, codec::UviBytes<Vec<u8>>>,
}

impl<T> IdentifySender<T> where T: AsyncWrite {
    /// Sends back information to the client. Returns a future that is signalled whenever the
    /// info have been sent.
    pub fn send(self, info: IdentifyInfo, observed_addr: &Multiaddr) -> IdentifySenderFuture<T> {
        debug!("Sending identify info to client");
        trace!("Sending: {:?}", info);

        let listen_addrs = info.listen_addrs
            .into_iter()
            .map(|addr| addr.into_bytes())
            .collect();

        let mut message = structs_proto::Identify::new();
        message.set_agentVersion(info.agent_version);
        message.set_protocolVersion(info.protocol_version);
        message.set_publicKey(info.public_key.into_protobuf_encoding());
        message.set_listenAddrs(listen_addrs);
        message.set_observedAddr(observed_addr.to_bytes());
        message.set_protocols(RepeatedField::from_vec(info.protocols));

        let bytes = message
            .write_to_bytes()
            .expect("writing protobuf failed; should never happen");

        IdentifySenderFuture {
            inner: self.inner,
            item: Some(bytes),
        }
    }
}

/// Future returned by `IdentifySender::send()`. Must be processed to the end in order to send
/// the information to the remote.
// Note: we don't use a `futures::sink::Sink` because it requires `T` to implement `Sink`, which
//       means that we would require `T: AsyncWrite` in this struct definition. This requirement
//       would then propagate everywhere.
#[must_use = "futures do nothing unless polled"]
pub struct IdentifySenderFuture<T> {
    /// The Sink where to send the data.
    inner: Framed<T, codec::UviBytes<Vec<u8>>>,
    /// Bytes to send, or `None` if we've already sent them.
    item: Option<Vec<u8>>,
}

impl<T> Future for IdentifySenderFuture<T>
where T: AsyncWrite
{
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(item) = self.item.take() {
            if let AsyncSink::NotReady(item) = self.inner.start_send(item)? {
                self.item = Some(item);
                return Ok(Async::NotReady);
            }
        }

        try_ready!(self.inner.poll_complete());
        Ok(Async::Ready(()))
    }
}

/// Information sent from the listener to the dialer.
#[derive(Debug, Clone)]
pub struct IdentifyInfo {
    /// Public key of the node.
    pub public_key: PublicKey,
    /// Version of the "global" protocol, e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the client. Can be thought as similar to the `User-Agent` header
    /// of HTTP.
    pub agent_version: String,
    /// Addresses that the node is listening on.
    pub listen_addrs: Vec<Multiaddr>,
    /// Protocols supported by the node, e.g. `/ipfs/ping/1.0.0`.
    pub protocols: Vec<String>,

    _priv: ()
}

impl UpgradeInfo for IdentifyProtocolConfig {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/ipfs/id/1.0.0"), ()))
    }
}

impl<C> InboundUpgrade<C> for IdentifyProtocolConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = IdentifySender<C>;
    type Error = IoError;
    type Future = FutureResult<Self::Output, IoError>;

    fn upgrade_inbound(self, socket: C, _: ()) -> Self::Future {
        trace!("Upgrading inbound connection");
        let socket = Framed::new(socket, codec::UviBytes::default());
        let sender = IdentifySender { inner: socket };
        future::ok(sender)
    }
}

impl<C> OutboundUpgrade<C> for IdentifyProtocolConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = RemoteInfo;
    type Error = IoError;
    type Future = IdentifyOutboundFuture<C>;

    fn upgrade_outbound(self, socket: C, _: ()) -> Self::Future {
        IdentifyOutboundFuture {
            inner: Framed::new(socket, codec::UviBytes::<BytesMut>::default()),
        }
    }
}

/// Future returned by `OutboundUpgrade::upgrade_outbound`.
pub struct IdentifyOutboundFuture<T> {
    inner: Framed<T, codec::UviBytes<BytesMut>>,
}

impl<T> Future for IdentifyOutboundFuture<T>
where T: AsyncRead
{
    type Item = RemoteInfo;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let msg = match try_ready!(self.inner.poll()) {
            Some(i) => i,
            None => {
                debug!("Identify protocol stream closed before receiving info");
                return Err(IoErrorKind::InvalidData.into());
            }
        };

        debug!("Received identify message");

        let (info, observed_addr) = match parse_proto_msg(msg) {
            Ok(v) => v,
            Err(err) => {
                debug!("Failed to parse protobuf message; error = {:?}", err);
                return Err(err.into());
            }
        };

        trace!("Remote observes us as {:?}", observed_addr);
        trace!("Information received: {:?}", info);

        Ok(Async::Ready(RemoteInfo {
            info,
            observed_addr: observed_addr.clone(),
            _priv: ()
        }))
    }
}

// Turns a protobuf message into an `IdentifyInfo` and an observed address. If something bad
// happens, turn it into an `IoError`.
fn parse_proto_msg(msg: BytesMut) -> Result<(IdentifyInfo, Multiaddr), IoError> {
    match protobuf_parse_from_bytes::<structs_proto::Identify>(&msg) {
        Ok(mut msg) => {
            // Turn a `Vec<u8>` into a `Multiaddr`. If something bad happens, turn it into
            // an `IoError`.
            fn bytes_to_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, IoError> {
                Multiaddr::from_bytes(bytes)
                    .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
            }

            let listen_addrs = {
                let mut addrs = Vec::new();
                for addr in msg.take_listenAddrs().into_iter() {
                    addrs.push(bytes_to_multiaddr(addr)?);
                }
                addrs
            };

            let observed_addr = bytes_to_multiaddr(msg.take_observedAddr())?;
            let info = IdentifyInfo {
                public_key: PublicKey::from_protobuf_encoding(msg.get_publicKey())?,
                protocol_version: msg.take_protocolVersion(),
                agent_version: msg.take_agentVersion(),
                listen_addrs: listen_addrs,
                protocols: msg.take_protocols().into_vec(),
                _priv: ()
            };

            Ok((info, observed_addr))
        }

        Err(err) => Err(IoError::new(IoErrorKind::InvalidData, err)),
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio;

    use self::tokio::runtime::current_thread::Runtime;
    use self::libp2p_tcp_transport::TcpConfig;
    use futures::{Future, Stream};
    use libp2p_core::{PublicKey, Transport, upgrade::{apply_outbound, apply_inbound}};
    use std::sync::mpsc;
    use std::thread;
    use {IdentifyInfo, RemoteInfo, IdentifyProtocolConfig};

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send info from the server to the client, and check that
        // they were successfully received.

        let (tx, rx) = mpsc::channel();

        let bg_thread = thread::spawn(move || {
            let transport = TcpConfig::new();

            let (listener, addr) = transport
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();

            tx.send(addr).unwrap();

            let future = listener
                .into_future()
                .map_err(|(err, _)| err)
                .and_then(|(client, _)| client.unwrap().0)
                .and_then(|socket| {
                    apply_inbound(socket, IdentifyProtocolConfig).map_err(|e| e.into_io_error())
                })
                .and_then(|sender| {
                    sender.send(
                        IdentifyInfo {
                            public_key: PublicKey::Ed25519(vec![1, 2, 3, 4, 5, 7]),
                            protocol_version: "proto_version".to_owned(),
                            agent_version: "agent_version".to_owned(),
                            listen_addrs: vec![
                                "/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                                "/ip6/::1/udp/1000".parse().unwrap(),
                            ],
                            protocols: vec!["proto1".to_string(), "proto2".to_string()],
                            _priv: ()
                        },
                        &"/ip4/100.101.102.103/tcp/5000".parse().unwrap(),
                    )
                });
            let mut rt = Runtime::new().unwrap();
            let _ = rt.block_on(future).unwrap();
        });

        let transport = TcpConfig::new();

        let future = transport.dial(rx.recv().unwrap())
            .unwrap_or_else(|_| panic!())
            .and_then(|socket| {
                apply_outbound(socket, IdentifyProtocolConfig).map_err(|e| e.into_io_error())
            })
            .and_then(|RemoteInfo { info, observed_addr, .. }| {
                assert_eq!(observed_addr, "/ip4/100.101.102.103/tcp/5000".parse().unwrap());
                assert_eq!(info.public_key, PublicKey::Ed25519(vec![1, 2, 3, 4, 5, 7]));
                assert_eq!(info.protocol_version, "proto_version");
                assert_eq!(info.agent_version, "agent_version");
                assert_eq!(info.listen_addrs,
                    &["/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                      "/ip6/::1/udp/1000".parse().unwrap()]);
                assert_eq!(info.protocols, &["proto1".to_string(), "proto2".to_string()]);
                Ok(())
            });

        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
        bg_thread.join().unwrap();
    }
}
