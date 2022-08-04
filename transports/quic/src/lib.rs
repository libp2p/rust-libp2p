

use libp2p_core::{Transport, StreamMuxer,
    PeerId,
    multiaddr::{Multiaddr, Protocol},
    identity::Keypair,
    transport::{TransportError, ListenerId, TransportEvent},
};

use std::{
    task::{Context, Poll},
    pin::Pin,
    future::Future,
    io::self,
    time::Duration,
    sync::Arc,
    net::SocketAddr,
};

mod tls;

pub struct QuicSubstream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl futures::AsyncRead for QuicSubstream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        futures::AsyncRead::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

impl futures::AsyncWrite for QuicSubstream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        futures::AsyncWrite::poll_write(Pin::new(&mut self.get_mut().send), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        futures::AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().send), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        futures::AsyncWrite::poll_close(Pin::new(&mut self.get_mut().send), cx)
    }
}

pub struct QuicMuxer {
    connection: quinn::Connection,
    incoming: quinn::IncomingBiStreams,
    outgoing: Option<quinn::OpenBi>,
}

impl StreamMuxer for QuicMuxer {
    type Substream = QuicSubstream;
    type Error = quinn::ConnectionError;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let res = futures::Stream::poll_next(Pin::new(&mut self.get_mut().incoming), cx);
        let res = res?;
        match res {
            Poll::Ready(Some((send, recv))) => Poll::Ready(Ok(QuicSubstream { send, recv })),
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => panic!("exhasted")
        }
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();

        let open_future = this.outgoing.take();

        if let Some(mut open_future) = open_future {
            match Pin::new(&mut open_future).poll(cx) {
                Poll::Pending => {
                    this.outgoing.replace(open_future);
                    Poll::Pending
                },
                Poll::Ready(result) => {
                    let result = result
                        .map(|(send, recv)| QuicSubstream { send, recv });
                    Poll::Ready(result)
                },
            }
        } else {
            let open_future = this.connection.open_bi();
            this.outgoing.replace(open_future);
            
            Pin::new(this).poll_outbound(cx)
        }
    }

    fn poll_address_change(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Multiaddr, Self::Error>> {
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connection.close(From::from(0u32), &[]);
        Poll::Ready(Ok(()))
    }

}

pub struct QuicUpgrade {
    connecting: quinn::Connecting,
}

impl QuicUpgrade {
    /// Builds an [`Upgrade`] that wraps around a [`quinn::Connecting`].
    pub(crate) fn from_connecting(connecting: quinn::Connecting) -> Self {
        QuicUpgrade { connecting }
    }
}

impl Future for QuicUpgrade {
    type Output = Result<(PeerId, QuicMuxer), io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let connecting = Pin::new(&mut self.get_mut().connecting);

        connecting.poll(cx)
            .map_err(|e| io::Error::from(e))
            .map_ok(|new_connection| {
                let quinn::NewConnection { connection, bi_streams, .. } = new_connection;
                let muxer = QuicMuxer { connection, incoming: bi_streams, outgoing: None};
                let peer_id = PeerId::from_bytes(&[]).unwrap(); // TODO
                (peer_id, muxer)
            })
    }
}

/// Represents the configuration for the [`Endpoint`].
#[derive(Debug, Clone)]
pub struct Config {
    /// The client configuration to pass to `quinn`.
    client_config: quinn::ClientConfig,
    /// The server configuration to pass to `quinn`.
    server_config: quinn::ServerConfig
}

impl Config {
    /// Creates a new configuration object with default values.
    pub fn new(keypair: &Keypair) -> Result<Self, tls::ConfigError> {
        let mut transport = quinn::TransportConfig::default();
        transport.max_concurrent_uni_streams(0u32.into()); // Can only panic if value is out of range.
        transport.datagram_receive_buffer_size(None);
        transport.keep_alive_interval(Some(Duration::from_millis(10)));
        let transport = Arc::new(transport);

        let client_tls_config = tls::make_client_config(keypair).unwrap();
        let server_tls_config = tls::make_server_config(keypair).unwrap();

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(server_tls_config));
        server_config.transport = Arc::clone(&transport);

        let mut client_config = quinn::ClientConfig::new(Arc::new(client_tls_config));
        client_config.transport_config(transport);
        Ok(Self {
            client_config,
            server_config: server_config,
        })
    }
}

struct QuicTransport {
    config: Config,
    endpoint: Option<(quinn::Endpoint, quinn::Incoming)>,
}

impl QuicTransport {
    pub fn new(keypair: &Keypair) -> Self {
        Self { config: Config::new(keypair).unwrap(), endpoint: None }
    }
}

impl Transport for QuicTransport {
    type Output = (PeerId, QuicMuxer);
    type Error = io::Error;
    type ListenerUpgrade = QuicUpgrade;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let socket_addr =
            multiaddr_to_socketaddr(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;

        let client_config = self.config.client_config.clone();
        let server_config = self.config.server_config.clone();

        let (mut endpoint, incoming) = quinn::Endpoint::server(server_config, socket_addr).unwrap();
        endpoint.set_default_client_config(client_config);

        self.endpoint = Some((endpoint, incoming));

        Ok(ListenerId::new())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        true
        // if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
        //     listener.close(Ok(()));
        //     true
        // } else {
        //     false
        // }
    }

    fn address_translation(&self, _server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Some(observed.clone())
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let server_addr = "[::]:0".parse().unwrap();
        let client_config = self.config.client_config.clone();
        let server_config = self.config.server_config.clone();

        let (mut endpoint, _) = quinn::Endpoint::server(server_config, server_addr).unwrap();
        endpoint.set_default_client_config(client_config);

        Ok(Box::pin(async move {
            let connecting = endpoint.connect(socket_addr, "server_name").unwrap();
            QuicUpgrade::from_connecting(connecting).await
        }))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        // TODO: As the listener of a QUIC hole punch, we need to send a random UDP packet to the
        // `addr`. See DCUtR specification below.
        //
        // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
        self.dial(addr)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let incoming = Pin::new(&mut self.endpoint.as_mut().unwrap().1);
        futures::Stream::poll_next(incoming, cx)
            .map(|connecting| {
                let connecting = connecting.unwrap();
                let upgrade = QuicUpgrade::from_connecting(connecting);
                let event = TransportEvent::Incoming {
                    upgrade,
                    local_addr: Multiaddr::empty(),
                    send_back_addr: Multiaddr::empty(),
                    listener_id: ListenerId::new(),
                };
                event
            })
        // match self.listeners.poll_next_unpin(cx) {
        //     Poll::Ready(Some(ev)) => Poll::Ready(ev),
        //     _ => Poll::Pending,
        // }
    }
}

pub fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    for proto in iter {
        match proto {
            Protocol::P2p(_) => {} // Ignore a `/p2p/...` prefix of possibly outer protocols, if present.
            _ => return None,
        }
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::Quic) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::Quic) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        _ => None,
    }
}
