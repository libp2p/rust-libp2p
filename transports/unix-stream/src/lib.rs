// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p [`libp2p_core::Transport`] trait for UNIX-domain stream sockets.
//!
//! # Usage
//!
//! This crate provides a [`async_io::Transport`] and [`tokio::Transport`], depending on
//! the enabled features, which implement the [`libp2p_core::Transport`] trait for use as a
//! transport with `libp2p-core` or `libp2p-swarm`.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg(unix)]

mod provider;

#[cfg(any(target_os = "android", target_os = "linux"))]
use std::os::linux::net::SocketAddrExt;
use std::{
    borrow::Cow,
    collections::VecDeque,
    ffi::OsStr,
    io,
    os::unix::{
        ffi::OsStrExt,
        net::{SocketAddr, UnixListener, UnixStream},
    },
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{future::Ready, prelude::*, stream::SelectAll};
use futures_timer::Delay;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
};
use percent_encoding_rfc3986::{percent_decode_str, percent_encode};
#[cfg(feature = "async-io")]
pub use provider::async_io;
#[cfg(feature = "tokio")]
pub use provider::tokio;
use provider::{Incoming, Provider};

/// An abstract [`libp2p_core::Transport`] implementation.
///
/// You shouldn't need to use this type directly. Use one of the following instead:
///
/// - [`tokio::Transport`]
/// - [`async_io::Transport`]
pub struct Transport<T>
where
    T: Provider + Send,
{
    /// All the active listeners.
    /// The [`ListenStream`] struct contains a stream that we want to be pinned. Since the
    /// `VecDeque` can be resized, the only way is to use a `Pin<Box<>>`.
    listeners: SelectAll<ListenStream<T>>,
    /// Pending transport events to return from [`libp2p_core::Transport::poll`].
    pending_events:
        VecDeque<TransportEvent<<Self as libp2p_core::Transport>::ListenerUpgrade, io::Error>>,
}

impl<T> Transport<T>
where
    T: Provider + Send,
{
    /// Create a new instance of [`Transport`].
    ///
    /// It is best to call this function through one of the type-aliases of this type:
    ///
    /// - [`tokio::Transport::new`]
    /// - [`async_io::Transport::new`]
    pub fn new() -> Self {
        Default::default()
    }

    fn do_listen(
        &mut self,
        id: ListenerId,
        socket_addr: &SocketAddr,
    ) -> io::Result<ListenStream<T>> {
        let listener = UnixListener::bind_addr(socket_addr)?;
        listener.set_nonblocking(true)?;

        let listen_addr = socketaddr_to_multiaddr(socket_addr).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                format!("{:?}", socket_addr),
            )
        })?;
        self.pending_events.push_back(TransportEvent::NewAddress {
            listener_id: id,
            listen_addr,
        });
        ListenStream::<T>::new(id, listener)
    }
}

impl<T> Default for Transport<T>
where
    T: Provider + Send,
{
    /// Creates a [`Transport`] with reasonable defaults.
    ///
    /// This transport will have port-reuse disabled.
    fn default() -> Self {
        Transport {
            listeners: SelectAll::new(),
            pending_events: VecDeque::new(),
        }
    }
}

impl<T> libp2p_core::Transport for Transport<T>
where
    T: Provider + Send + 'static,
    T::Listener: Unpin,
    T::Stream: Unpin,
{
    type Output = T::Stream;
    type Error = io::Error;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        let socket_addr = multiaddr_to_socketaddr(addr.clone())
            .map_err(|_| TransportError::MultiaddrNotSupported(addr))?;
        tracing::debug!("listening on {:?}", socket_addr);
        let listener = self
            .do_listen(id, &socket_addr)
            .map_err(TransportError::Other)?;
        self.listeners.push(listener);
        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
            listener.close(Ok(()));
            true
        } else {
            false
        }
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        _: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let Ok(socket_addr) = multiaddr_to_socketaddr(addr.clone()) else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        tracing::debug!(address=?socket_addr, "dialing address");

        // [`Transport::dial`] should do no work unless the returned [`Future`] is polled. Thus
        // do the `connect` call within the [`Future`].
        Ok(async move {
            // We can't set non-blocking mode before connect(2) in Rust; this always completes
            // instantly
            let socket = UnixStream::connect_addr(&socket_addr)?;
            socket.set_nonblocking(true)?;

            T::new_stream(socket.into()).await
        }
        .boxed())
    }

    /// Poll all listeners.
    #[tracing::instrument(level = "trace", name = "Transport::poll", skip(self, cx))]
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        // Return pending events from closed listeners.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }

        match self.listeners.poll_next_unpin(cx) {
            Poll::Ready(Some(transport_event)) => Poll::Ready(transport_event),
            _ => Poll::Pending,
        }
    }
}

/// A stream of incoming connections on one or more interfaces.
struct ListenStream<T>
where
    T: Provider,
{
    /// The ID of this listener.
    listener_id: ListenerId,
    /// The async listening socket for incoming connections.
    listener: T::Listener,
    /// How long to sleep after a (non-fatal) error while trying
    /// to accept a new connection.
    sleep_on_error: Duration,
    /// The current pause, if any.
    pause: Option<Delay>,
    /// Pending event to reported.
    pending_event: Option<<Self as Stream>::Item>,
    /// The listener can be manually closed with
    /// [`Transport::remove_listener`](libp2p_core::Transport::remove_listener).
    is_closed: bool,
    /// The stream must be awaken after it has been closed to deliver the last event.
    close_listener_waker: Option<Waker>,
}

impl<T> ListenStream<T>
where
    T: Provider,
{
    /// Constructs a [`ListenStream`] for incoming connections around
    /// the given [`UnixListener`].
    fn new(listener_id: ListenerId, listener: UnixListener) -> io::Result<Self> {
        let listener = T::new_listener(listener)?;

        Ok(ListenStream {
            listener,
            listener_id,
            pause: None,
            sleep_on_error: Duration::from_millis(100),
            pending_event: None,
            is_closed: false,
            close_listener_waker: None,
        })
    }

    /// Close the listener.
    ///
    /// This will create a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream once the event has been reported.
    fn close(&mut self, reason: Result<(), io::Error>) {
        if self.is_closed {
            return;
        }
        self.pending_event = Some(TransportEvent::ListenerClosed {
            listener_id: self.listener_id,
            reason,
        });
        self.is_closed = true;

        // Wake the stream to deliver the last event.
        if let Some(waker) = self.close_listener_waker.take() {
            waker.wake();
        }
    }
}

impl<T> Stream for ListenStream<T>
where
    T: Provider,
    T::Listener: Unpin,
    T::Stream: Unpin,
{
    type Item = TransportEvent<Ready<Result<T::Stream, io::Error>>, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(mut pause) = self.pause.take() {
            match pause.poll_unpin(cx) {
                Poll::Ready(_) => {}
                Poll::Pending => {
                    self.pause = Some(pause);
                    return Poll::Pending;
                }
            }
        }

        if let Some(event) = self.pending_event.take() {
            return Poll::Ready(Some(event));
        }

        if self.is_closed {
            // Terminate the stream if the listener closed
            // and all remaining events have been reported.
            return Poll::Ready(None);
        }

        // Take the pending connection from the backlog.
        match T::poll_accept(&mut self.listener, cx) {
            Poll::Ready(Ok(Incoming {
                local_addr,
                remote_addr,
                stream,
            })) => {
                let local_addr = socketaddr_to_multiaddr(&local_addr)
                    .expect("can't be anonymous address on accept");
                let remote_addr =
                    socketaddr_to_multiaddr(&remote_addr).unwrap_or(Multiaddr::empty()); // this will, more often than not, be anonymous

                tracing::debug!(
                    remote_address=?remote_addr,
                    local_address=?local_addr,
                    "Incoming connection from remote at local"
                );

                return Poll::Ready(Some(TransportEvent::Incoming {
                    listener_id: self.listener_id,
                    upgrade: future::ok(stream),
                    local_addr,
                    send_back_addr: remote_addr,
                }));
            }
            Poll::Ready(Err(error)) => {
                // These errors are non-fatal for the listener stream.
                self.pause = Some(Delay::new(self.sleep_on_error));
                return Poll::Ready(Some(TransportEvent::ListenerError {
                    listener_id: self.listener_id,
                    error,
                }));
            }
            Poll::Pending => {}
        }

        self.close_listener_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// The valid characters in a /unix/ are ALPHA DIGIT -._~ !$&'()*+,;= :@
const MULTI_UNIX: percent_encoding_rfc3986::AsciiSet = percent_encoding_rfc3986::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    .remove(b'=')
    .remove(b':')
    .remove(b'@');

/// Extracts a `SocketAddr` from a given `Multiaddr`.
///
/// Fails if the given `Multiaddr` does not begin with a `/unix`
/// or if that `/unix` lists an invalid path or unusable address
/// (i.e. is abstract on non-linux).
fn multiaddr_to_socketaddr(mut addr: Multiaddr) -> Result<SocketAddr, ()> {
    // "Pop" the UNIX path from the end of the address,
    // ignoring a `/p2p/...` suffix as well as any prefix of possibly
    // outer protocols, if present.
    while let Some(proto) = addr.pop() {
        match proto {
            Protocol::Unix(path) => {
                let bytes: Cow<_> = percent_decode_str(&path).map_err(|_| ())?.into();
                if bytes.starts_with(b"\0") {
                    #[cfg(any(target_os = "android", target_os = "linux"))]
                    return SocketAddr::from_abstract_name(&bytes[1..]).map_err(|_| ());
                    #[allow(unreachable_code)]
                    return Err(());
                } else {
                    return SocketAddr::from_pathname(OsStr::from_bytes(&bytes)).map_err(|_| ());
                }
            }
            Protocol::P2p(_) => {}
            _ => return Err(()),
        }
    }
    Err(())
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn socketaddr_to_multiaddr(sock: &SocketAddr) -> Option<Multiaddr> {
    let mut path: Option<Cow<_>> = sock
        .as_pathname()
        .map(|p| percent_encode(p.as_os_str().as_bytes(), &MULTI_UNIX).into());

    if path.is_none() {
        if let Some(name) = sock.as_abstract_name() {
            path = Some(format!("%00{}", percent_encode(name, &MULTI_UNIX)).into());
        }
    }

    Some(Multiaddr::empty().with(Protocol::Unix(path?)))
}

#[cfg(test)]
mod tests {
    use futures::channel::mpsc;
    use libp2p_core::{transport::PortUse, Endpoint, Transport as _};

    use super::*;

    #[test]
    fn multiaddr_to_unix_conversion() {
        // SocketAddr is !Display, but its Debug implementations contain all the interesting data
        fn d(sa: SocketAddr) -> String {
            format!("{:?}", sa)
        }

        assert!(
            multiaddr_to_socketaddr("/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_socketaddr(
                "/unix/%2Frun%2Fsystemd%2Fnotify"
                    .parse::<Multiaddr>()
                    .unwrap()
            )
            .map(d),
            Ok(SocketAddr::from_pathname("/run/systemd/notify").unwrap()).map(d)
        );
        assert_eq!(
            multiaddr_to_socketaddr("/unix/notify".parse::<Multiaddr>().unwrap()).map(d),
            Ok(SocketAddr::from_pathname("notify").unwrap()).map(d)
        );
        #[cfg(any(target_os = "android", target_os = "linux"))]
        assert_eq!(
            multiaddr_to_socketaddr(
                "/unix/%009e48095dfe839881%2Fbus%2Fsystemd-timesyn%2Fbus-api-timesync"
                    .parse::<Multiaddr>()
                    .unwrap()
            )
            .map(d),
            Ok(SocketAddr::from_abstract_name(
                "9e48095dfe839881/bus/systemd-timesyn/bus-api-timesync"
            )
            .unwrap())
            .map(d)
        );
        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        assert!(multiaddr_to_socketaddr(
            "%009e48095dfe839881%2Fbus%2Fsystemd-timesyn%2Fbus-api-timesync"
                .parse::<Multiaddr>()
                .unwrap()
        )
        .is_err());
    }

    // https://github.com/multiformats/multiaddr/pull/174#issuecomment-2964331099
    #[test]
    fn socketaddr_to_multiaddr_conversion() {
        use libp2p_core::multiaddr::multiaddr;
        assert_eq!(
            socketaddr_to_multiaddr(&SocketAddr::from_pathname("/run/systemd/notify").unwrap()),
            Some(multiaddr!(Unix(Cow::from("%2Frun%2Fsystemd%2Fnotify")))),
        );
        assert_eq!(
            socketaddr_to_multiaddr(&SocketAddr::from_pathname("notify").unwrap()),
            Some(multiaddr!(Unix(Cow::from("notify")))),
        );
        #[cfg(any(target_os = "android", target_os = "linux"))]
        assert_eq!(
            socketaddr_to_multiaddr(
                &SocketAddr::from_abstract_name(
                    "9e48095dfe839881/bus/systemd-timesyn/bus-api-timesync"
                )
                .unwrap()
            ),
            Some(multiaddr!(Unix(Cow::from(
                "%009e48095dfe839881%2Fbus%2Fsystemd-timesyn%2Fbus-api-timesync"
            )))),
        );
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        async fn listener<T: Provider>(addr: Multiaddr, mut ready_tx: mpsc::Sender<Multiaddr>) {
            let mut tcp = Transport::<T>::default().boxed();
            tcp.listen_on(ListenerId::next(), addr).unwrap();
            loop {
                match tcp.select_next_some().await {
                    TransportEvent::NewAddress { listen_addr, .. } => {
                        ready_tx.send(listen_addr).await.unwrap();
                    }
                    TransportEvent::Incoming { upgrade, .. } => {
                        let mut upgrade = upgrade.await.unwrap();
                        let mut buf = [0u8; 3];
                        upgrade.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [1, 2, 3]);
                        upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        return;
                    }
                    e => panic!("Unexpected transport event: {e:?}"),
                }
            }
        }

        async fn dialer<T: Provider>(mut ready_rx: mpsc::Receiver<Multiaddr>) {
            let addr = ready_rx.next().await.unwrap();
            let mut tcp = Transport::<T>::default();

            // Obtain a future socket through dialing
            let mut socket = tcp
                .dial(
                    addr.clone(),
                    DialOpts {
                        role: Endpoint::Dialer,
                        port_use: PortUse::Reuse,
                    },
                )
                .unwrap()
                .await
                .unwrap();
            socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

            let mut buf = [0u8; 3];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [4, 5, 6]);
        }

        fn test(addr: Multiaddr, rm: Option<&str>) {
            rm.and_then(|r| std::fs::remove_file(r).ok());
            #[cfg(feature = "async-io")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let listener = listener::<async_io::UnixStrm>(addr.clone(), ready_tx);
                let dialer = dialer::<async_io::UnixStrm>(ready_rx);
                let listener = async_std::task::spawn(listener);
                async_std::task::block_on(dialer);
                async_std::task::block_on(listener);
            }

            rm.and_then(|r| std::fs::remove_file(r).ok());
            #[cfg(feature = "tokio")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let listener = listener::<tokio::UnixStrm>(addr, ready_tx);
                let dialer = dialer::<tokio::UnixStrm>(ready_rx);
                let rt = ::tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();
                let tasks = ::tokio::task::LocalSet::new();
                let listener = tasks.spawn_local(listener);
                tasks.block_on(&rt, dialer);
                tasks.block_on(&rt, listener).unwrap();
            }
            rm.and_then(|r| std::fs::remove_file(r).ok());
        }
        test(
            "/unix/communicating_between_dialer_and_listener"
                .parse()
                .unwrap(),
            Some("communicating_between_dialer_and_listener"),
        );
        #[cfg(any(target_os = "android", target_os = "linux"))]
        test(
            "/unix/%00communicating_between_dialer_and_listener"
                .parse()
                .unwrap(),
            None,
        );
    }

    #[test]
    fn listen_invalid_addr() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let mut tcp = async_io::Transport::default();
                assert!(tcp.listen_on(ListenerId::next(), addr.clone()).is_err());
            }

            #[cfg(feature = "tokio")]
            {
                let mut tcp = tokio::Transport::default();
                assert!(tcp.listen_on(ListenerId::next(), addr).is_err());
            }
        }

        test("/unix/".parse().unwrap()); // "empty" (len=0) address is the only really invalid one
    }

    #[test]
    fn test_remove_listener() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        async fn cycle_listeners<T: Provider>() -> bool {
            let mut tcp = Transport::<T>::default().boxed();
            let listener_id = ListenerId::next();
            tcp.listen_on(listener_id, "/unix/cycle_listeners".parse().unwrap())
                .unwrap();
            tcp.remove_listener(listener_id)
        }

        let _ = std::fs::remove_file("cycle_listeners");
        #[cfg(feature = "async-io")]
        {
            assert!(async_std::task::block_on(cycle_listeners::<
                async_io::UnixStrm,
            >()));
        }

        let _ = std::fs::remove_file("cycle_listeners");
        #[cfg(feature = "tokio")]
        {
            let rt = ::tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
                .unwrap();
            assert!(rt.block_on(cycle_listeners::<tokio::UnixStrm>()));
        }
        let _ = std::fs::remove_file("cycle_listeners");
    }
}
