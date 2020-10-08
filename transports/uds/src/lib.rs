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

//! Implementation of the libp2p `Transport` trait for Unix domain sockets.
//!
//! # Platform support
//!
//! This transport only works on Unix platforms.
//!
//! # Usage
//!
//! The `UdsConfig` transport supports multiaddresses of the form `/unix//tmp/foo`.
//!
//! The `UdsConfig` structs implements the `Transport` trait of the `core` library. See the
//! documentation of `core` and of libp2p in general to learn how to use the `Transport` trait.

#![cfg(all(unix, not(target_os = "emscripten")))]
#![cfg_attr(docsrs, doc(cfg(all(unix, not(target_os = "emscripten")))))]

use async_io::Async;
use futures::{prelude::*, future::{BoxFuture, Ready}};
use libp2p_core::{
    Dialer, Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::debug;
use std::{io, path::PathBuf, pin::Pin, task::{Context, Poll}};
use std::os::unix::net::{UnixListener, UnixStream};

/// Represents the configuration for a Unix domain sockets transport capability for libp2p.
#[derive(Debug, Clone)]
pub struct UdsConfig {
}

impl UdsConfig {
    /// Creates a new configuration object for Unix domain sockets.
    pub fn new() -> UdsConfig {
        UdsConfig {}
    }
}

#[derive(Debug, Clone)]
pub struct UdsDialer {}

impl Dialer for UdsDialer {
    type Output = Async<UnixStream>;
    type Error = io::Error;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            debug!("Dialing {}", addr);
            Ok(Async::<UnixStream>::connect(path).boxed())
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
    }
}

impl Transport for UdsConfig {
    type Output = Async<UnixStream>;
    type Error = io::Error;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dialer = UdsDialer;
    type Listener = UdsListenStream;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    fn dialer(&self) -> Self::Dialer {
        UdsDialer {}
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            let listener = Async::<UnixListener>::bind(path).map_err(TransportError::Other)?;
            Ok((UdsListenStream { new_address: true, listener, addr }, UdsDialer {}))
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
    }
}

pub struct UdsListenStream {
    new_address: bool,
    listener: Async<UnixListener>,
    addr: Multiaddr,
}

impl Stream for UdsListenStream {
    type Item = Result<ListenerEvent<Ready<Result<Async<UnixStream>, std::io::Error>>, std::io::Error>, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if self.new_address {
            self.new_address = false;
            return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(self.addr.clone()))));
        }

        let (stream, _) = loop {
            match self.listener.accept().now_or_never() {
                Some(Ok(res)) => break res,
                Some(Err(e)) => {
                    debug!("error accepting incoming connection: {}", e);
                    return Poll::Ready(Some(Ok(ListenerEvent::Error(e))));
                }
                None => {
                    match self.listener.poll_readable(cx) {
                        Poll::Ready(Ok(())) => continue,
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                    }
                }
            }
        };

        debug!("incoming connection on {}", self.addr);
        let event = ListenerEvent::Upgrade {
            upgrade: future::ok(stream),
            local_addr: self.addr.clone(),
            remote_addr: self.addr.clone()
        };
        Poll::Ready(Some(Ok(event)))
    }
}

/// Turns a `Multiaddr` containing a single `Unix` component into a path.
///
/// Also returns an error if the path is not absolute, as we don't want to dial/listen on relative
/// paths.
// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_path(addr: &Multiaddr) -> Result<PathBuf, ()> {
    let mut iter = addr.iter();
    let path = iter.next();

    if iter.next().is_some() {
        return Err(());
    }

    let out: PathBuf = match path {
        Some(Protocol::Unix(ref path)) => path.as_ref().into(),
        _ => return Err(())
    };

    if !out.is_absolute() {
        return Err(());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{multiaddr_to_path, UdsConfig};
    use futures::{channel::oneshot, prelude::*};
    use std::{self, borrow::Cow, path::Path};
    use libp2p_core::{Dialer, Transport, multiaddr::{Protocol, Multiaddr}};
    use tempfile;

    #[test]
    fn multiaddr_to_path_conversion() {
        assert!(
            multiaddr_to_path(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_path(&Multiaddr::from(Protocol::Unix("/tmp/foo".into()))),
            Ok(Path::new("/tmp/foo").to_owned())
        );
        assert_eq!(
            multiaddr_to_path(&Multiaddr::from(Protocol::Unix("/home/bar/baz".into()))),
            Ok(Path::new("/home/bar/baz").to_owned())
        );
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket = temp_dir.path().join("socket");
        let addr = Multiaddr::from(Protocol::Unix(Cow::Owned(socket.to_string_lossy().into_owned())));

        let (tx, rx) = oneshot::channel();

        async_std::task::spawn(async move {
            let mut listener = UdsConfig::new().listen_on(addr).unwrap().0;

            let listen_addr = listener.try_next().await.unwrap()
                .expect("some event")
                .into_new_address()
                .expect("listen address");

            tx.send(listen_addr).unwrap();

            let (sock, _addr) = listener.try_filter_map(|e| future::ok(e.into_upgrade()))
                .try_next()
                .await
                .unwrap()
                .expect("some event");

            let mut sock = sock.await.unwrap();
            let mut buf = [0u8; 3];
            sock.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [1, 2, 3]);
        });

        async_std::task::block_on(async move {
            let uds = UdsConfig::new();
            let addr = rx.await.unwrap();
            let mut socket = uds.dialer().dial(addr).unwrap().await.unwrap();
            socket.write(&[1, 2, 3]).await.unwrap();
        });
    }

    #[test]
    #[ignore]       // TODO: for the moment unix addresses fail to parse
    fn larger_addr_denied() {
        let uds = UdsConfig::new();

        let addr = "/unix//foo/bar"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(uds.listen_on(addr).is_err());
    }

    #[test]
    #[ignore]       // TODO: for the moment unix addresses fail to parse
    fn relative_addr_denied() {
        assert!("/unix/./foo/bar".parse::<Multiaddr>().is_err());
    }
}
