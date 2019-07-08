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
//! Example:
//!
//! ```
//! extern crate libp2p_uds;
//! use libp2p_uds::UdsConfig;
//!
//! # fn main() {
//! let uds = UdsConfig::new();
//! # }
//! ```
//!
//! The `UdsConfig` structs implements the `Transport` trait of the `core` library. See the
//! documentation of `core` and of libp2p in general to learn how to use the `Transport` trait.

#![cfg(all(unix, not(any(target_os = "emscripten", target_os = "unknown"))))]

use futures::{prelude::*, ready, future::Ready};
use futures::stream::Stream;
use log::debug;
use romio::uds::{UnixListener, UnixStream};
use std::{io, path::PathBuf, pin::Pin, task::Context, task::Poll};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};

/// Represents the configuration for a Unix domain sockets transport capability for libp2p.
///
/// The Unix sockets created by libp2p will need to be progressed by running the futures and
/// streams obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone)]
pub struct UdsConfig {
}

impl UdsConfig {
    /// Creates a new configuration object for Unix domain sockets.
    #[inline]
    pub fn new() -> UdsConfig {
        UdsConfig {}
    }
}

impl Transport for UdsConfig {
    type Output = UnixStream;
    type Error = io::Error;
    type Listener = ListenerStream<romio::uds::Incoming>;
    type ListenerUpgrade = Ready<Result<Self::Output, io::Error>>;
    type Dial = romio::uds::ConnectFuture;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            let listener = UnixListener::bind(&path);
            // We need to build the `Multiaddr` to return from this function. If an error happened,
            // just return the original multiaddr.
            match listener {
                Ok(listener) => {
                    debug!("Now listening on {}", addr);
                    let future = ListenerStream {
                        stream: listener.incoming(),
                        addr: addr.clone(),
                        tell_new_addr: true
                    };
                    Ok(future)
                }
                Err(_) => return Err(TransportError::MultiaddrNotSupported(addr)),
            }
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            debug!("Dialing {}", addr);
            Ok(UnixStream::connect(&path))
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
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

pub struct ListenerStream<T> {
    stream: T,
    addr: Multiaddr,
    tell_new_addr: bool
}

impl<T> Stream for ListenerStream<T>
where
    T: TryStream + Unpin
{
    type Item = Result<ListenerEvent<future::Ready<Result<T::Ok, T::Error>>>, T::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if self.tell_new_addr {
            self.tell_new_addr = false;
            return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(self.addr.clone()))))
        }

        match ready!(TryStream::try_poll_next(Pin::new(&mut self.stream), cx)) {
            Some(item) => {
                debug!("incoming connection on {}", self.addr);
                Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                    upgrade: future::ready(item),
                    local_addr: self.addr.clone(),
                    remote_addr: self.addr.clone()
                })))
            }
            None => Poll::Ready(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{multiaddr_to_path, UdsConfig};
    use futures::prelude::*;
    use std::{self, borrow::Cow, path::Path};
    use libp2p_core::{
        Transport,
        multiaddr::{Protocol, Multiaddr}
    };
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
        let addr2 = addr.clone();

        async_std::task::spawn(
            UdsConfig::new().listen_on(addr2).unwrap()
                .try_filter_map(|ev| future::ok(ev.into_upgrade()))
                .try_for_each(|(sock, _)| {
                    async {
                        let mut sock = sock.await.unwrap();
                        let mut buf = [0u8; 3];
                        sock.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [1, 2, 3]);
                        Ok(())
                    }
                })
        );

        futures::executor::block_on(async {
            let uds = UdsConfig::new();
            let mut socket = uds.dial(addr.clone()).unwrap().await.unwrap();
            socket.write(&[0x1, 0x2, 0x3]).await.unwrap();
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
