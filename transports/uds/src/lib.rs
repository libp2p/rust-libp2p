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
//! Uses [the *tokio* library](https://tokio.rs).
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

#![cfg(all(unix, any(target_os = "emscripten", target_os = "unknown")))]

extern crate futures;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate tokio_uds;

#[cfg(test)]
extern crate tempfile;
#[cfg(test)]
extern crate tokio_io;
#[cfg(test)]
extern crate tokio;

use futures::{future::{self, FutureResult}, prelude::*, try_ready};
use futures::stream::Stream;
use multiaddr::{Protocol, Multiaddr};
use std::io::Error as IoError;
use std::path::PathBuf;
use libp2p_core::Transport;
use tokio_uds::{UnixListener, UnixStream};

/// Represents the configuration for a Unix domain sockets transport capability for libp2p.
///
/// The Unixs sockets created by libp2p will need to be progressed by running the futures and
/// streams obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone)]
pub struct UdsConfig {
}

impl UdsConfig {
    /// Creates a new configuration object for TCP/IP.
    #[inline]
    pub fn new() -> UdsConfig {
        UdsConfig {}
    }
}

impl Transport for UdsConfig {
    type Output = UnixStream;
    type Listener = ListenerStream<tokio_uds::Incoming>;
    type ListenerUpgrade = FutureResult<Self::Output, IoError>;
    type Dial = tokio_uds::ConnectFuture;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            let listener = UnixListener::bind(&path);
            // We need to build the `Multiaddr` to return from this function. If an error happened,
            // just return the original multiaddr.
            match listener {
                Ok(listener) => {
                    debug!("Now listening on {}", addr);
                    let future = ListenerStream {
                        stream: listener.incoming(),
                        addr: addr.clone()
                    };
                    Ok((future, addr))
                }
                Err(_) => return Err((self, addr)),
            }
        } else {
            Err((self, addr))
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            debug!("Dialing {}", addr);
            Ok(UnixStream::connect(&path))
        } else {
            Err((self, addr))
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        if server == observed {
            Some(observed.clone())
        } else {
            None
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
    addr: Multiaddr
}

impl<T> Stream for ListenerStream<T>
where
    T: Stream
{
    type Item = (FutureResult<T::Item, T::Error>, Multiaddr);
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match try_ready!(self.stream.poll()) {
            Some(item) => {
                debug!("incoming connection on {}", self.addr);
                Ok(Async::Ready(Some((future::ok(item), self.addr.clone()))))
            }
            None => Ok(Async::Ready(None))
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::runtime::current_thread::Runtime;
    use super::{multiaddr_to_path, UdsConfig};
    use futures::stream::Stream;
    use futures::Future;
    use multiaddr::{Protocol, Multiaddr};
    use std::{self, borrow::Cow, path::Path};
    use libp2p_core::Transport;
    use tempfile;
    use tokio_io;

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
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let socket = temp_dir.path().join("socket");
        let addr = Multiaddr::from(Protocol::Unix(Cow::Owned(socket.to_string_lossy().into_owned())));
        let addr2 = addr.clone();

        std::thread::spawn(move || {
            let tcp = UdsConfig::new();

            let mut rt = Runtime::new().unwrap();
            let handle = rt.handle();
            let listener = tcp.listen_on(addr2).unwrap().0.for_each(|(sock, _)| {
                sock.and_then(|sock| {
                    // Define what to do with the socket that just connected to us
                    // Which in this case is read 3 bytes
                    let handle_conn = tokio_io::io::read_exact(sock, [0; 3])
                        .map(|(_, buf)| assert_eq!(buf, [1, 2, 3]))
                        .map_err(|err| panic!("IO error {:?}", err));

                    // Spawn the future as a concurrent task
                    handle.spawn(handle_conn).unwrap();
                    Ok(())
                })
            });

            rt.block_on(listener).unwrap();
            rt.run().unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let tcp = UdsConfig::new();
        // Obtain a future socket through dialing
        let socket = tcp.dial(addr.clone()).unwrap();
        // Define what to do with the socket once it's obtained
        let action = socket.then(|sock| -> Result<(), ()> {
            sock.unwrap().write(&[0x1, 0x2, 0x3]).unwrap();
            Ok(())
        });
        // Execute the future in our event loop
        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(action).unwrap();
    }

    #[test]
    #[ignore]       // TODO: for the moment unix addresses fail to parse
    fn larger_addr_denied() {
        let tcp = UdsConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/12345/unix//foo/bar"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(tcp.listen_on(addr).is_err());
    }

    #[test]
    #[ignore]       // TODO: for the moment unix addresses fail to parse
    fn relative_addr_denied() {
        assert!("/ip4/127.0.0.1/tcp/12345/unix/./foo/bar".parse::<Multiaddr>().is_err());
    }
}
