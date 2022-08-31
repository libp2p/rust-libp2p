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

mod error;

pub(crate) mod pool;

pub use error::{
    ConnectionError, PendingConnectionError, PendingInboundConnectionError,
    PendingOutboundConnectionError,
};
pub use pool::{ConnectionCounters, ConnectionLimits};
pub use pool::{EstablishedConnection, PendingConnection};

use crate::handler::ConnectionHandler;
use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend, SendWrapper};
use crate::{
    ConnectionHandlerEvent, ConnectionHandlerUpgrErr, IntoConnectionHandler, KeepAlive,
    SubstreamProtocol,
};
use futures::stream::FuturesUnordered;
use futures::FutureExt;
use futures::StreamExt;
use futures_timer::Delay;
use instant::Instant;
use libp2p_core::connection::ConnectedPoint;
use libp2p_core::multiaddr::Multiaddr;
use libp2p_core::muxing::{StreamMuxerBox, StreamMuxerEvent, StreamMuxerExt, SubstreamBox};
use libp2p_core::upgrade::{InboundUpgradeApply, OutboundUpgradeApply};
use libp2p_core::PeerId;
use libp2p_core::{upgrade, UpgradeError};
use std::collections::VecDeque;
use std::future::Future;
use std::{fmt, io, pin::Pin, task::Context, task::Poll};

/// Information about a successfully established connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    /// The connected endpoint, including network address information.
    pub endpoint: ConnectedPoint,
    /// Information obtained from the transport.
    pub peer_id: PeerId,
}

/// Event generated by a [`Connection`].
#[derive(Debug, Clone)]
pub enum Event<T> {
    /// Event generated by the [`ConnectionHandler`].
    Handler(T),
    /// Address of the remote has changed.
    AddressChange(Multiaddr),
}

/// A multiplexed connection to a peer with an associated [`ConnectionHandler`].
pub struct Connection<THandler>
where
    THandler: ConnectionHandler,
{
    /// Node that handles the muxing.
    muxing: StreamMuxerBox,
    /// The underlying handler.
    handler: THandler,
    /// Futures that upgrade incoming substreams.
    negotiating_in: FuturesUnordered<
        SubstreamUpgrade<
            THandler::InboundOpenInfo,
            InboundUpgradeApply<SubstreamBox, SendWrapper<THandler::InboundProtocol>>,
        >,
    >,
    /// Futures that upgrade outgoing substreams.
    negotiating_out: FuturesUnordered<
        SubstreamUpgrade<
            THandler::OutboundOpenInfo,
            OutboundUpgradeApply<SubstreamBox, SendWrapper<THandler::OutboundProtocol>>,
        >,
    >,
    /// The currently planned connection & handler shutdown.
    shutdown: Shutdown,
    /// The substream upgrade protocol override, if any.
    substream_upgrade_protocol_override: Option<upgrade::Version>,
    /// The maximum number of inbound streams concurrently negotiating on a
    /// connection. New inbound streams exceeding the limit are dropped and thus
    /// reset.
    ///
    /// Note: This only enforces a limit on the number of concurrently
    /// negotiating inbound streams. The total number of inbound streams on a
    /// connection is the sum of negotiating and negotiated streams. A limit on
    /// the total number of streams can be enforced at the
    /// [`StreamMuxerBox`](libp2p_core::muxing::StreamMuxerBox) level.
    max_negotiating_inbound_streams: usize,
    /// For each outbound substream request, how to upgrade it.
    pending_dial_upgrades:
        VecDeque<SubstreamProtocol<THandler::OutboundProtocol, THandler::OutboundOpenInfo>>,
}

impl<THandler> fmt::Debug for Connection<THandler>
where
    THandler: ConnectionHandler + fmt::Debug,
    THandler::OutboundOpenInfo: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("handler", &self.handler)
            .finish()
    }
}

impl<THandler> Unpin for Connection<THandler> where THandler: ConnectionHandler {}

impl<THandler> Connection<THandler>
where
    THandler: ConnectionHandler,
{
    /// Builds a new `Connection` from the given substream multiplexer
    /// and connection handler.
    pub fn new(
        peer_id: PeerId,
        endpoint: ConnectedPoint,
        muxer: StreamMuxerBox,
        handler: impl IntoConnectionHandler<Handler = THandler>,
        substream_upgrade_protocol_override: Option<upgrade::Version>,
        max_negotiating_inbound_streams: usize,
    ) -> Self {
        Connection {
            muxing: muxer,
            handler: handler.into_handler(&peer_id, &endpoint),
            negotiating_in: Default::default(),
            negotiating_out: Default::default(),
            shutdown: Shutdown::None,
            substream_upgrade_protocol_override,
            max_negotiating_inbound_streams,
            pending_dial_upgrades: VecDeque::with_capacity(8),
        }
    }

    /// Notifies the connection handler of an event.
    pub fn inject_event(&mut self, event: THandler::InEvent) {
        self.handler.inject_event(event);
    }

    /// Begins an orderly shutdown of the connection, returning the connection
    /// handler and a `Future` that resolves when connection shutdown is complete.
    pub fn close(self) -> (THandler, impl Future<Output = io::Result<()>>) {
        (self.handler, self.muxing.close())
    }

    /// Polls the handler and the substream, forwarding events from the former to the latter and
    /// vice versa.
    pub fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Event<THandler::OutEvent>, ConnectionError<THandler::Error>>> {
        loop {
            // Poll the [`ConnectionHandler`].
            if let Poll::Ready(handler_event) = self.handler.poll(cx) {
                match handler_event {
                    ConnectionHandlerEvent::Custom(event) => {
                        return Poll::Ready(Ok(Event::Handler(event)))
                    }
                    ConnectionHandlerEvent::Close(err) => {
                        return Poll::Ready(Err(ConnectionError::Handler(err)))
                    }
                    ConnectionHandlerEvent::OutboundSubstreamRequest { protocol } => {
                        self.pending_dial_upgrades.push_back(protocol);

                        continue;
                    }
                }
            }

            // In case the [`ConnectionHandler`] can not make any more progress, poll the negotiating outbound streams.
            if let Poll::Ready(Some((user_data, res))) = self.negotiating_out.poll_next_unpin(cx) {
                match res {
                    Ok(upgrade) => self
                        .handler
                        .inject_fully_negotiated_outbound(upgrade, user_data),
                    Err(err) => self.handler.inject_dial_upgrade_error(user_data, err),
                }

                // After the `inject_*` calls, the [`ConnectionHandler`] might be able to make progress.
                continue;
            }

            // In case both the [`ConnectionHandler`] and the negotiating outbound streams can not
            // make any more progress, poll the negotiating inbound streams.
            if let Poll::Ready(Some((user_data, res))) = self.negotiating_in.poll_next_unpin(cx) {
                match res {
                    Ok(upgrade) => self
                        .handler
                        .inject_fully_negotiated_inbound(upgrade, user_data),
                    Err(err) => self.handler.inject_listen_upgrade_error(user_data, err),
                }

                // After the `inject_*` calls, the [`ConnectionHandler`] might be able to make progress.
                continue;
            }

            // Ask the handler whether it wants the connection (and the handler itself)
            // to be kept alive, which determines the planned shutdown, if any.
            let keep_alive = self.handler.connection_keep_alive();
            match (&mut self.shutdown, keep_alive) {
                (Shutdown::Later(timer, deadline), KeepAlive::Until(t)) => {
                    if *deadline != t {
                        *deadline = t;
                        if let Some(dur) = deadline.checked_duration_since(Instant::now()) {
                            timer.reset(dur)
                        }
                    }
                }
                (_, KeepAlive::Until(t)) => {
                    if let Some(dur) = t.checked_duration_since(Instant::now()) {
                        self.shutdown = Shutdown::Later(Delay::new(dur), t)
                    }
                }
                (_, KeepAlive::No) => self.shutdown = Shutdown::Asap,
                (_, KeepAlive::Yes) => self.shutdown = Shutdown::None,
            };

            // Check if the connection (and handler) should be shut down.
            // As long as we're still negotiating substreams, shutdown is always postponed.
            if self.negotiating_in.is_empty() && self.negotiating_out.is_empty() {
                match self.shutdown {
                    Shutdown::None => {}
                    Shutdown::Asap => return Poll::Ready(Err(ConnectionError::KeepAliveTimeout)),
                    Shutdown::Later(ref mut delay, _) => match Future::poll(Pin::new(delay), cx) {
                        Poll::Ready(_) => {
                            return Poll::Ready(Err(ConnectionError::KeepAliveTimeout))
                        }
                        Poll::Pending => {}
                    },
                }
            }

            match self.muxing.poll_unpin(cx)? {
                Poll::Pending => {}
                Poll::Ready(StreamMuxerEvent::AddressChange(address)) => {
                    self.handler.inject_address_change(&address);
                    return Poll::Ready(Ok(Event::AddressChange(address)));
                }
            }

            if !self.pending_dial_upgrades.is_empty() {
                match self.muxing.poll_outbound_unpin(cx)? {
                    Poll::Pending => {}
                    Poll::Ready(substream) => {
                        let protocol = self
                            .pending_dial_upgrades
                            .pop_front()
                            .expect("`open_info` is not empty");

                        self.negotiating_out.push(SubstreamUpgrade::new_outbound(
                            substream,
                            protocol,
                            self.substream_upgrade_protocol_override,
                        ));

                        continue; // Go back to the top, handler can potentially make progress again.
                    }
                }
            }

            if self.negotiating_in.len() < self.max_negotiating_inbound_streams {
                match self.muxing.poll_inbound_unpin(cx)? {
                    Poll::Pending => {}
                    Poll::Ready(substream) => {
                        let protocol = self.handler.listen_protocol();

                        self.negotiating_in
                            .push(SubstreamUpgrade::new_inbound(substream, protocol));

                        continue; // Go back to the top, handler can potentially make progress again.
                    }
                }
            }

            return Poll::Pending; // Nothing can make progress, return `Pending`.
        }
    }
}

/// Borrowed information about an incoming connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct IncomingInfo<'a> {
    /// Local connection address.
    pub local_addr: &'a Multiaddr,
    /// Address used to send back data to the remote.
    pub send_back_addr: &'a Multiaddr,
}

impl<'a> IncomingInfo<'a> {
    /// Builds the [`ConnectedPoint`] corresponding to the incoming connection.
    pub fn create_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Listener {
            local_addr: self.local_addr.clone(),
            send_back_addr: self.send_back_addr.clone(),
        }
    }
}

/// Information about a connection limit.
#[derive(Debug, Clone)]
pub struct ConnectionLimit {
    /// The maximum number of connections.
    pub limit: u32,
    /// The current number of connections.
    pub current: u32,
}

impl fmt::Display for ConnectionLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.current, self.limit)
    }
}

/// A `ConnectionLimit` can represent an error if it has been exceeded.
impl std::error::Error for ConnectionLimit {}

struct SubstreamUpgrade<UserData, Upgrade> {
    user_data: Option<UserData>,
    timeout: Delay,
    upgrade: Upgrade,
}

impl<UserData, Upgrade>
    SubstreamUpgrade<UserData, OutboundUpgradeApply<SubstreamBox, SendWrapper<Upgrade>>>
where
    Upgrade: Send + OutboundUpgradeSend,
{
    fn new_outbound(
        substream: SubstreamBox,
        protocol: SubstreamProtocol<Upgrade, UserData>,
        version_override: Option<upgrade::Version>,
    ) -> Self {
        let timeout = *protocol.timeout();
        let (upgrade, open_info) = protocol.into_upgrade();

        let effective_version = match version_override {
            Some(version_override) if version_override != upgrade::Version::default() => {
                log::debug!(
                    "Substream upgrade protocol override: {:?} -> {:?}",
                    upgrade::Version::default(),
                    version_override
                );

                version_override
            }
            _ => upgrade::Version::default(),
        };

        Self {
            user_data: Some(open_info),
            timeout: Delay::new(timeout),
            upgrade: upgrade::apply_outbound(substream, SendWrapper(upgrade), effective_version),
        }
    }
}

impl<UserData, Upgrade>
    SubstreamUpgrade<UserData, InboundUpgradeApply<SubstreamBox, SendWrapper<Upgrade>>>
where
    Upgrade: Send + InboundUpgradeSend,
{
    fn new_inbound(
        substream: SubstreamBox,
        protocol: SubstreamProtocol<Upgrade, UserData>,
    ) -> Self {
        let timeout = *protocol.timeout();
        let (upgrade, open_info) = protocol.into_upgrade();

        Self {
            user_data: Some(open_info),
            timeout: Delay::new(timeout),
            upgrade: upgrade::apply_inbound(substream, SendWrapper(upgrade)),
        }
    }
}

impl<UserData, Upgrade> Unpin for SubstreamUpgrade<UserData, Upgrade> {}

impl<UserData, Upgrade, UpgradeOutput, TUpgradeError> Future for SubstreamUpgrade<UserData, Upgrade>
where
    Upgrade: Future<Output = Result<UpgradeOutput, UpgradeError<TUpgradeError>>> + Unpin,
{
    type Output = (
        UserData,
        Result<UpgradeOutput, ConnectionHandlerUpgrErr<TUpgradeError>>,
    );

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.timeout.poll_unpin(cx) {
            Poll::Ready(()) => {
                return Poll::Ready((
                    self.user_data
                        .take()
                        .expect("Future not to be polled again once ready."),
                    Err(ConnectionHandlerUpgrErr::Timeout),
                ))
            }

            Poll::Pending => {}
        }

        match self.upgrade.poll_unpin(cx) {
            Poll::Ready(Ok(upgrade)) => Poll::Ready((
                self.user_data
                    .take()
                    .expect("Future not to be polled again once ready."),
                Ok(upgrade),
            )),
            Poll::Ready(Err(err)) => Poll::Ready((
                self.user_data
                    .take()
                    .expect("Future not to be polled again once ready."),
                Err(ConnectionHandlerUpgrErr::Upgrade(err)),
            )),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The options for a planned connection & handler shutdown.
///
/// A shutdown is planned anew based on the the return value of
/// [`ConnectionHandler::connection_keep_alive`] of the underlying handler
/// after every invocation of [`ConnectionHandler::poll`].
///
/// A planned shutdown is always postponed for as long as there are ingoing
/// or outgoing substreams being negotiated, i.e. it is a graceful, "idle"
/// shutdown.
#[derive(Debug)]
enum Shutdown {
    /// No shutdown is planned.
    None,
    /// A shut down is planned as soon as possible.
    Asap,
    /// A shut down is planned for when a `Delay` has elapsed.
    Later(Delay, Instant),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::DummyConnectionHandler;
    use futures::AsyncRead;
    use futures::AsyncWrite;
    use libp2p_core::StreamMuxer;
    use quickcheck::*;
    use std::sync::{Arc, Weak};
    use void::Void;

    #[test]
    fn max_negotiating_inbound_streams() {
        fn prop(max_negotiating_inbound_streams: u8) {
            let max_negotiating_inbound_streams: usize = max_negotiating_inbound_streams.into();

            let alive_substream_counter = Arc::new(());

            let mut connection = Connection::new(
                PeerId::random(),
                ConnectedPoint::Listener {
                    local_addr: Multiaddr::empty(),
                    send_back_addr: Multiaddr::empty(),
                },
                StreamMuxerBox::new(DummyStreamMuxer {
                    counter: alive_substream_counter.clone(),
                }),
                DummyConnectionHandler {
                    keep_alive: KeepAlive::Yes,
                },
                None,
                max_negotiating_inbound_streams,
            );

            let result = Pin::new(&mut connection)
                .poll(&mut Context::from_waker(futures::task::noop_waker_ref()));

            assert!(result.is_pending());
            assert_eq!(
                Arc::weak_count(&alive_substream_counter),
                max_negotiating_inbound_streams,
                "Expect no more than the maximum number of allowed streams"
            );
        }

        QuickCheck::new().quickcheck(prop as fn(_));
    }

    struct DummyStreamMuxer {
        counter: Arc<()>,
    }

    impl StreamMuxer for DummyStreamMuxer {
        type Substream = PendingSubstream;
        type Error = Void;

        fn poll_inbound(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Result<Self::Substream, Self::Error>> {
            Poll::Ready(Ok(PendingSubstream(Arc::downgrade(&self.counter))))
        }

        fn poll_outbound(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Result<Self::Substream, Self::Error>> {
            Poll::Pending
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
            Poll::Pending
        }
    }

    struct PendingSubstream(Weak<()>);

    impl AsyncRead for PendingSubstream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for PendingSubstream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Pending
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }

        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }
}
