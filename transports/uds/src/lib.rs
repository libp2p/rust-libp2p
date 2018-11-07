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
//! let uds = UdsConfig::default();
//! # }
//! ```
//!
//! The `UdsConfig` structs implements the `Transport` trait of the `core` library. See the
//! documentation of `core` and of libp2p in general to learn how to use the `Transport` trait.

#![cfg(all(unix, not(target_os = "emscripten")))]

extern crate futures;
extern crate libp2p_core;
extern crate log;
extern crate multiaddr;
extern crate tokio_uds;

#[cfg(test)]
extern crate tempfile;
#[cfg(test)]
extern crate tokio_io;
#[cfg(test)]
extern crate tokio;

use futures::future::{self, Future, FutureResult};
use futures::stream::Stream;
use log::debug;
use multiaddr::{Protocol, Multiaddr};
use std::io;
use std::path::PathBuf;
use libp2p_core::transport::{Dialer, Listener};
use tokio_uds::{UnixListener, UnixStream};

/// Represents the configuration for a Unix domain sockets transport capability for libp2p.
///
/// The Unixs sockets created by libp2p will need to be progressed by running the futures and
/// streams obtained by libp2p through the tokio reactor.
#[derive(Clone, Debug, Default)]
pub struct UdsConfig { }

#[derive(Clone, Debug, Default)]
pub struct UdsListener {
    config: UdsConfig
}

impl UdsListener {
    pub fn new(config: UdsConfig) -> Self {
        UdsListener { config }
    }
}

impl Listener for UdsListener {
    type Output = UnixStream;
    type Error = io::Error;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = io::Error> + Send + Sync>;
    type Upgrade = FutureResult<Self::Output, Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            let listener = UnixListener::bind(&path);
            // We need to build the `Multiaddr` to return from this function. If an error happened,
            // just return the original multiaddr.
            match listener {
                Ok(_) => {},
                Err(_) => return Err((self, addr)),
            };

            debug!("Now listening on {}", addr);
            let new_addr = addr.clone();

            let future = future::result(listener)
                .map(move |listener| {
                    // Pull out a stream of sockets for incoming connections
                    listener.incoming().map(move |sock| {
                        debug!("Incoming connection on {}", addr);
                        (future::ok(sock), addr.clone())
                    })
                })
                .flatten_stream();
            Ok((Box::new(future), new_addr))
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

#[derive(Clone, Debug, Default)]
pub struct UdsDialer {
    config: UdsConfig
}

impl UdsDialer {
    pub fn new(config: UdsConfig) -> Self {
        UdsDialer { config }
    }
}

impl Dialer for UdsDialer {
    type Output = UnixStream;
    type Error = io::Error;
    type Outbound = Box<Future<Item = UnixStream, Error = Self::Error> + Send + Sync>;  // TODO: name this type

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        if let Ok(path) = multiaddr_to_path(&addr) {
            debug!("Dialing {}", addr);
            let fut = UnixStream::connect(&path);
            Ok(Box::new(fut) as Box<_>)
        } else {
            Err((self, addr))
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

#[cfg(test)]
mod tests {
    use tokio::runtime::current_thread::Runtime;
    use super::{multiaddr_to_path, UdsDialer, UdsListener};
    use futures::stream::Stream;
    use futures::Future;
    use multiaddr::{Protocol, Multiaddr};
    use std::{self, borrow::Cow, path::Path};
    use libp2p_core::transport::{Dialer, Listener};
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
            let listener = UdsListener::default();
            let mut rt = Runtime::new().unwrap();
            let handle = rt.handle();
            let inbound = listener.listen_on(addr2).unwrap().0.for_each(|(sock, _)| {
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

            rt.block_on(inbound).unwrap();
            rt.run().unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        let dialer = UdsDialer::default();
        // Obtain a future socket through dialing
        let socket = dialer.dial(addr.clone()).unwrap();
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
        let listener = UdsListener::default();

        let addr = "/ip4/127.0.0.1/tcp/12345/unix//foo/bar"
            .parse::<Multiaddr>()
            .unwrap();

        assert!(listener.listen_on(addr).is_err());
    }

    #[test]
    #[ignore]       // TODO: for the moment unix addresses fail to parse
    fn relative_addr_denied() {
        assert!("/ip4/127.0.0.1/tcp/12345/unix/./foo/bar".parse::<Multiaddr>().is_err());
    }
}
