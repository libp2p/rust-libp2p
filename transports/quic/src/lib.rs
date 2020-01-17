// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for QUIC/UDP/IP.
//!
//! Uses [the *tokio* library](https://tokio.rs).
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! use libp2p_quic::{QuicConfig, Endpoint};
//! use libp2p_core::Multiaddr;
//!
//! # fn main() {
//! let quic_config = QuicConfig::default();
//! let quic_endpoint = Endpoint::new(
//!     quic_config,
//!     "/ip4/127.0.0.1/udp/12345/quic".parse().expect("bad address?"),
//! )
//! .expect("I/O error");
//! # }
//! ```
//!
//! The `QuicConfig` structs implements the `Transport` trait of the `swarm` library. See the
//! documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.
//!
//! Note that QUIC provides transport, security, and multiplexing in a single protocol.  Therefore,
//! QUIC connections do not need to be upgraded.  You will get a compile-time error if you try.
//! Instead, you must pass all needed configuration into the constructor.
//!
//! # Design Notes
//!
//! The entry point is the `Endpoint` struct.  It represents a single QUIC endpoint.  You
//! should generally have one of these per process.
//!
//! `Endpoint` manages a background task that processes all incoming packets.  Each
//! `QuicConnection` also manages a background task, which handles socket output and timer polling.

#![forbid(unused_must_use, unstable_features, warnings)]
#![deny(missing_copy_implementations)]
#![deny(trivial_casts)]
mod certificate;
mod endpoint;
#[cfg(test)]
mod tests;
mod verifier;
use async_macros::ready;
pub use certificate::make_cert;
pub use endpoint::Endpoint;
use endpoint::EndpointInner;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_core::StreamMuxer;
use log::{debug, error, trace};
use parking_lot::{Mutex, MutexGuard};
use quinn_proto::{Connection, ConnectionEvent, ConnectionHandle, Dir, StreamId};
use std::{
    collections::HashMap,
    io,
    mem::replace,
    pin::Pin,
    sync::Arc,
    task::{
        Context,
        Poll::{self, Pending, Ready},
    },
    time::Instant,
};
#[derive(Debug)]
pub struct QuicSubstream {
    id: StreamId,
    status: SubstreamStatus,
}

#[derive(Debug)]
enum SubstreamStatus {
    Live,
    Finishing(oneshot::Receiver<()>),
    Finished,
}

impl QuicSubstream {
    fn new(id: StreamId) -> Self {
        let status = SubstreamStatus::Live;
        Self { id, status }
    }

    fn id(&self) -> StreamId {
        self.id
    }

    fn is_live(&self) -> bool {
        match self.status {
            SubstreamStatus::Live => true,
            SubstreamStatus::Finishing(_) | SubstreamStatus::Finished => false,
        }
    }
}

/// Represents the configuration for a QUIC/UDP/IP transport capability for libp2p.
///
/// The QUIC endpoints created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone)]
pub struct QuicConfig {
    /// The client configuration.  Quinn provides functions for making one.
    pub client_config: quinn_proto::ClientConfig,
    /// The server configuration.  Quinn provides functions for making one.
    pub server_config: Arc<quinn_proto::ServerConfig>,
    /// The endpoint configuration
    pub endpoint_config: Arc<quinn_proto::EndpointConfig>,
}

fn make_client_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
) -> quinn_proto::ClientConfig {
    let mut transport = quinn_proto::TransportConfig::default();
    transport.stream_window_uni(0);
    transport.datagram_receive_buffer_size(None);
    let mut crypto = rustls::ClientConfig::new();
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto.enable_early_data = true;
    crypto.set_single_client_cert(vec![certificate], key);
    let verifier = verifier::VeryInsecureAllowAllCertificatesWithoutChecking;
    crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(verifier));
    quinn_proto::ClientConfig {
        transport: Arc::new(transport),
        crypto: Arc::new(crypto),
    }
}

fn make_server_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
) -> quinn_proto::ServerConfig {
    let mut transport = quinn_proto::TransportConfig::default();
    transport.stream_window_uni(0);
    transport.datagram_receive_buffer_size(None);
    let mut crypto = rustls::ServerConfig::new(Arc::new(
        verifier::VeryInsecureRequireClientCertificateButDoNotCheckIt,
    ));
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto
        .set_single_cert(vec![certificate], key)
        .expect("we are given a valid cert; qed");
    let mut config = quinn_proto::ServerConfig::default();
    config.transport = Arc::new(transport);
    config.crypto = Arc::new(crypto);
    config
}

impl Default for QuicConfig {
    /// Creates a new configuration object for TCP/IP.
    fn default() -> Self {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        let cert = make_cert(&keypair);
        let (cert, key) = (
            rustls::Certificate(
                cert.serialize_der()
                    .expect("serialization of a valid cert will succeed; qed"),
            ),
            rustls::PrivateKey(cert.serialize_private_key_der()),
        );
        Self {
            client_config: make_client_config(cert.clone(), key.clone()),
            server_config: Arc::new(make_server_config(cert, key)),
            endpoint_config: Default::default(),
        }
    }
}

#[derive(Debug)]
enum EndpointMessage {
    ConnectionAccepted,
    EndpointEvent {
        handle: ConnectionHandle,
        event: quinn_proto::EndpointEvent,
    },
}

#[derive(Debug, Clone)]
pub struct QuicMuxer(Arc<Mutex<Muxer>>);

impl QuicMuxer {
    fn inner<'a>(&'a self) -> MutexGuard<'a, Muxer> {
        self.0.lock()
    }
}

#[derive(Debug)]
enum OutboundInner {
    Complete(Result<StreamId, io::Error>),
    Pending(oneshot::Receiver<StreamId>),
    Done,
}

pub struct Outbound(OutboundInner);

impl Future for Outbound {
    type Output = Result<QuicSubstream, io::Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;
        match this.0 {
            OutboundInner::Complete(_) => match replace(&mut this.0, OutboundInner::Done) {
                OutboundInner::Complete(e) => Ready(e.map(QuicSubstream::new)),
                _ => unreachable!(),
            },
            OutboundInner::Pending(ref mut receiver) => {
                let result = ready!(receiver.poll_unpin(cx))
                    .map(QuicSubstream::new)
                    .map_err(|oneshot::Canceled| io::ErrorKind::ConnectionAborted.into());
                this.0 = OutboundInner::Done;
                Ready(result)
            }
            OutboundInner::Done => panic!("polled after yielding Ready"),
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = Outbound;
    type Substream = QuicSubstream;
    type Error = io::Error;
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let mut inner = self.inner();
        if let Some(ref e) = inner.close_reason {
            Outbound(OutboundInner::Complete(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                e.clone(),
            ))))
        } else if let Some(id) = inner.get_pending_stream() {
            Outbound(OutboundInner::Complete(Ok(id)))
        } else {
            let (sender, receiver) = oneshot::channel();
            inner.connectors.push_front(sender);
            inner.wake_driver();
            Outbound(OutboundInner::Pending(receiver))
        }
    }
    fn destroy_outbound(&self, _: Outbound) {}
    fn destroy_substream(&self, substream: Self::Substream) {
        let mut inner = self.inner();
        if let Some(waker) = inner.writers.remove(&substream.id()) {
            waker.wake();
        }
        if let Some(waker) = inner.readers.remove(&substream.id()) {
            waker.wake();
        }
        drop(inner.connection.finish(substream.id()));
        drop(
            inner
                .connection
                .stop_sending(substream.id(), Default::default()),
        )
    }
    fn is_remote_acknowledged(&self) -> bool {
        true
    }

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
        debug!("being polled for inbound connections!");
        let mut inner = self.inner();
        if inner.connection.is_drained() {
            return Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                inner
                    .close_reason
                    .as_ref()
                    .expect("closed connections always have a reason; qed")
                    .clone(),
            )));
        }
        inner.wake_driver();
        match inner.connection.accept(quinn_proto::Dir::Bi) {
            None => {
                if let Some(waker) = replace(&mut inner.accept_waker, Some(cx.waker().clone())) {
                    waker.wake()
                }
                Pending
            }
            Some(id) => {
                inner.finishers.insert(id, None);
                Ready(Ok(QuicSubstream::new(id)))
            }
        }
    }

    fn write_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::WriteError;
        if !substream.is_live() {
            return Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        let mut inner = self.inner();
        inner.wake_driver();
        if let Some(ref e) = inner.close_reason {
            return Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                e.clone(),
            )));
        }

        assert!(
            !inner.connection.is_drained(),
            "attempting to write to a drained connection"
        );
        match inner.connection.write(substream.id(), buf) {
            Ok(bytes) => Ready(Ok(bytes)),
            Err(WriteError::Blocked) => {
                if let Some(ref e) = inner.close_reason {
                    return Ready(Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        e.clone(),
                    )));
                }
                inner.writers.insert(substream.id(), cx.waker().clone());
                Pending
            }
            Err(WriteError::UnknownStream) => {
                panic!("libp2p never uses a closed stream, so this cannot happen; qed")
            }
            Err(WriteError::Stopped(_)) => Ready(Err(io::ErrorKind::ConnectionAborted.into())),
        }
    }

    fn poll_outbound(
        &self,
        cx: &mut Context,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        substream.poll_unpin(cx)
    }

    fn read_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::ReadError;
        let mut inner = self.inner();
        inner.wake_driver();
        match inner.connection.read(substream.id(), buf) {
            Ok(Some(bytes)) => Ready(Ok(bytes)),
            Ok(None) => Ready(Ok(0)),
            Err(ReadError::Blocked) => {
                inner.readers.insert(substream.id(), cx.waker().clone());
                Pending
            }
            Err(ReadError::UnknownStream) => {
                error!("you used a stream that was already closed!");
                Ready(Ok(0))
            }
            Err(ReadError::Reset(_)) => Ready(Err(io::ErrorKind::ConnectionReset.into())),
        }
    }

    fn shutdown_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        match substream.status {
            SubstreamStatus::Finished => return Ready(Ok(())),
            SubstreamStatus::Finishing(ref mut channel) => {
                self.inner().wake_driver();
                return channel
                    .poll_unpin(cx)
                    .map_err(|e| io::Error::new(io::ErrorKind::ConnectionAborted, e.clone()));
            }
            SubstreamStatus::Live => {}
        }
        let mut inner = self.inner();
        inner.wake_driver();
        inner
            .connection
            .finish(substream.id())
            .map_err(|e| match e {
                quinn_proto::FinishError::UnknownStream => {
                    io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        quinn_proto::FinishError::UnknownStream,
                    );
                    unreachable!("we checked for this above!")
                }
                quinn_proto::FinishError::Stopped { .. } => {
                    io::Error::from(io::ErrorKind::ConnectionReset)
                }
            })?;
        let (sender, mut receiver) = oneshot::channel();
        assert!(
            receiver.poll_unpin(cx).is_pending(),
            "we haven’t written to the peer yet"
        );
        substream.status = SubstreamStatus::Finishing(receiver);
        assert!(inner
            .finishers
            .insert(substream.id(), Some(sender))
            .unwrap()
            .is_none());
        Pending
    }

    fn flush_substream(
        &self,
        _cx: &mut Context,
        _substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        Ready(Ok(()))
    }

    fn flush_all(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Ready(Ok(()))
    }

    fn close(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        trace!("close() called");
        let mut inner = self.inner();
        if inner.connection.is_closed() {
            return Ready(Ok(()));
        } else if inner.finishers.is_empty() {
            inner.shutdown();
            inner.close_reason = Some(quinn_proto::ConnectionError::LocallyClosed);
            drop(inner.driver().poll_unpin(cx));
            return Ready(Ok(()));
        } else if inner.close_waker.is_some() {
            inner.close_waker = Some(cx.waker().clone());
            return Pending;
        } else {
            inner.close_waker = Some(cx.waker().clone())
        }
        let Muxer {
            ref mut finishers,
            ref mut connection,
            ..
        } = *inner;
        finishers.retain(|id, channel| {
            if channel.is_some() {
                true
            } else {
                connection.finish(*id).is_ok()
            }
        });
        Pending
    }
}

#[derive(Debug)]
pub struct QuicUpgrade {
    muxer: Option<QuicMuxer>,
}

#[cfg(test)]
impl Drop for QuicUpgrade {
    fn drop(&mut self) {
        debug!("dropping upgrade!");
        assert!(
            self.muxer.is_none(),
            "dropped before being polled to completion"
        );
    }
}

impl Future for QuicUpgrade {
    type Output = Result<QuicMuxer, io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let muxer = &mut self.get_mut().muxer;
        trace!("outbound polling!");
        let res = {
            let mut inner = muxer.as_mut().expect("polled after yielding Ready").inner();
            if inner.connection.is_closed() {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    inner
                        .close_reason
                        .as_ref()
                        .expect("closed connections always have a reason; qed")
                        .clone(),
                ))
            } else if inner.connection.is_handshaking() {
                assert!(!inner.connection.is_closed(), "deadlock");
                inner.handshake_waker = Some(cx.waker().clone());
                return Pending;
            } else if inner.connection.side().is_server() {
                ready!(inner.endpoint_channel.poll_ready(cx))
                    .expect("we have a reference to the peer; qed");
                Ok(inner
                    .endpoint_channel
                    .start_send(EndpointMessage::ConnectionAccepted)
                    .expect(
                        "we only send one datum per clone of the channel, so we have capacity \
                         to send this; qed",
                    ))
            } else {
                Ok(())
            }
        };
        let muxer = muxer.take().expect("polled after yielding Ready");
        Ready(res.map(|()| muxer))
    }
}

type StreamSenderQueue = std::collections::VecDeque<oneshot::Sender<StreamId>>;

#[derive(Debug)]
pub struct Muxer {
    /// The pending stream, if any.
    pending_stream: Option<StreamId>,
    /// The associated endpoint
    endpoint: Endpoint,
    /// The `quinn_proto::Connection` struct.
    connection: Connection,
    /// Connection handle
    handle: ConnectionHandle,
    /// Tasks blocked on writing
    writers: HashMap<StreamId, std::task::Waker>,
    /// Tasks blocked on reading
    readers: HashMap<StreamId, std::task::Waker>,
    /// Tasks blocked on finishing
    finishers: HashMap<StreamId, Option<oneshot::Sender<()>>>,
    /// Task waiting for new connections
    handshake_waker: Option<std::task::Waker>,
    /// Task waiting for new connections
    accept_waker: Option<std::task::Waker>,
    /// Tasks waiting to make a connection
    connectors: StreamSenderQueue,
    /// Pending transmit
    pending: Option<quinn_proto::Transmit>,
    /// The timer being used by this connection
    timer: Option<futures_timer::Delay>,
    /// The close reason, if this connection has been lost
    close_reason: Option<quinn_proto::ConnectionError>,
    /// Waker to wake up the driver
    waker: Option<std::task::Waker>,
    /// Channel for endpoint events
    endpoint_channel: mpsc::Sender<EndpointMessage>,
    /// Last timeout
    last_timeout: Option<Instant>,
    /// Join handle for the driver
    driver: Option<async_std::task::JoinHandle<Result<(), io::Error>>>,
    /// Close waker
    close_waker: Option<std::task::Waker>,
}

impl Drop for Muxer {
    fn drop(&mut self) {
        self.shutdown()
    }
}

impl Drop for QuicMuxer {
    fn drop(&mut self) {
        let inner = self.inner();
        debug!("dropping muxer with side {:?}", inner.connection.side());
        #[cfg(test)]
        assert!(
            !inner.connection.is_handshaking(),
            "dropped a connection that was still handshaking"
        );
    }
}

impl Muxer {
    fn wake_driver(&mut self) {
        if let Some(waker) = self.waker.take() {
            debug!("driver awoken!");
            waker.wake();
        }
    }

    fn driver(&mut self) -> &mut async_std::task::JoinHandle<Result<(), io::Error>> {
        self.driver
            .as_mut()
            .expect("we don’t call this until the driver is spawned; qed")
    }

    fn drive_timer(&mut self, cx: &mut Context, now: Instant) -> bool {
        let mut keep_going = false;
        loop {
            match self.connection.poll_timeout() {
                None => {
                    self.timer = None;
                    self.last_timeout = None
                }
                Some(t) if t <= now => {
                    self.connection.handle_timeout(now);
                    keep_going = true;
                    continue;
                }
                t if t == self.last_timeout => {}
                t => {
                    let delay = t.expect("already checked to be Some; qed") - now;
                    self.timer = Some(futures_timer::Delay::new(delay))
                }
            }
            break;
        }

        while let Some(ref mut timer) = self.timer {
            if timer.poll_unpin(cx).is_pending() {
                break;
            }
            self.connection.handle_timeout(now);
            keep_going = true
        }
        keep_going
    }

    fn new(endpoint: Endpoint, connection: Connection, handle: ConnectionHandle) -> Self {
        Muxer {
            close_waker: None,
            last_timeout: None,
            pending_stream: None,
            connection,
            handle,
            writers: HashMap::new(),
            readers: HashMap::new(),
            finishers: HashMap::new(),
            accept_waker: None,
            handshake_waker: None,
            connectors: Default::default(),
            endpoint_channel: endpoint.event_channel(),
            endpoint: endpoint,
            pending: None,
            timer: None,
            close_reason: None,
            waker: None,
            driver: None,
        }
    }

    /// Process all endpoint-facing events for this connection.  This is synchronous and will not
    /// fail.
    fn send_to_endpoint(&mut self, endpoint: &mut EndpointInner) {
        while let Some(endpoint_event) = self.connection.poll_endpoint_events() {
            if let Some(connection_event) = endpoint.handle_event(self.handle, endpoint_event) {
                self.connection.handle_event(connection_event)
            }
        }
    }

    /// Send endpoint events.  Returns true if and only if there are endpoint events remaining to
    /// be sent.
    fn poll_endpoint_events(&mut self, cx: &mut Context<'_>) -> bool {
        loop {
            match self.endpoint_channel.poll_ready(cx) {
                Pending => break true,
                Ready(Err(_)) => unreachable!("we have a reference to the peer; qed"),
                Ready(Ok(())) => {}
            }
            if let Some(event) = self.connection.poll_endpoint_events() {
                self.endpoint_channel
                    .start_send(EndpointMessage::EndpointEvent {
                        handle: self.handle,
                        event,
                    })
                    .expect(
                        "we checked in `pre_application_io` that this channel had space; \
                         that is always called first, and there is a lock preventing concurrency \
                         problems; qed",
                    )
            } else {
                break false;
            }
        }
    }

    fn pre_application_io(
        &mut self,
        now: Instant,
        cx: &mut Context<'_>,
    ) -> Result<bool, io::Error> {
        if let Some(transmit) = self.pending.take() {
            if self.poll_transmit(cx, transmit)? {
                return Ok(false);
            }
        }
        let mut needs_timer_update = false;
        while let Some(transmit) = self.connection.poll_transmit(now) {
            needs_timer_update = true;
            if self.poll_transmit(cx, transmit)? {
                break;
            }
        }
        Ok(needs_timer_update)
    }

    fn poll_transmit(
        &mut self,
        cx: &mut Context<'_>,
        transmit: quinn_proto::Transmit,
    ) -> Result<bool, io::Error> {
        loop {
            let res = self
                .endpoint
                .poll_send_to(cx, &transmit.contents, &transmit.destination);
            break match res {
                Pending => {
                    self.pending = Some(transmit);
                    Ok(true)
                }
                Ready(Ok(size)) => {
                    trace!("sent packet of length {} to {}", size, transmit.destination);
                    Ok(false)
                }
                Ready(Err(e)) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Ready(Err(e)) => Err(e),
            };
        }
    }

    fn shutdown(&mut self) {
        debug!("shutting connection down!");
        if let Some(w) = self.accept_waker.take() {
            w.wake()
        }
        if let Some(w) = self.handshake_waker.take() {
            w.wake()
        }
        for (_, v) in self.writers.drain() {
            v.wake();
        }
        for (_, v) in self.readers.drain() {
            v.wake();
        }
        for sender in self.finishers.drain().filter_map(|x| x.1) {
            drop(sender.send(()))
        }
        self.connectors.truncate(0);
        if !self.connection.is_closed() {
            self.connection
                .close(Instant::now(), Default::default(), Default::default());
            self.process_app_events();
        }
        self.wake_driver();
    }

    /// Process application events
    fn process_connection_events(&mut self, endpoint: &mut EndpointInner, event: ConnectionEvent) {
        self.connection.handle_event(event);
        self.process_endpoint_communication(endpoint)
    }

    fn process_endpoint_communication(&mut self, endpoint: &mut EndpointInner) {
        if self.connection.is_drained() {
            return;
        }
        self.send_to_endpoint(endpoint);
        self.process_app_events();
        self.wake_driver();
        assert!(self.connection.poll_endpoint_events().is_none());
        assert!(self.connection.poll().is_none());
    }

    fn get_pending_stream(&mut self) -> Option<StreamId> {
        self.wake_driver();
        if let Some(id) = self.pending_stream.take() {
            Some(id)
        } else {
            self.connection.open(Dir::Bi)
        }
        .map(|id| {
            self.finishers.insert(id, None);
            id
        })
    }

    pub fn process_app_events(&mut self) -> bool {
        use quinn_proto::Event;
        let mut keep_going = false;
        'a: while let Some(event) = self.connection.poll() {
            keep_going = true;
            match event {
                Event::StreamOpened { dir: Dir::Uni } | Event::DatagramReceived => {
                    panic!("we disabled incoming unidirectional streams and datagrams")
                }
                Event::StreamAvailable { dir: Dir::Uni } => {
                    panic!("we don’t use unidirectional streams")
                }
                Event::StreamReadable { stream } => {
                    trace!("Stream {:?} readable", stream);
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.readers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamWritable { stream } => {
                    trace!("Stream {:?} writable", stream);
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamAvailable { dir: Dir::Bi } => {
                    trace!("Bidirectional stream available");
                    if self.connectors.is_empty() {
                        // no task to wake up
                        continue;
                    }
                    assert!(
                        self.pending_stream.is_none(),
                        "we cannot have both pending tasks and a pending stream; qed"
                    );
                    let stream = self.connection.open(Dir::Bi)
                            .expect("we just were told that there is a stream available; there is a mutex that prevents other threads from calling open() in the meantime; qed");
                    while let Some(oneshot) = self.connectors.pop_front() {
                        match oneshot.send(stream) {
                            Ok(()) => continue 'a,
                            Err(_) => {}
                        }
                    }
                    self.pending_stream = Some(stream)
                }
                Event::ConnectionLost { reason } => {
                    debug!("lost connection due to {:?}", reason);
                    assert!(self.connection.is_closed());
                    self.close_reason = Some(reason);
                    self.close_waker.take().map(|e| e.wake());
                    self.shutdown();
                }
                Event::StreamFinished { stream, .. } => {
                    // Wake up the task waiting on us (if any)
                    debug!("Stream {:?} finished!", stream);
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                    if let (_, Some(sender)) = self.finishers.remove_entry(&stream).expect(
                        "every write stream is placed in this map, and entries are removed \
                         exactly once; qed",
                    ) {
                        drop(sender.send(()))
                    }
                    if self.finishers.is_empty() {
                        self.close_waker.take().map(|e| e.wake());
                    }
                }
                Event::Connected => {
                    debug!("connected!");
                    assert!(!self.connection.is_handshaking(), "quinn-proto bug");
                    if let Some(w) = self.handshake_waker.take() {
                        w.wake()
                    }
                }
                Event::StreamOpened { dir: Dir::Bi } => {
                    debug!("stream opened!");
                    if let Some(w) = self.accept_waker.take() {
                        w.wake()
                    }
                }
            }
        }
        keep_going
    }
}

#[derive(Debug)]
struct ConnectionDriver {
    inner: Arc<Mutex<Muxer>>,
    endpoint: Endpoint,
    outgoing_packet: Option<quinn_proto::Transmit>,
}

impl Unpin for ConnectionDriver {}

impl ConnectionDriver {
    fn new(muxer: Muxer) -> (Self, Arc<Mutex<Muxer>>) {
        let endpoint = muxer.endpoint.clone();
        let inner = Arc::new(Mutex::new(muxer));
        (
            Self {
                inner: inner.clone(),
                endpoint,
                outgoing_packet: None,
            },
            inner,
        )
    }
}

impl Future for ConnectionDriver {
    type Output = Result<(), io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // cx.waker().wake_by_ref();
        let this = self.get_mut();
        debug!("being polled for timer!");
        let mut inner = this.inner.lock();
        inner.waker = Some(cx.waker().clone());
        let now = Instant::now();
        loop {
            let mut needs_timer_update = false;
            needs_timer_update |= inner.drive_timer(cx, now);
            needs_timer_update |= inner.pre_application_io(now, cx)?;
            needs_timer_update |= inner.process_app_events();
            let needs_to_send_endpoint_events = inner.poll_endpoint_events(cx);
            if inner.connection.is_drained() {
                break Ready(
                    match inner
                        .close_reason
                        .clone()
                        .expect("we never have a closed connection with no reason; qed")
                    {
                        quinn_proto::ConnectionError::LocallyClosed => {
                            if needs_timer_update {
                                debug!("continuing until all events are finished");
                                continue;
                            } else {
                                debug!("exiting driver");
                                Ok(())
                            }
                        }
                        e @ quinn_proto::ConnectionError::TimedOut => {
                            Err(io::Error::new(io::ErrorKind::TimedOut, e))
                        }
                        e @ quinn_proto::ConnectionError::Reset => {
                            Err(io::Error::new(io::ErrorKind::ConnectionReset, e))
                        }
                        e @ quinn_proto::ConnectionError::TransportError { .. } => {
                            Err(io::Error::new(io::ErrorKind::InvalidData, e))
                        }
                        e @ quinn_proto::ConnectionError::ConnectionClosed { .. } => {
                            Err(io::Error::new(io::ErrorKind::ConnectionAborted, e))
                        }
                        e => Err(io::Error::new(io::ErrorKind::Other, e)),
                    },
                );
            } else if !needs_timer_update {
                break if inner.close_reason.is_none() {
                    Pending
                } else if needs_to_send_endpoint_events {
                    debug!("still have endpoint events to send!");
                    Pending
                } else {
                    Ready(Ok(()))
                };
            }
        }
    }
}
