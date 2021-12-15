// Copyright 2021 David Craven.
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

use crate::crypto::{CryptoConfig, TlsCrypto};
use crate::muxer::QuicMuxer;
use crate::{QuicConfig, QuicError};
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use quinn_proto::{
    ClientConfig as QuinnClientConfig, ConnectionEvent, ConnectionHandle, DatagramEvent,
    EcnCodepoint, Endpoint as QuinnEndpoint, EndpointEvent, ServerConfig as QuinnServerConfig,
    Transmit,
};
use std::collections::{HashMap, VecDeque};
use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use udp_socket::{RecvMeta, SocketType, UdpCapabilities, UdpSocket, BATCH_SIZE};

/// Message sent to the endpoint background task.
#[derive(Debug)]
enum ToEndpoint {
    /// Instructs the endpoint to start connecting to the given address.
    Dial {
        /// UDP address to connect to.
        addr: SocketAddr,
        /// Channel to return the result of the dialing to.
        tx: oneshot::Sender<Result<QuicMuxer, QuicError>>,
    },
    /// Sent by a `quinn_proto` connection when the endpoint needs to process an event generated
    /// by a connection. The event itself is opaque to us.
    ConnectionEvent {
        connection_id: ConnectionHandle,
        event: EndpointEvent,
    },
    /// Instruct the endpoint to transmit a packet on its UDP socket.
    Transmit(Transmit),
}

#[derive(Debug)]
pub struct TransportChannel {
    tx: mpsc::UnboundedSender<ToEndpoint>,
    rx: mpsc::Receiver<Result<QuicMuxer, QuicError>>,
    port: u16,
    ty: SocketType,
}

impl TransportChannel {
    pub fn dial(&mut self, addr: SocketAddr) -> oneshot::Receiver<Result<QuicMuxer, QuicError>> {
        let (tx, rx) = oneshot::channel();
        let msg = ToEndpoint::Dial { addr, tx };
        self.tx.unbounded_send(msg).expect("endpoint has crashed");
        rx
    }

    pub fn poll_incoming(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Option<Result<QuicMuxer, QuicError>>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn ty(&self) -> SocketType {
        self.ty
    }
}

#[derive(Debug)]
pub struct ConnectionChannel {
    id: ConnectionHandle,
    tx: mpsc::UnboundedSender<ToEndpoint>,
    rx: mpsc::Receiver<ConnectionEvent>,
    port: u16,
    max_datagrams: usize,
}

impl ConnectionChannel {
    pub fn poll_channel_events(&mut self, cx: &mut Context) -> Poll<ConnectionEvent> {
        match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Ready(Some(event)) => Poll::Ready(event),
            Poll::Ready(None) => panic!("endpoint has crashed"),
            Poll::Pending => Poll::Pending,
        }
    }

    pub fn send_endpoint_event(&mut self, event: EndpointEvent) {
        let msg = ToEndpoint::ConnectionEvent {
            connection_id: self.id,
            event,
        };
        self.tx.unbounded_send(msg).expect("endpoint has crashed")
    }

    pub fn send_transmit(&mut self, transmit: Transmit) {
        let msg = ToEndpoint::Transmit(transmit);
        self.tx.unbounded_send(msg).expect("endpoint has crashed")
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn max_datagrams(&self) -> usize {
        self.max_datagrams
    }
}

#[derive(Debug)]
struct EndpointChannel {
    rx: mpsc::UnboundedReceiver<ToEndpoint>,
    tx: mpsc::Sender<Result<QuicMuxer, QuicError>>,
    port: u16,
    max_datagrams: usize,
    connection_tx: mpsc::UnboundedSender<ToEndpoint>,
}

impl EndpointChannel {
    pub fn poll_next_event(&mut self, cx: &mut Context) -> Poll<Option<ToEndpoint>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }

    pub fn create_connection(
        &self,
        id: ConnectionHandle,
    ) -> (ConnectionChannel, mpsc::Sender<ConnectionEvent>) {
        let (tx, rx) = mpsc::channel(12);
        let channel = ConnectionChannel {
            id,
            tx: self.connection_tx.clone(),
            rx,
            port: self.port,
            max_datagrams: self.max_datagrams,
        };
        (channel, tx)
    }
}

pub struct EndpointConfig {
    socket: UdpSocket,
    endpoint: QuinnEndpoint,
    port: u16,
    crypto_config: Arc<CryptoConfig>,
    capabilities: UdpCapabilities,
}

impl EndpointConfig {
    pub fn new(mut config: QuicConfig, addr: SocketAddr) -> Result<Self, QuicError> {
        config.transport.max_concurrent_uni_streams(0u32.into());
        config.transport.datagram_receive_buffer_size(None);
        let transport = Arc::new(config.transport);

        let crypto_config = Arc::new(CryptoConfig {
            keypair: config.keypair,
            keylogger: config.keylogger,
            transport: transport.clone(),
        });

        let crypto = TlsCrypto::new_server_config(&crypto_config);
        let mut server_config = QuinnServerConfig::with_crypto(crypto);
        server_config.transport = transport;

        let endpoint = QuinnEndpoint::new(Default::default(), Some(Arc::new(server_config)));

        let socket = UdpSocket::bind(addr)?;
        let port = socket.local_addr()?.port();
        let capabilities = UdpSocket::capabilities()?;
        Ok(Self {
            socket,
            endpoint,
            port,
            crypto_config,
            capabilities,
        })
    }

    pub fn spawn(self) -> TransportChannel {
        let (tx1, rx1) = mpsc::unbounded();
        let (tx2, rx2) = mpsc::channel(1);
        let transport = TransportChannel {
            tx: tx1,
            rx: rx2,
            port: self.port,
            ty: self.socket.socket_type(),
        };
        let endpoint = EndpointChannel {
            tx: tx2,
            rx: rx1,
            port: self.port,
            max_datagrams: self.capabilities.max_gso_segments,
            connection_tx: transport.tx.clone(),
        };
        async_global_executor::spawn(Endpoint::new(endpoint, self)).detach();
        transport
    }
}

struct Endpoint {
    channel: EndpointChannel,
    endpoint: QuinnEndpoint,
    socket: UdpSocket,
    crypto_config: Arc<CryptoConfig>,
    connections: HashMap<ConnectionHandle, mpsc::Sender<ConnectionEvent>>,
    outgoing: VecDeque<udp_socket::Transmit>,
    recv_buf: Box<[u8]>,
    incoming_slot: Option<QuicMuxer>,
    event_slot: Option<(ConnectionHandle, ConnectionEvent)>,
}

impl Endpoint {
    pub fn new(channel: EndpointChannel, config: EndpointConfig) -> Self {
        let max_udp_payload_size = config
            .endpoint
            .config()
            .get_max_udp_payload_size()
            .min(u16::MAX as _) as usize;
        let recv_buf = vec![0; max_udp_payload_size * BATCH_SIZE].into_boxed_slice();
        Self {
            channel,
            endpoint: config.endpoint,
            socket: config.socket,
            crypto_config: config.crypto_config,
            connections: Default::default(),
            outgoing: Default::default(),
            recv_buf,
            incoming_slot: None,
            event_slot: None,
        }
    }

    pub fn transmit(&mut self, transmit: Transmit) {
        let ecn = transmit
            .ecn
            .map(|ecn| udp_socket::EcnCodepoint::from_bits(ecn as u8))
            .unwrap_or_default();
        let transmit = udp_socket::Transmit {
            destination: transmit.destination,
            contents: transmit.contents,
            ecn,
            segment_size: transmit.segment_size,
            src_ip: transmit.src_ip,
        };
        self.outgoing.push_back(transmit);
    }

    fn send_incoming(&mut self, muxer: QuicMuxer, cx: &mut Context) -> bool {
        assert!(self.incoming_slot.is_none());
        match self.channel.tx.poll_ready(cx) {
            Poll::Pending => {
                self.incoming_slot = Some(muxer);
                true
            }
            Poll::Ready(Ok(())) => {
                self.channel.tx.try_send(Ok(muxer)).ok();
                false
            }
            Poll::Ready(_err) => false,
        }
    }

    fn send_event(
        &mut self,
        id: ConnectionHandle,
        event: ConnectionEvent,
        cx: &mut Context,
    ) -> bool {
        assert!(self.event_slot.is_none());
        let conn = self.connections.get_mut(&id).unwrap();
        match conn.poll_ready(cx) {
            Poll::Pending => {
                self.event_slot = Some((id, event));
                true
            }
            Poll::Ready(Ok(())) => {
                conn.try_send(event).ok();
                false
            }
            Poll::Ready(_err) => false,
        }
    }
}

impl Future for Endpoint {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let me = Pin::into_inner(self);

        if let Some(muxer) = me.incoming_slot.take() {
            if !me.send_incoming(muxer, cx) {
                tracing::info!("cleared incoming slot");
            }
        }

        if let Some((id, event)) = me.event_slot.take() {
            if !me.send_event(id, event, cx) {
                tracing::info!("cleared event slot");
            }
        }

        while let Some(transmit) = me.endpoint.poll_transmit() {
            me.transmit(transmit);
        }

        if me.event_slot.is_none() {
            while let Poll::Ready(event) = me.channel.poll_next_event(cx) {
                match event {
                    Some(ToEndpoint::Dial { addr, tx }) => {
                        let crypto = TlsCrypto::new_client_config(&me.crypto_config);
                        let mut client_config = QuinnClientConfig::new(crypto);
                        client_config.transport = me.crypto_config.transport.clone();

                        let (id, connection) =
                            match me.endpoint.connect(client_config, addr, "server_name") {
                                Ok(c) => c,
                                Err(err) => {
                                    tracing::error!("dial failure: {}", err);
                                    let _ = tx.send(Err(err.into()));
                                    continue;
                                }
                            };
                        let (channel, conn) = me.channel.create_connection(id);
                        me.connections.insert(id, conn);
                        let muxer = QuicMuxer::new(channel, connection);
                        tx.send(Ok(muxer)).ok();
                    }
                    Some(ToEndpoint::ConnectionEvent {
                        connection_id,
                        event,
                    }) => {
                        let is_drained_event = event.is_drained();
                        if is_drained_event {
                            me.connections.remove(&connection_id);
                        }
                        if let Some(event) = me.endpoint.handle_event(connection_id, event) {
                            if me.send_event(connection_id, event, cx) {
                                tracing::info!("filled event slot");
                                break;
                            }
                        }
                    }
                    Some(ToEndpoint::Transmit(transmit)) => {
                        me.transmit(transmit);
                    }
                    None => {
                        me.endpoint.reject_new_connections();
                        return Poll::Ready(());
                    }
                }
            }
        }

        while !me.outgoing.is_empty() {
            let transmits: &[_] = me.outgoing.make_contiguous();
            match me.socket.poll_send(cx, transmits) {
                Poll::Ready(Ok(n)) => {
                    me.outgoing.drain(..n);
                }
                Poll::Ready(Err(err)) => tracing::error!("send_to: {}", err),
                Poll::Pending => break,
            }
        }

        if me.event_slot.is_none() && me.incoming_slot.is_none() {
            let mut metas = [RecvMeta::default(); BATCH_SIZE];
            let mut iovs = MaybeUninit::<[IoSliceMut; BATCH_SIZE]>::uninit();
            fn init_iovs<'a>(
                iovs: &'a mut MaybeUninit<[IoSliceMut<'a>; BATCH_SIZE]>,
                recv_buf: &'a mut [u8],
            ) -> &'a mut [IoSliceMut<'a>] {
                let chunk_size = recv_buf.len() / BATCH_SIZE;
                let chunks = recv_buf.chunks_mut(chunk_size);
                // every iovs elem must be initialized with an according elem from buf chunks
                assert_eq!(chunks.len(), BATCH_SIZE);
                chunks.enumerate().for_each(|(i, buf)| unsafe {
                    iovs.as_mut_ptr()
                        .cast::<IoSliceMut>()
                        .add(i)
                        .write(IoSliceMut::new(buf));
                });

                unsafe {
                    // SAFETY: all elements are initialized
                    iovs.assume_init_mut()
                }
            }
            let mut recv_buf = core::mem::replace(&mut me.recv_buf, Vec::new().into_boxed_slice());
            let iovs = init_iovs(&mut iovs, &mut recv_buf);
            while let Poll::Ready(result) = me.socket.poll_recv(cx, iovs, &mut metas) {
                let n = match result {
                    Ok(n) => n,
                    Err(err) => {
                        tracing::error!("recv_from: {}", err);
                        continue;
                    }
                };
                for i in 0..n {
                    let meta = &metas[i];
                    let packet = From::from(&iovs[i][..meta.len]);
                    let ecn = meta
                        .ecn
                        .map(|ecn| EcnCodepoint::from_bits(ecn as u8))
                        .unwrap_or_default();
                    match me
                        .endpoint
                        .handle(Instant::now(), meta.source, meta.dst_ip, ecn, packet)
                    {
                        None => {}
                        Some((id, DatagramEvent::ConnectionEvent(event))) => {
                            if me.send_event(id, event, cx) {
                                tracing::info!("filled event slot");
                                break;
                            }
                        }
                        Some((id, DatagramEvent::NewConnection(connection))) => {
                            let (channel, tx) = me.channel.create_connection(id);
                            me.connections.insert(id, tx);
                            let muxer = QuicMuxer::new(channel, connection);
                            if me.send_incoming(muxer, cx) {
                                tracing::info!("filled incoming slot");
                                break;
                            }
                        }
                    }
                }
            }
            me.recv_buf = recv_buf;
        }

        Poll::Pending
    }
}
