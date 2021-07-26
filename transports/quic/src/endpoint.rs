use crate::crypto::{Crypto, CryptoConfig};
use crate::muxer::QuicMuxer;
use crate::{QuicConfig, QuicError};
use ed25519_dalek::PublicKey;
use fnv::FnvHashMap;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use quinn_proto::crypto::Session;
use quinn_proto::generic::{ClientConfig, ServerConfig};
use quinn_proto::{
    ConnectionEvent, ConnectionHandle, DatagramEvent, EcnCodepoint, EndpointEvent, Transmit,
};
use std::collections::VecDeque;
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
enum ToEndpoint<C: Crypto> {
    /// Instructs the endpoint to start connecting to the given address.
    Dial {
        /// UDP address to connect to.
        addr: SocketAddr,
        /// The remotes public key.
        public_key: PublicKey,
        /// Channel to return the result of the dialing to.
        tx: oneshot::Sender<Result<QuicMuxer<C>, QuicError>>,
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
pub struct TransportChannel<C: Crypto> {
    tx: mpsc::UnboundedSender<ToEndpoint<C>>,
    rx: mpsc::Receiver<Result<QuicMuxer<C>, QuicError>>,
    port: u16,
    ty: SocketType,
}

impl<C: Crypto> TransportChannel<C> {
    pub fn dial(
        &mut self,
        addr: SocketAddr,
        public_key: PublicKey,
    ) -> oneshot::Receiver<Result<QuicMuxer<C>, QuicError>> {
        let (tx, rx) = oneshot::channel();
        let msg = ToEndpoint::Dial {
            addr,
            public_key,
            tx,
        };
        self.tx.unbounded_send(msg).expect("endpoint has crashed");
        rx
    }

    pub fn poll_incoming(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Option<Result<QuicMuxer<C>, QuicError>>> {
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
pub struct ConnectionChannel<C: Crypto> {
    id: ConnectionHandle,
    tx: mpsc::UnboundedSender<ToEndpoint<C>>,
    rx: mpsc::Receiver<ConnectionEvent>,
    port: u16,
    max_datagrams: usize,
}

impl<C: Crypto> ConnectionChannel<C> {
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
struct EndpointChannel<C: Crypto> {
    rx: mpsc::UnboundedReceiver<ToEndpoint<C>>,
    tx: mpsc::Sender<Result<QuicMuxer<C>, QuicError>>,
    port: u16,
    max_datagrams: usize,
    connection_tx: mpsc::UnboundedSender<ToEndpoint<C>>,
}

impl<C: Crypto> EndpointChannel<C> {
    pub fn poll_next_event(&mut self, cx: &mut Context) -> Poll<Option<ToEndpoint<C>>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }

    pub fn create_connection(
        &self,
        id: ConnectionHandle,
    ) -> (ConnectionChannel<C>, mpsc::Sender<ConnectionEvent>) {
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

type QuinnEndpointConfig<S> = quinn_proto::generic::EndpointConfig<S>;
type QuinnEndpoint<S> = quinn_proto::generic::Endpoint<S>;

pub struct EndpointConfig<C: Crypto> {
    socket: UdpSocket,
    endpoint: QuinnEndpoint<C::Session>,
    port: u16,
    crypto_config: Arc<CryptoConfig<C::Keylogger>>,
    capabilities: UdpCapabilities,
}

impl<C: Crypto> EndpointConfig<C> {
    pub fn new(mut config: QuicConfig<C>, addr: SocketAddr) -> Result<Self, QuicError> {
        config.transport.max_concurrent_uni_streams(0)?;
        config.transport.datagram_receive_buffer_size(None);
        let transport = Arc::new(config.transport);

        let crypto_config = Arc::new(CryptoConfig {
            keypair: config.keypair,
            psk: config.psk,
            keylogger: config.keylogger,
            transport: transport.clone(),
        });

        let mut server_config = ServerConfig::<C::Session>::default();
        server_config.transport = transport;
        server_config.crypto = C::new_server_config(&crypto_config);

        let mut endpoint_config = QuinnEndpointConfig::default();
        endpoint_config
            .supported_versions(C::supported_quic_versions(), C::default_quic_version())?;

        let socket = UdpSocket::bind(addr)?;
        let port = socket.local_addr()?.port();
        let endpoint = quinn_proto::generic::Endpoint::new(
            Arc::new(endpoint_config),
            Some(Arc::new(server_config)),
        );
        let capabilities = UdpSocket::capabilities()?;
        Ok(Self {
            socket,
            endpoint,
            port,
            crypto_config,
            capabilities,
        })
    }

    pub fn spawn(self) -> TransportChannel<C>
    where
        <C::Session as Session>::ClientConfig: Send + Unpin,
        <C::Session as Session>::HeaderKey: Unpin,
        <C::Session as Session>::PacketKey: Unpin,
    {
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

struct Endpoint<C: Crypto> {
    channel: EndpointChannel<C>,
    endpoint: QuinnEndpoint<C::Session>,
    socket: UdpSocket,
    crypto_config: Arc<CryptoConfig<C::Keylogger>>,
    connections: FnvHashMap<ConnectionHandle, mpsc::Sender<ConnectionEvent>>,
    outgoing: VecDeque<udp_socket::Transmit>,
    recv_buf: Box<[u8]>,
    incoming_slot: Option<QuicMuxer<C>>,
    event_slot: Option<(ConnectionHandle, ConnectionEvent)>,
}

impl<C: Crypto> Endpoint<C> {
    pub fn new(channel: EndpointChannel<C>, config: EndpointConfig<C>) -> Self {
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

    fn send_incoming(&mut self, muxer: QuicMuxer<C>, cx: &mut Context) -> bool {
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

impl<C: Crypto> Future for Endpoint<C>
where
    <C::Session as Session>::ClientConfig: Unpin,
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
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
                    Some(ToEndpoint::Dial {
                        addr,
                        public_key,
                        tx,
                    }) => {
                        let crypto = C::new_client_config(&me.crypto_config, public_key);
                        let client_config = ClientConfig {
                            transport: me.crypto_config.transport.clone(),
                            crypto,
                        };
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
            me.outgoing.make_contiguous();
            match me.socket.poll_send(cx, me.outgoing.as_slices().0) {
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
            me.recv_buf
                .chunks_mut(me.recv_buf.len() / BATCH_SIZE)
                .enumerate()
                .for_each(|(i, buf)| unsafe {
                    iovs.as_mut_ptr()
                        .cast::<IoSliceMut>()
                        .add(i)
                        .write(IoSliceMut::new(buf));
                });
            let mut iovs = unsafe { iovs.assume_init() };
            while let Poll::Ready(result) = me.socket.poll_recv(cx, &mut iovs, &mut metas) {
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
        }

        Poll::Pending
    }
}
