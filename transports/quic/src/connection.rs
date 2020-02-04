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
use super::{
    certificate,
    endpoint::{EndpointData, EndpointInner},
    error::Error,
    socket::Pending,
    verifier,
};
use async_macros::ready;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_core::StreamMuxer;
use log::{debug, error, info, trace, warn};
use parking_lot::{Mutex, MutexGuard};
use quinn_proto::{Connection, ConnectionEvent, ConnectionHandle, Dir, StreamId};
use std::{
    collections::HashMap,
    io,
    mem::replace,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll},
    time::Instant,
};
#[derive(Debug)]
pub struct Substream {
    id: StreamId,
    status: SubstreamStatus,
}

#[derive(Debug)]
enum SubstreamStatus {
    Unwritten,
    Live,
    Finishing(oneshot::Receiver<()>),
    Finished,
}

impl Substream {
    fn unwritten(id: StreamId) -> Self {
        let status = SubstreamStatus::Unwritten;
        Self { id, status }
    }

    fn live(id: StreamId) -> Self {
        let status = SubstreamStatus::Live;
        Self { id, status }
    }

    fn is_live(&self) -> bool {
        match self.status {
            SubstreamStatus::Live | SubstreamStatus::Unwritten => true,
            SubstreamStatus::Finishing(_) | SubstreamStatus::Finished => false,
        }
    }
}

/// Represents the configuration for a QUIC/UDP/IP transport capability for libp2p.
///
/// The QUIC endpoints created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through a reactor.
#[derive(Debug, Clone)]
pub struct Config {
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
    use std::time::Duration;
    transport.keep_alive_interval(Some(Duration::from_millis(1000)));
    let mut crypto = rustls::ClientConfig::new();
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto.enable_early_data = true;
    crypto.set_single_client_cert(vec![certificate], key);
    let verifier = verifier::VeryInsecureRequireExactlyOneSelfSignedServerCertificate;
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
        verifier::VeryInsecureRequireExactlyOneSelfSignedClientCertificate,
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

impl Config {
    /// Creates a new configuration object for TCP/IP.
    pub fn new(keypair: &libp2p_core::identity::Keypair) -> Self {
        let cert = super::make_cert(&keypair);
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
pub(super) enum EndpointMessage {
    Dummy,
    ConnectionAccepted,
    EndpointEvent {
        handle: ConnectionHandle,
        event: quinn_proto::EndpointEvent,
    },
}

#[derive(Debug, Clone)]
pub struct QuicMuxer(Arc<Mutex<Muxer>>);

impl QuicMuxer {
    fn inner(&self) -> MutexGuard<Muxer> {
        self.0.lock()
    }
}

#[derive(Debug)]
enum OutboundInner {
    Complete(Result<StreamId, Error>),
    Pending(oneshot::Receiver<StreamId>),
    Done,
}

pub struct Outbound(OutboundInner);

impl Future for Outbound {
    type Output = Result<Substream, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;
        match this.0 {
            OutboundInner::Complete(_) => match replace(&mut this.0, OutboundInner::Done) {
                OutboundInner::Complete(e) => Poll::Ready(e.map(Substream::unwritten)),
                _ => unreachable!(),
            },
            OutboundInner::Pending(ref mut receiver) => {
                let result = ready!(receiver.poll_unpin(cx))
                    .map(Substream::unwritten)
                    .map_err(|oneshot::Canceled| Error::ConnectionLost);
                this.0 = OutboundInner::Done;
                Poll::Ready(result)
            }
            OutboundInner::Done => panic!("polled after yielding Ready"),
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = Outbound;
    type Substream = Substream;
    type Error = crate::error::Error;
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let mut inner = self.inner();
        if let Some(ref e) = inner.close_reason {
            Outbound(OutboundInner::Complete(Err(Error::ConnectionError(
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
        if let Some(waker) = inner.writers.remove(&substream.id) {
            waker.wake();
        }
        if let Some(waker) = inner.readers.remove(&substream.id) {
            waker.wake();
        }
        if substream.is_live() && inner.close_reason.is_none() {
            if let Err(e) = inner.connection.finish(substream.id) {
                warn!("Error closing stream: {}", e);
            }
        }
        let _ = inner
            .connection
            .stop_sending(substream.id, Default::default());
    }
    fn is_remote_acknowledged(&self) -> bool {
        true
    }

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
        debug!("being polled for inbound connections!");
        let mut inner = self.inner();
        if inner.connection.is_drained() {
            return Poll::Ready(Err(Error::ConnectionError(
                inner
                    .close_reason
                    .clone()
                    .expect("closed connections always have a reason; qed"),
            )));
        }
        inner.wake_driver();
        match inner.connection.accept(quinn_proto::Dir::Bi) {
            None => {
                if let Some(waker) = replace(
                    &mut inner.handshake_or_accept_waker,
                    Some(cx.waker().clone()),
                ) {
                    waker.wake()
                }
                Poll::Pending
            }
            Some(id) => {
                inner.finishers.insert(id, None);
                Poll::Ready(Ok(Substream::live(id)))
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
            error!(
                "The application used stream {:?} after it was no longer live",
                substream.id
            );
            return Poll::Ready(Err(Error::ExpiredStream));
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut inner = self.inner();
        debug_assert!(
            inner.finishers.get(&substream.id).is_some(),
            "no entry in finishers map for write stream"
        );
        inner.wake_driver();
        if let Some(ref e) = inner.close_reason {
            return Poll::Ready(Err(Error::ConnectionError(e.clone())));
        }
        match inner.connection.write(substream.id, buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(WriteError::Blocked) => {
                if let Some(ref e) = inner.close_reason {
                    return Poll::Ready(Err(Error::ConnectionError(e.clone())));
                }
                if let Some(w) = inner.writers.insert(substream.id, cx.waker().clone()) {
                    w.wake();
                }
                Poll::Pending
            }
            Err(WriteError::UnknownStream) => {
                if let Some(e) = &inner.close_reason {
                    error!(
                        "The application used a connection that is already being \
                        closed. This is a bug in the application or in libp2p."
                    );
                    Poll::Ready(Err(Error::ConnectionError(e.clone())))
                } else {
                    error!(
                        "The application used a stream that has already been \
                        closed. This is a bug in the application or in libp2p."
                    );
                    Poll::Ready(Err(Error::ExpiredStream))
                }
            }
            Err(WriteError::Stopped(e)) => {
                inner.finishers.remove(&substream.id);
                if let Some(w) = inner.writers.remove(&substream.id) {
                    w.wake()
                }
                substream.status = SubstreamStatus::Finished;
                Poll::Ready(Err(Error::Stopped(e)))
            }
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
        match inner.connection.read(substream.id, buf) {
            Ok(Some(bytes)) => Poll::Ready(Ok(bytes)),
            Ok(None) => Poll::Ready(Ok(0)),
            Err(ReadError::Blocked) => {
                if let Some(error) = &inner.close_reason {
                    Poll::Ready(Err(Error::ConnectionError(error.clone())))
                } else if let SubstreamStatus::Unwritten = substream.status {
                    Poll::Ready(Err(Error::CannotReadFromUnwrittenStream))
                } else {
                    trace!(
                        "Blocked on reading stream {:?} with side {:?}",
                        substream.id,
                        inner.connection.side()
                    );
                    if let Some(w) = inner.readers.insert(substream.id, cx.waker().clone()) {
                        w.wake()
                    }
                    Poll::Pending
                }
            }
            Err(ReadError::UnknownStream) => {
                error!(
                    "The application used a stream that has already been closed. This is a bug."
                );
                Poll::Ready(Err(Error::ExpiredStream))
            }
            Err(ReadError::Reset(e)) => {
                if let Some(w) = inner.readers.remove(&substream.id) {
                    w.wake()
                }
                Poll::Ready(Err(Error::Reset(e)))
            }
        }
    }

    fn shutdown_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        match substream.status {
            SubstreamStatus::Finished => return Poll::Ready(Ok(())),
            SubstreamStatus::Finishing(ref mut channel) => {
                self.inner().wake_driver();
                return channel
                    .poll_unpin(cx)
                    .map_err(|e| Error::IO(io::Error::new(io::ErrorKind::ConnectionAborted, e)));
            }
            SubstreamStatus::Unwritten | SubstreamStatus::Live => {}
        }
        let mut inner = self.inner();
        inner.wake_driver();
        inner.connection.finish(substream.id).map_err(|e| match e {
            quinn_proto::FinishError::UnknownStream => unreachable!("we checked for this above!"),
            quinn_proto::FinishError::Stopped(e) => Error::Stopped(e),
        })?;
        let (sender, mut receiver) = oneshot::channel();
        assert!(
            receiver.poll_unpin(cx).is_pending(),
            "we haven’t written to the peer yet"
        );
        substream.status = SubstreamStatus::Finishing(receiver);
        match inner.finishers.insert(substream.id, Some(sender)) {
            Some(None) => {}
            _ => unreachable!(
                "We inserted a None value earlier; and haven’t touched this entry since; qed"
            ),
        }
        Poll::Pending
    }

    fn flush_substream(
        &self,
        _cx: &mut Context,
        _substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn flush_all(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn close(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        trace!("close() called");
        let mut inner = self.inner();
        if inner.connection.is_closed() || inner.close_reason.is_some() {
            return Poll::Ready(Ok(()));
        } else if inner.finishers.is_empty() {
            inner.shutdown(0);
            inner.close_reason = Some(quinn_proto::ConnectionError::LocallyClosed);
            drop(inner.driver().poll_unpin(cx));
            return Poll::Ready(Ok(()));
        }
        if let Some(waker) = replace(&mut inner.close_waker, Some(cx.waker().clone())) {
            waker.wake();
            return Poll::Pending;
        }
        let Muxer {
            finishers,
            connection,
            ..
        } = &mut *inner;
        for (id, channel) in finishers {
            if channel.is_none() {
                match connection.finish(*id) {
                    Ok(()) => {}
                    Err(error) => warn!("Finishing stream {:?} failed: {}", id, error),
                }
            }
        }
        Poll::Pending
    }
}

#[derive(Debug)]
pub struct Upgrade {
    muxer: Option<QuicMuxer>,
}

#[cfg(test)]
impl Drop for Upgrade {
    fn drop(&mut self) {
        debug!("dropping upgrade!");
        assert!(
            self.muxer.is_none(),
            "dropped before being polled to completion"
        );
    }
}

impl Future for Upgrade {
    type Output = Result<(libp2p_core::PeerId, QuicMuxer), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let muxer = &mut self.get_mut().muxer;
        trace!("outbound polling!");
        let res = {
            let mut inner = muxer.as_mut().expect("polled after yielding Ready").inner();
            if inner.connection.is_drained() {
                return Poll::Ready(Err(Error::ConnectionError(
                    inner
                        .close_reason
                        .clone()
                        .expect("closed connections always have a reason; qed"),
                )));
            } else if inner.connection.is_handshaking() {
                assert!(
                    !inner.connection.is_closed(),
                    "a closed connection cannot be handshaking; qed"
                );
                inner.handshake_or_accept_waker = Some(cx.waker().clone());
                return Poll::Pending;
            } else if inner.connection.side().is_server() {
                ready!(inner.endpoint_channel.poll_ready(cx))
                    .expect("we have a reference to the peer; qed");
                inner
                    .endpoint_channel
                    .start_send(EndpointMessage::ConnectionAccepted)
                    .expect("we just checked that we have capacity to send this; qed")
            }

            let peer_certificates: Vec<rustls::Certificate> = inner
                .connection
                .crypto_session()
                .get_peer_certificates()
                .expect("we have finished handshaking, so we have exactly one certificate; qed");
            certificate::verify_libp2p_certificate(
                peer_certificates
                    .get(0)
                    .expect(
                        "our certificate verifiers require exactly one \
                         certificate to be presented, so an empty certificate \
                         chain would already have been rejected; qed",
                    )
                    .as_ref(),
            )
        };
        let muxer = muxer.take().expect("polled after yielding Ready");
        Poll::Ready(match res {
            Ok(e) => Ok((e, muxer)),
            Err(_) => Err(Error::BadCertificate(ring::error::Unspecified)),
        })
    }
}

type StreamSenderQueue = std::collections::VecDeque<oneshot::Sender<StreamId>>;

#[derive(Debug)]
pub(crate) struct Muxer {
    /// The pending stream, if any.
    pending_stream: Option<StreamId>,
    /// The associated endpoint
    endpoint: Arc<EndpointData>,
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
    /// Task waiting for new connections, or for the connection to finish handshaking.
    handshake_or_accept_waker: Option<std::task::Waker>,
    /// Tasks waiting to make a connection
    connectors: StreamSenderQueue,
    /// Pending transmit
    pending: super::socket::Pending,
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
    driver: Option<async_std::task::JoinHandle<Result<(), Error>>>,
    /// Close waker
    close_waker: Option<std::task::Waker>,
    /// Have we gotten a connection lost event?
    connection_lost: bool,
}

const RESET: u32 = 1;

impl Drop for Muxer {
    fn drop(&mut self) {
        if self.close_reason.is_none() {
            warn!("connection uncleanly closed!");
            self.shutdown(RESET)
        }
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

    fn wake_incoming(&mut self) {
        if let Some(waker) = self.handshake_or_accept_waker.take() {
            waker.wake()
        }
    }

    fn driver(&mut self) -> &mut async_std::task::JoinHandle<Result<(), Error>> {
        self.driver
            .as_mut()
            .expect("we don’t call this until the driver is spawned; qed")
    }

    fn drive_timer(&mut self, cx: &mut Context, now: Instant) -> bool {
        match self.connection.poll_timeout() {
            None => {
                self.timer = None;
                self.last_timeout = None
            }
            Some(t) if t <= now => {
                self.connection.handle_timeout(now);
                return true;
            }
            t if t == self.last_timeout => {}
            Some(t) => self.timer = Some(futures_timer::Delay::new(t - now)),
        }
        if let Some(ref mut timer) = self.timer {
            if timer.poll_unpin(cx).is_ready() {
                self.connection.handle_timeout(now);
                return true;
            }
        }
        false
    }

    fn new(endpoint: Arc<EndpointData>, connection: Connection, handle: ConnectionHandle) -> Self {
        Muxer {
            connection_lost: false,
            close_waker: None,
            last_timeout: None,
            pending_stream: None,
            connection,
            handle,
            writers: HashMap::new(),
            readers: HashMap::new(),
            finishers: HashMap::new(),
            handshake_or_accept_waker: None,
            connectors: Default::default(),
            endpoint_channel: endpoint.event_channel(),
            endpoint,
            pending: Pending::default(),
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
    fn poll_endpoint_events(&mut self, cx: &mut Context<'_>) {
        loop {
            match self.endpoint_channel.poll_ready(cx) {
                Poll::Pending => break,
                Poll::Ready(Err(_)) => unreachable!("we have a reference to the peer; qed"),
                Poll::Ready(Ok(())) => {}
            }
            if let Some(event) = self.connection.poll_endpoint_events() {
                self.endpoint_channel
                    .start_send(EndpointMessage::EndpointEvent {
                        handle: self.handle,
                        event,
                    })
                    .expect("we just checked that we have capacity; qed");
            } else {
                break;
            }
        }
    }

    fn transmit_pending(&mut self, now: Instant, cx: &mut Context<'_>) -> Result<(), Error> {
        let Self {
            pending,
            endpoint,
            connection,
            ..
        } = self;
        let _ =
            pending.send_packet(cx, endpoint.socket(), &mut || connection.poll_transmit(now))?;
        Ok(())
    }

    fn shutdown(&mut self, error_code: u32) {
        debug!("shutting connection down!");
        self.wake_incoming();
        for (_, v) in self.writers.drain() {
            v.wake();
        }
        for (_, v) in self.readers.drain() {
            v.wake();
        }
        for sender in self.finishers.drain().filter_map(|x| x.1) {
            let _ = sender.send(());
        }
        self.connectors.truncate(0);
        if !self.connection.is_closed() {
            self.connection.close(
                Instant::now(),
                quinn_proto::VarInt::from_u32(error_code),
                Default::default(),
            );
            self.process_app_events();
        }
        self.wake_driver();
    }

    /// Process application events
    pub(crate) fn process_connection_events(
        &mut self,
        endpoint: &mut EndpointInner,
        event: Option<ConnectionEvent>,
    ) {
        if let Some(event) = event {
            self.connection.handle_event(event);
        }
        if self.connection.is_drained() {
            return;
        }
        self.send_to_endpoint(endpoint);
        self.process_app_events();
        self.wake_driver();
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

    fn process_app_events(&mut self) -> bool {
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
                    trace!(
                        "Stream {:?} readable for side {:?}",
                        stream,
                        self.connection.side()
                    );
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.readers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamWritable { stream } => {
                    trace!(
                        "Stream {:?} writable for side {:?}",
                        stream,
                        self.connection.side()
                    );
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamAvailable { dir: Dir::Bi } => {
                    trace!(
                        "Bidirectional stream available for side {:?}",
                        self.connection.side()
                    );
                    if self.connectors.is_empty() {
                        // no task to wake up
                        continue;
                    }
                    assert!(
                        self.pending_stream.is_none(),
                        "we cannot have both pending tasks and a pending stream; qed"
                    );
                    let stream = self.connection.open(Dir::Bi).expect(
                        "we just were told that there is a stream available; there is \
                        a mutex that prevents other threads from calling open() in the \
                        meantime; qed",
                    );
                    while let Some(oneshot) = self.connectors.pop_front() {
                        if let Ok(()) = oneshot.send(stream) {
                            continue 'a;
                        }
                    }
                    self.pending_stream = Some(stream)
                }
                Event::ConnectionLost { reason } => {
                    debug!("lost connection due to {:?}", reason);
                    debug_assert!(
                        self.connection.is_closed(),
                        "connection lost without being closed"
                    );
                    self.close_reason = Some(reason);
                    self.connection_lost = true;
                    if let Some(e) = self.close_waker.take() {
                        e.wake()
                    }
                    self.shutdown(0);
                }
                Event::StreamFinished {
                    stream,
                    stop_reason,
                } => {
                    // Wake up the task waiting on us (if any)
                    trace!(
                        "Stream {:?} finished for side {:?} because of {:?}",
                        stream,
                        self.connection.side(),
                        stop_reason
                    );
                    if let Some(waker) = self.writers.remove(&stream) {
                        waker.wake()
                    }
                    if let Some(sender) = self.finishers.remove(&stream).expect(
                        "every write stream is placed in this map, and entries are removed \
                         exactly once; qed",
                    ) {
                        let _ = sender.send(());
                    }
                    if self.finishers.is_empty() {
                        if let Some(e) = self.close_waker.take() {
                            e.wake()
                        }
                    }
                }
                Event::Connected => {
                    debug!("connected!");
                    self.wake_incoming();
                }
                Event::StreamOpened { dir: Dir::Bi } => {
                    debug!("stream opened for side {:?}", self.connection.side());
                    self.wake_incoming()
                }
            }
        }
        keep_going
    }
}

#[derive(Debug)]
pub(super) struct ConnectionDriver {
    inner: Arc<Mutex<Muxer>>,
    outgoing_packet: Option<quinn_proto::Transmit>,
}

impl ConnectionDriver {
    pub(crate) fn spawn<T: FnOnce(Weak<Mutex<Muxer>>)>(
        endpoint: Arc<EndpointData>,
        connection: Connection,
        handle: ConnectionHandle,
        cb: T,
    ) -> Upgrade {
        let inner = Arc::new(Mutex::new(Muxer::new(endpoint, connection, handle)));
        cb(Arc::downgrade(&inner));
        let handle = async_std::task::spawn(Self {
            inner: inner.clone(),
            outgoing_packet: None,
        });
        inner.lock().driver = Some(handle);
        Upgrade {
            muxer: Some(QuicMuxer(inner)),
        }
    }
}

impl Future for ConnectionDriver {
    type Output = Result<(), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        debug!("being polled for timer!");
        let mut inner = this.inner.lock();
        inner.waker = Some(cx.waker().clone());
        loop {
            let now = Instant::now();
            inner.transmit_pending(now, cx)?;
            inner.process_app_events();
            inner.poll_endpoint_events(cx);
            if inner.drive_timer(cx, now) {
                continue;
            }
            if !inner.connection.is_drained() {
                break Poll::Pending;
            }
            info!("exiting driver");
            let close_reason = inner
                .close_reason
                .clone()
                .expect("we never have a closed connection with no reason; qed");
            break Poll::Ready(match close_reason {
                quinn_proto::ConnectionError::LocallyClosed => Ok(()),
                e => Err(e.into()),
            });
        }
    }
}
