// Copyright 2018 Parity Technologies (UK) Ltd.
// Copyright 2018 Emotiq AG.
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
use futures::{future, Future, Sink, Stream};
use libp2p_core::{ConnectionUpgrade, Endpoint, PublicKey};
use multiaddr::Multiaddr;
use protobuf::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::Message as ProtobufMessage;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use structs_proto;
use tokio_io::codec::length_delimited;
use tokio_io::{AsyncRead, AsyncWrite};

/// Configuration for an upgrade to the identity protocol.
#[derive(Debug, Clone)]
pub struct NodeIdProtocolConfig {
    /// Public key of the node.
    pub public_key: PublicKey,
    /// Version of the "global" protocol, eg. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the client. Can be thought as similar to the `User-Agent` header
    /// of HTTP.
    pub agent_version: String,
}

/// Output of the connection upgrade.
pub struct NodeIdOutput<S> {
    pub remote_info: NodeInfo,
    /// The socket to communicate with the remote.
    pub socket: S,
}

/// Information sent from the listener to the dialer.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Public key of the node.
    pub public_key: PublicKey,
    /// Version of the "global" protocol, eg. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the client. Can be thought as similar to the `User-Agent` header
    /// of HTTP.
    pub agent_version: String,
}

impl<C, Maf> ConnectionUpgrade<C, Maf> for NodeIdProtocolConfig
where
    C: AsyncRead + AsyncWrite + Send + 'static,
    Maf: Future<Item = Multiaddr, Error = IoError> + Send + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();
    type Output = NodeIdOutput<C>;
    type MultiaddrFuture = Maf;
    type Future = Box<Future<Item = (Self::Output, Maf), Error = IoError> + Send>;

    /// TODO: Proper protocol name
    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/ipfs/idx/1.0.0"), ()))
    }

    fn upgrade(self, socket: C, _: (), ty: Endpoint, remote_addr: Maf) -> Self::Future {
        trace!("Upgrading connection as {:?}", ty);

        struct HandshakeContext {
            local_key: PublicKey,
        }

        let context = HandshakeContext {
            local_key: self.public_key.clone(),
        };

        // The handshake messages all start with a 4-bytes message length prefix.
        let socket = length_delimited::Builder::new()
            .big_endian()
            .length_field_length(4)
            .new_framed(socket);

        let future = future::ok::<_, IoError>(context)
          // Generate our nonce.
          .and_then(|context| {
              trace!("starting handshake ; local key = {:?}", context.local_key);
              Ok(context)
          })
          .and_then(|context| {
              let mut message = structs_proto::NodeId::new();
              message.set_agentVersion(self.agent_version);
              message.set_protocolVersion(self.protocol_version);
              message.set_publicKey(self.public_key.into_protobuf_encoding());

              let bytes = message
                  .write_to_bytes()
                  .expect("writing protobuf failed ; should never happen");

            trace!("sending my info to remote");

            socket.send(BytesMut::from(bytes.clone()))
                .from_err()
                .map(|s| (s, context))
          })

          .and_then(move |(socket, _)| {
              socket.into_future()
                  .map_err(|(e, _)| e.into())
                  .and_then(move |(msg, socket)| {
                      debug!("Received identify message");

                      if let Some(msg) = msg {
                          let info = match parse_proto_msg(msg) {
                              Ok(v) => v,
                              Err(err) => {
                                  debug!("Failed to parse protobuf message ; error = {:?}", err);
                                  return Err(err.into());
                              }
                          };

                          trace!("Information received: {:?}", info);

                          let out = NodeIdOutput {
                              remote_info: info,
                              socket: socket.into_inner(),
                          };

                          Ok((out, remote_addr))
                      } else {
                          debug!("Identify protocol stream closed before receiving info");
                          Err(IoErrorKind::InvalidData.into())
                      }
                  })
          });
        Box::new(future) as Box<_>
    }
}

// Turns a protobuf message into an `IdentifyInfo` and an observed address. If something bad
// happens, turn it into an `IoError`.
fn parse_proto_msg(msg: BytesMut) -> Result<NodeInfo, IoError> {
    match protobuf_parse_from_bytes::<structs_proto::NodeId>(&msg) {
        Ok(mut msg) => {
            let info = NodeInfo {
                public_key: PublicKey::from_protobuf_encoding(msg.get_publicKey())?,
                protocol_version: msg.take_protocolVersion(),
                agent_version: msg.take_agentVersion(),
            };

            Ok(info)
        }

        Err(err) => Err(IoError::new(IoErrorKind::InvalidData, err)),
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio_current_thread;
    extern crate tokio_io;

    use self::libp2p_tcp_transport::TcpConfig;
    use futures::{future, Future, Stream};
    use libp2p_core::{PublicKey, Transport};
    use libp2p_peerstore::PeerId;
    use std::io::Write;
    use std::sync::mpsc;
    use std::thread;
    use std::time;
    use NodeIdProtocolConfig;

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send info from the server to the client, and check that
        // they were successfully received.

        let (tx, rx) = mpsc::channel();

        let _bg_thread = thread::spawn(move || {
            let node_id_upgrade = NodeIdProtocolConfig {
                public_key: PublicKey::Ed25519(vec![1, 2, 3, 4, 5, 6, 7]),
                protocol_version: "proto_version".to_owned(),
                agent_version: "agent_version".to_owned(),
            };
            let transport = TcpConfig::new().with_upgrade(node_id_upgrade).and_then(
                move |out, _, remote_addr| {
                    let socket = out.socket;
                    assert_eq!(
                        PeerId::from_public_key(out.remote_info.public_key.clone()),
                        PeerId::from_public_key(PublicKey::Ed25519(vec![7, 6, 5, 4, 3, 2, 1]))
                    );
                    future::ok((socket, remote_addr))
                },
            );

            let (listener, addr) = transport
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap_or_else(|_| panic!());

            tx.send(addr).unwrap();

            let future = listener.for_each(|sock| {
                sock.and_then(|(sock, _)| {
                    // Define what to do with the socket that just connected to us
                    // Which in this case is read 3 bytes
                    let handle_conn = tokio_io::io::read_exact(sock, [0; 3])
                        .map(|(_, buf)| {
                            assert_eq!(buf, [1, 2, 3]);
                        }).map_err(|err| panic!("IO error {:?}", err));

                    // Spawn the future as a concurrent task
                    tokio_current_thread::spawn(handle_conn);

                    Ok(())
                })
            });

            let _ = tokio_current_thread::block_on_all(future).unwrap();
        });

        let node_id_upgrade = NodeIdProtocolConfig {
            public_key: PublicKey::Ed25519(vec![7, 6, 5, 4, 3, 2, 1]),
            protocol_version: "proto_version".to_owned(),
            agent_version: "agent_version".to_owned(),
        };
        let transport =
            TcpConfig::new()
                .with_upgrade(node_id_upgrade)
                .and_then(move |out, _, remote_addr| {
                    let socket = out.socket;
                    assert_eq!(
                        PeerId::from_public_key(out.remote_info.public_key.clone()),
                        PeerId::from_public_key(PublicKey::Ed25519(vec![1, 2, 3, 4, 5, 6, 7]))
                    );
                    future::ok((socket, remote_addr))
                });
        ;

        let addr = rx.recv().unwrap();

        // Obtain a future socket through dialing
        let socket = transport.dial(addr.clone()).unwrap_or_else(|_e| {
            println!("Failed to dial!");
            panic!()
        });

        // Define what to do with the socket once it's obtained
        let action = socket.then(|sock| -> Result<(), ()> {
            sock.unwrap().0.write(&[0x1, 0x2, 0x3]).unwrap();
            Ok(())
        });

        tokio_current_thread::block_on_all(action).unwrap();
        thread::sleep(time::Duration::from_millis(100));
    }
}
