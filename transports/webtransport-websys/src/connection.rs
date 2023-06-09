use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent, StreamMuxerExt};
use libp2p_core::{OutboundUpgrade, UpgradeInfo};
use libp2p_identity::{Keypair, PeerId};
use multihash::Multihash;
use send_wrapper::SendWrapper;
use std::collections::HashSet;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use wasm_bindgen_futures::JsFuture;
use web_sys::ReadableStreamDefaultReader;

use crate::bindings::{WebTransport, WebTransportBidirectionalStream};
use crate::endpoint::Endpoint;
use crate::fused_js_promise::FusedJsPromise;
use crate::stream::StreamSend;
use crate::utils::{detach_promise, parse_reader_response, to_js_type};
use crate::{Error, Stream};

/// An opened WebTransport connection.
#[derive(Debug)]
pub struct Connection {
    session: WebTransport,
    create_stream_promise: FusedJsPromise,
    incoming_stream_promise: FusedJsPromise,
    incoming_streams_reader: ReadableStreamDefaultReader,
    closed: bool,
}

/// Connection wrapped in [`SendWrapper`].
///
/// This is needed by Swarm. WASM is single-threaded and it is safe
/// to use [`SendWrapper`].
#[derive(Debug)]
pub(crate) struct ConnectionSend {
    inner: SendWrapper<Connection>,
}

impl Connection {
    pub(crate) fn new(endpoint: &Endpoint) -> Result<Self, Error> {
        let url = endpoint.url();

        let session = if endpoint.certhashes.is_empty() {
            // Endpoint has CA-signed TLS certificate.
            WebTransport::new(&url).map_err(Error::from_js_value)?
        } else {
            // Endpoint has self-signed TLS certificates.
            let opts = endpoint.webtransport_opts();
            WebTransport::new_with_options(&url, &opts).map_err(Error::from_js_value)?
        };

        let incoming_streams = session.incoming_bidirectional_streams();
        let incoming_streams_reader =
            to_js_type::<ReadableStreamDefaultReader>(incoming_streams.get_reader())?;

        Ok(Connection {
            session,
            create_stream_promise: FusedJsPromise::new(),
            incoming_stream_promise: FusedJsPromise::new(),
            incoming_streams_reader,
            closed: false,
        })
    }

    /// Authenticates with the server
    ///
    /// This methods runs the security handshake as descripted
    /// in the [spec][1]. It validates the certhashes and peer ID
    /// of the server.
    ///
    /// [1]: https://github.com/libp2p/specs/tree/master/webtransport#security-handshake
    pub(crate) async fn authenticate(
        &mut self,
        keypair: &Keypair,
        remote_peer: Option<PeerId>,
        certhashes: HashSet<Multihash<64>>,
    ) -> Result<PeerId, Error> {
        JsFuture::from(self.session.ready())
            .await
            .map_err(Error::from_js_value)?;

        let stream = StreamSend::new(self.create_stream().await?);
        let mut noise = libp2p_noise::Config::new(keypair)?;

        if !certhashes.is_empty() {
            noise = noise.with_webtransport_certhashes(certhashes);
        }

        // We do not use `upgrade::apply_outbound` function because it uses
        // `multistream_select` protocol, which is not used by WebTransport spec.
        let info = noise.protocol_info().next().unwrap_or_default();
        let (peer_id, _io) = noise.upgrade_outbound(stream, info).await?;

        // TODO: This should be part libp2p-noise
        if let Some(expected_peer_id) = remote_peer {
            if peer_id != expected_peer_id {
                return Err(Error::UnknownRemotePeerId);
            }
        }

        Ok(peer_id)
    }

    /// Creates new outbound stream.
    async fn create_stream(&mut self) -> Result<Stream, Error> {
        poll_fn(|cx| self.poll_outbound_unpin(cx)).await
    }

    /// Initiates and polls a promise from `create_bidirectional_stream`.
    fn poll_create_bidirectional_stream(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Stream, Error>> {
        // Create bidirectional stream
        let val = ready!(self
            .create_stream_promise
            .maybe_init_and_poll(cx, || self.session.create_bidirectional_stream()))
        .map_err(Error::from_js_value)?;

        let bidi_stream = to_js_type::<WebTransportBidirectionalStream>(val)?;
        let stream = Stream::new(bidi_stream)?;

        Poll::Ready(Ok(stream))
    }

    /// Polls for incoming stream from `incoming_bidirectional_streams` reader.
    fn poll_incoming_bidirectional_streams(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Stream, Error>> {
        // Read the next incoming stream from the JS channel
        let val = ready!(self
            .incoming_stream_promise
            .maybe_init_and_poll(cx, || self.incoming_streams_reader.read()))
        .map_err(Error::from_js_value)?;

        let val = parse_reader_response(&val)
            .map_err(Error::from_js_value)?
            .ok_or_else(|| Error::JsError("incoming_bidirectional_streams closed".to_string()))?;

        let bidi_stream = to_js_type::<WebTransportBidirectionalStream>(val)?;
        let stream = Stream::new(bidi_stream)?;

        Poll::Ready(Ok(stream))
    }

    /// Closes the session.
    ///
    /// This closes the streams also and they will return an error
    /// when they will be used.
    fn close_session(&mut self) {
        if !self.closed {
            detach_promise(self.incoming_streams_reader.cancel());
            self.session.close();
            self.closed = true;
        }
    }
}

/// WebTransport native multiplexing
impl StreamMuxer for Connection {
    type Substream = Stream;
    type Error = Error;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.poll_incoming_bidirectional_streams(cx)
    }

    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.poll_create_bidirectional_stream(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.close_session();
        Poll::Ready(Ok(()))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        Poll::Pending
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close_session();
    }
}

impl ConnectionSend {
    pub(crate) fn new(conn: Connection) -> Self {
        ConnectionSend {
            inner: SendWrapper::new(conn),
        }
    }
}

impl StreamMuxer for ConnectionSend {
    type Substream = StreamSend;
    type Error = Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let stream = ready!(self.get_mut().inner.poll_inbound_unpin(cx))?;
        Poll::Ready(Ok(StreamSend::new(stream)))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let stream = ready!(self.get_mut().inner.poll_outbound_unpin(cx))?;
        Poll::Ready(Ok(StreamSend::new(stream)))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().inner.poll_close_unpin(cx)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        self.get_mut().inner.poll_unpin(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::oneshot;
    use futures::{AsyncReadExt, AsyncWriteExt};
    use getrandom::getrandom;
    use libp2p_core::Transport as _;
    use libp2p_identity::Keypair;
    use multiaddr::{Multiaddr, Protocol};
    use std::future::poll_fn;
    use wasm_bindgen_futures::{spawn_local, JsFuture};
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::{window, Response};

    use crate::utils::to_js_type;
    use crate::{Config, Stream, Transport};

    #[wasm_bindgen_test]
    async fn single_conn_single_stream() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        let (_peer_id, mut conn) = transport.dial(addr).unwrap().await.unwrap();
        let mut stream = create_stream(&mut conn).await;

        send_recv(&mut stream).await;
    }

    #[wasm_bindgen_test]
    async fn single_conn_single_stream_incoming() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        let (_peer_id, mut conn) = transport.dial(addr).unwrap().await.unwrap();
        let mut stream = incoming_stream(&mut conn).await;

        send_recv(&mut stream).await;
    }

    #[wasm_bindgen_test]
    async fn single_conn_multiple_streams() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));
        let mut tasks = Vec::new();

        let (_peer_id, mut conn) = transport.dial(addr).unwrap().await.unwrap();
        let mut streams = Vec::new();

        for i in 0..30 {
            let stream = if i % 2 == 0 {
                create_stream(&mut conn).await
            } else {
                incoming_stream(&mut conn).await
            };

            streams.push(stream);
        }

        for stream in streams {
            tasks.push(send_recv_task(stream));
        }

        futures::future::try_join_all(tasks).await.unwrap();
    }

    #[wasm_bindgen_test]
    async fn multiple_conn_multiple_streams() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));
        let mut tasks = Vec::new();
        let mut conns = Vec::new();

        for _ in 0..10 {
            let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();
            let mut streams = Vec::new();

            for i in 0..10 {
                let stream = if i % 2 == 0 {
                    create_stream(&mut conn).await
                } else {
                    incoming_stream(&mut conn).await
                };

                streams.push(stream);
            }

            // If `conn` gets drop then its streams will close.
            // Keep it alive by moving it to the outer scope.
            conns.push(conn);

            for stream in streams {
                tasks.push(send_recv_task(stream));
            }
        }

        futures::future::try_join_all(tasks).await.unwrap();
    }

    #[wasm_bindgen_test]
    async fn multiple_conn_multiple_streams_sequential() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        for _ in 0..10 {
            let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();

            for i in 0..10 {
                let mut stream = if i % 2 == 0 {
                    create_stream(&mut conn).await
                } else {
                    incoming_stream(&mut conn).await
                };

                send_recv(&mut stream).await;
            }
        }
    }

    #[wasm_bindgen_test]
    async fn allow_read_after_closing_writer() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));
        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();

        let mut stream = create_stream(&mut conn).await;

        // Test that stream works
        send_recv(&mut stream).await;

        // Write random data
        let mut send_buf = [0u8; 1024];
        getrandom(&mut send_buf).unwrap();
        stream.write_all(&send_buf).await.unwrap();

        // Close writer by calling AsyncWrite::poll_close
        stream.close().await.unwrap();

        // Make sure writer is closed
        stream.write_all(b"1").await.unwrap_err();

        // We should be able to read
        let mut recv_buf = [0u8; 1024];
        stream.read_exact(&mut recv_buf).await.unwrap();

        assert_eq!(send_buf, recv_buf);
    }

    #[wasm_bindgen_test]
    async fn poll_outbound_error_after_connection_close() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));
        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();

        // Make sure that poll_outbound works well before closing the connection
        let mut stream = create_stream(&mut conn).await;
        send_recv(&mut stream).await;
        drop(stream);

        poll_fn(|cx| Pin::new(&mut conn).poll_close(cx))
            .await
            .unwrap();

        poll_fn(|cx| Pin::new(&mut conn).poll_outbound(cx))
            .await
            .expect_err("poll_outbound error after conn closed");
    }

    #[wasm_bindgen_test]
    async fn poll_inbound_error_after_connection_close() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));
        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();

        // Make sure that poll_inbound works well before closing the connection
        let mut stream = incoming_stream(&mut conn).await;
        send_recv(&mut stream).await;
        drop(stream);

        poll_fn(|cx| Pin::new(&mut conn).poll_close(cx))
            .await
            .unwrap();

        poll_fn(|cx| Pin::new(&mut conn).poll_inbound(cx))
            .await
            .expect_err("poll_inbound error after conn closed");
    }

    #[wasm_bindgen_test]
    async fn read_error_after_connection_drop() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();
        let mut stream = create_stream(&mut conn).await;

        send_recv(&mut stream).await;

        drop(conn);

        let mut buf = [0u8; 16];
        stream
            .read(&mut buf)
            .await
            .expect_err("read error after conn drop");
    }

    #[wasm_bindgen_test]
    async fn read_error_after_connection_close() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();
        let mut stream = create_stream(&mut conn).await;

        send_recv(&mut stream).await;

        poll_fn(|cx| Pin::new(&mut conn).poll_close(cx))
            .await
            .unwrap();

        let mut buf = [0u8; 16];
        stream
            .read(&mut buf)
            .await
            .expect_err("read error after conn drop");
    }

    #[wasm_bindgen_test]
    async fn write_error_after_connection_drop() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();
        let mut stream = create_stream(&mut conn).await;

        send_recv(&mut stream).await;

        drop(conn);

        let buf = [0u8; 16];
        stream
            .write(&buf)
            .await
            .expect_err("write error after conn drop");
    }

    #[wasm_bindgen_test]
    async fn write_error_after_connection_close() {
        let addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        let mut transport = Transport::new(Config::new(&keypair));

        let (_peer_id, mut conn) = transport.dial(addr.clone()).unwrap().await.unwrap();
        let mut stream = create_stream(&mut conn).await;

        send_recv(&mut stream).await;

        poll_fn(|cx| Pin::new(&mut conn).poll_close(cx))
            .await
            .unwrap();

        let buf = [0u8; 16];
        stream
            .write(&buf)
            .await
            .expect_err("write error after conn drop");
    }

    #[wasm_bindgen_test]
    async fn connect_without_peer_id() {
        let mut addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        // Remove peer id
        addr.pop();

        let mut transport = Transport::new(Config::new(&keypair));
        transport.dial(addr).unwrap().await.unwrap();
    }

    #[wasm_bindgen_test]
    async fn error_on_unknown_peer_id() {
        let mut addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        // Remove peer id
        addr.pop();

        // Add an unknown one
        addr.push(Protocol::P2p(PeerId::random()));

        let mut transport = Transport::new(Config::new(&keypair));
        let e = transport.dial(addr.clone()).unwrap().await.unwrap_err();
        assert!(matches!(e, Error::UnknownRemotePeerId));
    }

    #[wasm_bindgen_test]
    async fn error_on_unknown_certhash() {
        let mut addr = fetch_server_addr().await;
        let keypair = Keypair::generate_ed25519();

        // Remove peer id
        let peer_id = addr.pop().unwrap();

        // Add unknown certhash
        addr.push(Protocol::Certhash(Multihash::wrap(1, b"1").unwrap()));

        // Add peer id back
        addr.push(peer_id);

        let mut transport = Transport::new(Config::new(&keypair));
        let e = transport.dial(addr.clone()).unwrap().await.unwrap_err();
        assert!(matches!(
            e,
            Error::Noise(libp2p_noise::Error::UnknownWebTransportCerthashes(..))
        ));
    }

    /// Helper that returns the multiaddress of echo-server
    ///
    /// It fetches the multiaddress via HTTP request to
    /// 127.0.0.1:4455.
    async fn fetch_server_addr() -> Multiaddr {
        let url = "http://127.0.0.1:4455/";
        let window = window().expect("failed to get browser window");

        let value = JsFuture::from(window.fetch_with_str(url))
            .await
            .expect("fetch failed");
        let resp = to_js_type::<Response>(value).expect("cast failed");

        let text = resp.text().expect("text failed");
        let text = JsFuture::from(text).await.expect("text promise failed");

        text.as_string()
            .filter(|s| !s.is_empty())
            .expect("response not a text")
            .parse()
            .unwrap()
    }

    async fn create_stream(conn: &mut Connection) -> Stream {
        poll_fn(|cx| Pin::new(&mut *conn).poll_outbound(cx))
            .await
            .unwrap()
    }

    async fn incoming_stream(conn: &mut Connection) -> Stream {
        let mut stream = poll_fn(|cx| Pin::new(&mut *conn).poll_inbound(cx))
            .await
            .unwrap();

        // For the stream to be initiated `echo-server` sends a single byte
        let mut buf = [0u8; 1];
        stream.read_exact(&mut buf).await.unwrap();

        stream
    }

    fn send_recv_task(mut steam: Stream) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();

        spawn_local(async move {
            send_recv(&mut steam).await;
            tx.send(()).unwrap();
        });

        rx
    }

    async fn send_recv(stream: &mut Stream) {
        let mut send_buf = [0u8; 1024];
        let mut recv_buf = [0u8; 1024];

        for _ in 0..30 {
            getrandom(&mut send_buf).unwrap();

            stream.write_all(&send_buf).await.unwrap();
            stream.read_exact(&mut recv_buf).await.unwrap();

            assert_eq!(send_buf, recv_buf);
        }
    }
}
