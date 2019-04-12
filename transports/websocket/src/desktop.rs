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

use futures::{Future, IntoFuture, Sink, Stream};
use libp2p_core::{Transport, transport::{ListenerEvent, TransportError}};
use log::{debug, trace};
use multiaddr::{Protocol, Multiaddr};
use rw_stream_sink::RwStreamSink;
use std::{error, fmt};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};
use websocket::client::builder::ClientBuilder;
use websocket::message::OwnedMessage;
use websocket::server::upgrade::r#async::IntoWs;
use websocket::stream::r#async::Stream as AsyncStream;

/// Represents the configuration for a websocket transport capability for libp2p. Must be put on
/// top of another `Transport`.
///
/// This implementation of `Transport` accepts any address that ends with `/ws` or `/wss`, and will
/// try to pass the underlying multiaddress to the underlying `Transport`.
///
/// Note that the underlying multiaddr is `/dns4/...` or `/dns6/...`, then this library will
/// pass the domain name in the headers of the request. This is important is the listener is behind
/// an HTTP proxy.
///
/// > **Note**: `/wss` is only supported for dialing and not listening.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
    transport: T,
}

impl<T> WsConfig<T> {
    /// Creates a new configuration object for websocket.
    ///
    /// The websockets will run on top of the `Transport` you pass as parameter.
    #[inline]
    pub fn new(inner: T) -> WsConfig<T> {
        WsConfig { transport: inner }
    }
}

impl<T> Transport for WsConfig<T>
where
    // TODO: this 'static is pretty arbitrary and is necessary because of the websocket library
    T: Transport + 'static,
    T::Error: Send,
    T::Dial: Send,
    T::Listener: Send,
    T::ListenerUpgrade: Send,
    // TODO: this Send is pretty arbitrary and is necessary because of the websocket library
    T::Output: AsyncRead + AsyncWrite + Send,
{
    type Output = Box<dyn AsyncStream + Send>;
    type Error = WsError<T::Error>;
    type Listener = Box<dyn Stream<Item = ListenerEvent<Self::ListenerUpgrade>, Error = Self::Error> + Send>;
    type ListenerUpgrade = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;
    type Dial = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, original_addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let mut inner_addr = original_addr.clone();
        match inner_addr.pop() {
            Some(Protocol::Ws) => {}
            _ => return Err(TransportError::MultiaddrNotSupported(original_addr)),
        };

        let inner_listen = self.transport.listen_on(inner_addr)
            .map_err(|err| err.map(WsError::Underlying))?;

        let listen = inner_listen.map_err(WsError::Underlying).map(|event| {
            match event {
                ListenerEvent::NewAddress(mut a) => {
                    a.append(Protocol::Ws);
                    debug!("Listening on {}", a);
                    ListenerEvent::NewAddress(a)
                }
                ListenerEvent::AddressExpired(mut a) => {
                    a.append(Protocol::Ws);
                    ListenerEvent::AddressExpired(a)
                }
                ListenerEvent::Upgrade { upgrade, mut listen_addr, mut remote_addr } => {
                    listen_addr.append(Protocol::Ws);
                    remote_addr.append(Protocol::Ws);

                    // Upgrade the listener to websockets like the websockets library requires us to do.
                    let upgraded = upgrade.map_err(WsError::Underlying).and_then(move |stream| {
                        debug!("Incoming connection");
                        stream.into_ws()
                            .map_err(|e| WsError::WebSocket(Box::new(e.3)))
                            .and_then(|stream| {
                                // Accept the next incoming connection.
                                stream
                                    .accept()
                                    .map_err(|e| WsError::WebSocket(Box::new(e)))
                                    .map(|(client, _http_headers)| {
                                        debug!("Upgraded incoming connection to websockets");

                                        // Plug our own API on top of the `websockets` API.
                                        let framed_data = client
                                            .map_err(|err| IoError::new(IoErrorKind::Other, err))
                                            .sink_map_err(|err| IoError::new(IoErrorKind::Other, err))
                                            .with(|data| Ok(OwnedMessage::Binary(data)))
                                            .and_then(|recv| {
                                                match recv {
                                                    OwnedMessage::Binary(data) => Ok(Some(data)),
                                                    OwnedMessage::Text(data) => Ok(Some(data.into_bytes())),
                                                    OwnedMessage::Close(_) => Ok(None),
                                                    // TODO: handle pings and pongs, which is freaking hard
                                                    //         for now we close the socket when that happens
                                                    _ => Ok(None)
                                                }
                                            })
                                        // TODO: is there a way to merge both lines into one?
                                        .take_while(|v| Ok(v.is_some()))
                                            .map(|v| v.expect("we only take while this is Some"));

                                        let read_write = RwStreamSink::new(framed_data);
                                        Box::new(read_write) as Box<dyn AsyncStream + Send>
                                    })
                            })
                        .map(|s| Box::new(Ok(s).into_future()) as Box<dyn Future<Item = _, Error = _> + Send>)
                            .into_future()
                            .flatten()
                    });

                    ListenerEvent::Upgrade {
                        upgrade: Box::new(upgraded) as Box<dyn Future<Item = _, Error = _> + Send>,
                        listen_addr,
                        remote_addr
                    }
                }
            }
        });

        Ok(Box::new(listen) as Box<_>)
    }

    fn dial(self, original_addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let mut inner_addr = original_addr.clone();
        let is_wss = match inner_addr.pop() {
            Some(Protocol::Ws) => false,
            Some(Protocol::Wss) => true,
            _ => {
                trace!(
                    "Ignoring dial attempt for {} because it is not a websocket multiaddr",
                    original_addr
                );
                return Err(TransportError::MultiaddrNotSupported(original_addr));
            }
        };

        debug!("Dialing {} through inner transport", inner_addr);

        let ws_addr = client_addr_to_ws(&inner_addr, is_wss);

        let inner_dial = self.transport.dial(inner_addr)
            .map_err(|err| err.map(WsError::Underlying))?;

        let dial = inner_dial
            .map_err(WsError::Underlying)
            .into_future()
            .and_then(move |connec| {
                ClientBuilder::new(&ws_addr)
                    .expect("generated ws address is always valid")
                    .async_connect_on(connec)
                    .map_err(|e| WsError::WebSocket(Box::new(e)))
                    .map(|(client, _)| {
                        debug!("Upgraded outgoing connection to websockets");

                        // Plug our own API on top of the API of the websockets library.
                        let framed_data = client
                            .map_err(|err| IoError::new(IoErrorKind::Other, err))
                            .sink_map_err(|err| IoError::new(IoErrorKind::Other, err))
                            .with(|data| Ok(OwnedMessage::Binary(data)))
                            .and_then(|recv| {
                                match recv {
                                    OwnedMessage::Binary(data) => Ok(data),
                                    OwnedMessage::Text(data) => Ok(data.into_bytes()),
                                    // TODO: pings and pongs and close messages need to be
                                    //       answered; and this is really hard; for now we produce
                                    //         an error when that happens
                                    _ => Err(IoError::new(IoErrorKind::Other, "unimplemented")),
                                }
                            });
                        let read_write = RwStreamSink::new(framed_data);
                        Box::new(read_write) as Box<dyn AsyncStream + Send>
                    })
            });

        Ok(Box::new(dial) as Box<_>)
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

/// Error in WebSockets.
#[derive(Debug)]
pub enum WsError<TErr> {
    /// Error in the WebSocket layer.
    WebSocket(Box<dyn error::Error + Send + Sync>),
    /// Error in the transport layer underneath.
    Underlying(TErr),
}

impl<TErr> fmt::Display for WsError<TErr>
where TErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsError::WebSocket(err) => write!(f, "{}", err),
            WsError::Underlying(err) => write!(f, "{}", err),
        }
    }
}

impl<TErr> error::Error for WsError<TErr>
where TErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            WsError::WebSocket(err) => Some(&**err),
            WsError::Underlying(err) => Some(err),
        }
    }
}

fn client_addr_to_ws(client_addr: &Multiaddr, is_wss: bool) -> String {
    let inner = {
        let protocols: Vec<_> = client_addr.iter().collect();

        if protocols.len() != 2 {
            "127.0.0.1".to_owned()
        } else {
            match (&protocols[0], &protocols[1]) {
                (&Protocol::Ip4(ref ip), &Protocol::Tcp(port)) => {
                    format!("{}:{}", ip, port)
                }
                (&Protocol::Ip6(ref ip), &Protocol::Tcp(port)) => {
                    format!("[{}]:{}", ip, port)
                }
                (&Protocol::Dns4(ref ns), &Protocol::Tcp(port)) => {
                    format!("{}:{}", ns, port)
                }
                (&Protocol::Dns6(ref ns), &Protocol::Tcp(port)) => {
                    format!("{}:{}", ns, port)
                }
                _ => "127.0.0.1".to_owned(),
            }
        }
    };

    if is_wss {
        format!("wss://{}", inner)
    } else {
        format!("ws://{}", inner)
    }
}

#[cfg(test)]
mod tests {
    use libp2p_tcp as tcp;
    use tokio::runtime::current_thread::Runtime;
    use futures::{Future, Stream};
    use multiaddr::{Multiaddr, Protocol};
    use libp2p_core::{Transport, transport::ListenerEvent};
    use super::WsConfig;

    #[test]
    fn dialer_connects_to_listener_ipv4() {
        let ws_config = WsConfig::new(tcp::TcpConfig::new());

        let mut listener = ws_config.clone()
            .listen_on("/ip4/127.0.0.1/tcp/0/ws".parse().unwrap())
            .unwrap();

        let addr = listener.by_ref().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_eq!(Some(Protocol::Ws), addr.iter().nth(2));
        assert_ne!(Some(Protocol::Tcp(0)), addr.iter().nth(1));

        let listener = listener
            .filter_map(ListenerEvent::into_upgrade)
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(c, _)| c.unwrap().0);

        let dialer = ws_config.clone().dial(addr.clone()).unwrap();

        let future = listener
            .select(dialer)
            .map_err(|(e, _)| e)
            .and_then(|(_, n)| n);
        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    }

    #[test]
    fn dialer_connects_to_listener_ipv6() {
        let ws_config = WsConfig::new(tcp::TcpConfig::new());

        let mut listener = ws_config.clone()
            .listen_on("/ip6/::1/tcp/0/ws".parse().unwrap())
            .unwrap();

        let addr = listener.by_ref().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_eq!(Some(Protocol::Ws), addr.iter().nth(2));
        assert_ne!(Some(Protocol::Tcp(0)), addr.iter().nth(1));

        let listener = listener
            .filter_map(ListenerEvent::into_upgrade)
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(c, _)| c.unwrap().0);

        let dialer = ws_config.clone().dial(addr.clone()).unwrap();

        let future = listener
            .select(dialer)
            .map_err(|(e, _)| e)
            .and_then(|(_, n)| n);

        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    }

    #[test]
    fn nat_traversal() {
        let ws_config = WsConfig::new(tcp::TcpConfig::new());

        {
            let server = "/ip4/127.0.0.1/tcp/10000/ws".parse::<Multiaddr>().unwrap();
            let observed = "/ip4/80.81.82.83/tcp/25000/ws"
                .parse::<Multiaddr>()
                .unwrap();
            assert_eq!(
                ws_config.nat_traversal(&server, &observed).unwrap(),
                "/ip4/80.81.82.83/tcp/10000/ws"
                    .parse::<Multiaddr>()
                    .unwrap()
            );
        }

        {
            let server = "/ip4/127.0.0.1/tcp/10000/wss".parse::<Multiaddr>().unwrap();
            let observed = "/ip4/80.81.82.83/tcp/25000/wss"
                .parse::<Multiaddr>()
                .unwrap();
            assert_eq!(
                ws_config.nat_traversal(&server, &observed).unwrap(),
                "/ip4/80.81.82.83/tcp/10000/wss"
                    .parse::<Multiaddr>()
                    .unwrap()
            );
        }

        {
            let server = "/ip4/127.0.0.1/tcp/10000/ws".parse::<Multiaddr>().unwrap();
            let observed = "/ip4/80.81.82.83/tcp/25000/wss"
                .parse::<Multiaddr>()
                .unwrap();
            assert_eq!(
                ws_config.nat_traversal(&server, &observed).unwrap(),
                "/ip4/80.81.82.83/tcp/10000/ws"
                    .parse::<Multiaddr>()
                    .unwrap()
            );
        }

        {
            let server = "/ip4/127.0.0.1/tcp/10000/wss".parse::<Multiaddr>().unwrap();
            let observed = "/ip4/80.81.82.83/tcp/25000/ws"
                .parse::<Multiaddr>()
                .unwrap();
            assert_eq!(
                ws_config.nat_traversal(&server, &observed).unwrap(),
                "/ip4/80.81.82.83/tcp/10000/wss"
                    .parse::<Multiaddr>()
                    .unwrap()
            );
        }
    }
}
