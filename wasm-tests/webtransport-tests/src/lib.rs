#![allow(unexpected_cfgs)]
use std::{future::poll_fn, pin::Pin};

use futures::{channel::oneshot, AsyncReadExt, AsyncWriteExt};
use getrandom::getrandom;
use libp2p_core::{
    transport::{DialOpts, PortUse},
    Endpoint, StreamMuxer, Transport as _,
};
use libp2p_identity::{Keypair, PeerId};
use libp2p_noise as noise;
use libp2p_webtransport_websys::{Config, Connection, Error, Stream, Transport};
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{window, Response};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
pub async fn single_conn_single_stream() {
    let mut conn = new_connection_to_echo_server().await;
    let mut stream = create_stream(&mut conn).await;

    send_recv(&mut stream).await;
}

#[wasm_bindgen_test]
pub async fn single_conn_single_stream_incoming() {
    let mut conn = new_connection_to_echo_server().await;
    let mut stream = incoming_stream(&mut conn).await;

    send_recv(&mut stream).await;
}

#[wasm_bindgen_test]
pub async fn single_conn_multiple_streams() {
    let mut conn = new_connection_to_echo_server().await;
    let mut tasks = Vec::new();
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
pub async fn multiple_conn_multiple_streams() {
    let mut tasks = Vec::new();
    let mut conns = Vec::new();

    for _ in 0..10 {
        let mut conn = new_connection_to_echo_server().await;
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
pub async fn multiple_conn_multiple_streams_sequential() {
    for _ in 0..10 {
        let mut conn = new_connection_to_echo_server().await;

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
pub async fn read_leftovers() {
    let mut conn = new_connection_to_echo_server().await;
    let mut stream = create_stream(&mut conn).await;

    // Test that stream works
    send_recv(&mut stream).await;

    stream.write_all(b"hello").await.unwrap();

    let mut buf = [0u8; 3];

    // Read first half
    let len = stream.read(&mut buf[..]).await.unwrap();
    assert_eq!(len, 3);
    assert_eq!(&buf[..len], b"hel");

    // Read second half
    let len = stream.read(&mut buf[..]).await.unwrap();
    assert_eq!(len, 2);
    assert_eq!(&buf[..len], b"lo");
}

#[wasm_bindgen_test]
pub async fn allow_read_after_closing_writer() {
    let mut conn = new_connection_to_echo_server().await;
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
pub async fn poll_outbound_error_after_connection_close() {
    let mut conn = new_connection_to_echo_server().await;

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
pub async fn poll_inbound_error_after_connection_close() {
    let mut conn = new_connection_to_echo_server().await;

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
pub async fn read_error_after_connection_drop() {
    let mut conn = new_connection_to_echo_server().await;
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
pub async fn read_error_after_connection_close() {
    let mut conn = new_connection_to_echo_server().await;
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
pub async fn write_error_after_connection_drop() {
    let mut conn = new_connection_to_echo_server().await;
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
pub async fn write_error_after_connection_close() {
    let mut conn = new_connection_to_echo_server().await;
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
pub async fn connect_without_peer_id() {
    let mut addr = fetch_server_addr().await;
    let keypair = Keypair::generate_ed25519();

    // Remove peer id
    addr.pop();

    let mut transport = Transport::new(Config::new(&keypair));
    transport
        .dial(
            addr,
            DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
        )
        .unwrap()
        .await
        .unwrap();
}

#[wasm_bindgen_test]
pub async fn error_on_unknown_peer_id() {
    let mut addr = fetch_server_addr().await;
    let keypair = Keypair::generate_ed25519();

    // Remove peer id
    addr.pop();

    // Add an unknown one
    addr.push(Protocol::P2p(PeerId::random()));

    let mut transport = Transport::new(Config::new(&keypair));
    let e = transport
        .dial(
            addr.clone(),
            DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
        )
        .unwrap()
        .await
        .unwrap_err();
    assert!(matches!(e, Error::UnknownRemotePeerId));
}

#[wasm_bindgen_test]
pub async fn error_on_unknown_certhash() {
    let mut addr = fetch_server_addr().await;
    let keypair = Keypair::generate_ed25519();

    // Remove peer id
    let peer_id = addr.pop().unwrap();

    // Add unknown certhash
    addr.push(Protocol::Certhash(Multihash::wrap(1, b"1").unwrap()));

    // Add peer id back
    addr.push(peer_id);

    let mut transport = Transport::new(Config::new(&keypair));
    let e = transport
        .dial(
            addr.clone(),
            DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
        )
        .unwrap()
        .await
        .unwrap_err();
    assert!(matches!(
        e,
        Error::Noise(noise::Error::UnknownWebTransportCerthashes(..))
    ));
}

async fn new_connection_to_echo_server() -> Connection {
    let addr = fetch_server_addr().await;
    let keypair = Keypair::generate_ed25519();

    let mut transport = Transport::new(Config::new(&keypair));

    let (_peer_id, conn) = transport
        .dial(
            addr,
            DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
        )
        .unwrap()
        .await
        .unwrap();

    conn
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
    let resp = value.dyn_into::<Response>().expect("cast failed");

    let text = resp.text().expect("text failed");
    let text = JsFuture::from(text).await.expect("text promise failed");

    text.as_string()
        .filter(|s| !s.is_empty())
        .expect("response not a text")
        .parse()
        .unwrap()
}

#[allow(unknown_lints, clippy::needless_pass_by_ref_mut)] // False positive.
async fn create_stream(conn: &mut Connection) -> Stream {
    poll_fn(|cx| Pin::new(&mut *conn).poll_outbound(cx))
        .await
        .unwrap()
}

#[allow(unknown_lints, clippy::needless_pass_by_ref_mut)] // False positive.
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
