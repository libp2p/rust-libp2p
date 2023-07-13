use futures::channel::oneshot;
use futures::{
    future::FutureExt, // for `.fuse()`
    pin_mut,
    select,
};
use futures::{AsyncReadExt, AsyncWriteExt, AsyncWrite};
use getrandom::getrandom;
use libp2p_core::Transport;
use libp2p_core::multiaddr::multiaddr;
use libp2p_core::transport::TransportEvent;
use libp2p_identity::PeerId;
use libp2p_wasm_ext::{ffi::tcp_transport, Connection};
use multiaddr::Multiaddr;
use rand::Rng;
use std::future::poll_fn;
use std::pin::Pin;
use wasm_bindgen_futures::spawn_local;
use wasm_bindgen_test::wasm_bindgen_test;


use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn _log(s: &str);
}


async fn new_echo_listener() -> (Multiaddr, oneshot::Sender<()>) {
    let (sender, receiver) = oneshot::channel::<()>();

    let port = rand::thread_rng().gen_range(2000u16..65535u16);
    let listener_multiaddress = multiaddr!(Ip4([127, 0, 0, 1]), Tcp(port));
    let addr_copy = listener_multiaddress.clone();

    spawn_local(async move {
        let mut transport = build_tcp_transport();
        let listener_id = libp2p_core::transport::ListenerId::next();
        let _ = transport.listen_on(listener_id, addr_copy).unwrap();

        let mut echo_loops: Vec<oneshot::Sender<()>> = vec![];

        let t1 = receiver.fuse();

        pin_mut!(t1);

        loop {
            select! {
                _ = t1 => {
                    for l in echo_loops.into_iter() {
                        let _ = l.send(());
                    }
                    assert_eq!(transport.remove_listener(listener_id), true);
                    return ();
                },
                b = poll_fn(|cx| Pin::new(&mut transport).poll(cx)).fuse() => {
                    match b {
                        TransportEvent::Incoming { listener_id: _, upgrade, local_addr: _, send_back_addr: _ } => {
                            let mut conn = upgrade.await.unwrap();
                            
                            let (tx, rx) = oneshot::channel::<()>();
                            echo_loops.push(tx);

                            spawn_local(async move {
                                let frx = rx.fuse();

                                pin_mut!(frx);

                                loop {
                                    select!{
                                        _ = frx => return (),
                                        _ = echo_back(&mut conn).fuse() => continue
                                    }
                                }
                                
                            });
                        },
                        TransportEvent::NewAddress { listener_id: _, listen_addr: _ } => {},
                        TransportEvent::AddressExpired { listener_id: _, listen_addr: _ } => panic!("This should not happen"),
                        TransportEvent::ListenerClosed { listener_id: _, reason: _ } => panic!("This should not happen, listener should not close"),
                        TransportEvent::ListenerError { listener_id: _, error: _ } => panic!("There should not be a listener error"),
                    }
                }
            }
        }
    });

    (listener_multiaddress, sender)
}


#[wasm_bindgen_test]
async fn single_connection_to_single_listener() {
    let (addr, terminate) = new_echo_listener().await;

    let mut conn = new_connection_to_echo_listener(addr).await;
    send_recv(&mut conn).await;

    let _ = terminate.send(());
}

#[wasm_bindgen_test]
async fn read_leftovers() {
    let (addr, terminate) = new_echo_listener().await;

    let mut conn = new_connection_to_echo_listener(addr).await;

    send_recv(&mut conn).await;

    conn.write_all(b"hello").await.unwrap();

    let mut buf = [0u8; 3];

    // Read first half
    let len = conn.read(&mut buf[..]).await.unwrap();
    assert_eq!(len, 3);
    assert_eq!(&buf[..len], b"hel");

    // Read second half
    let len = conn.read(&mut buf[..2]).await.unwrap();
    assert_eq!(len, 2);
    assert_eq!(&buf[..len], b"lo");

    let _ = terminate.send(());
}

#[wasm_bindgen_test]
async fn read_error_after_connection_close() {
    let (addr, terminate) = new_echo_listener().await;

    let mut conn = new_connection_to_echo_listener(addr).await;
    send_recv(&mut conn).await;

    poll_fn(|cx| Pin::new(&mut conn).poll_close(cx))
        .await
        .unwrap();

    let mut buf = [0u8; 16];
    conn
        .read(&mut buf)
        .await
        .expect_err("read error after conn drop");

    let _ = terminate.send(());
}

#[wasm_bindgen_test]
async fn write_error_after_connection_close() {
    let (addr, terminate) = new_echo_listener().await;

    let mut conn = new_connection_to_echo_listener(addr).await;
    send_recv(&mut conn).await;
    
    poll_fn(|cx| Pin::new(&mut conn).poll_close(cx))
        .await
        .unwrap();

    let buf = [1u8; 16];
    // dummy write - the poll_write operation checks the status of the current promise in the next call
    // before the close is properly propagated.
    conn
        .write(&buf)
        .await
        .unwrap();

    conn
        .write(&buf)
        .await
        .expect_err("write error after conn drop");

    let _ = terminate.send(());
}

#[wasm_bindgen_test]
async fn connect_without_peer_id() {
    let (mut addr, terminate) = new_echo_listener().await;

    // Remove peer id
    addr.push(multiaddr::Protocol::P2p(PeerId::random()));
    addr.pop();

    let mut transport = build_tcp_transport();
    transport.dial(addr).unwrap().await.unwrap();

    let _ = terminate.send(());
}

fn build_tcp_transport() -> libp2p_wasm_ext::ExtTransport {
    libp2p_wasm_ext::ExtTransport::new(tcp_transport())
}

async fn new_connection_to_echo_listener(addr: Multiaddr) -> Connection {
    let mut transport = build_tcp_transport();
    transport.dial(addr).unwrap().await.unwrap()
}

fn send_recv_task(mut conn: Connection) -> oneshot::Receiver<()> {
    let (tx, rx) = oneshot::channel();

    spawn_local(async move {
        send_recv(&mut conn).await;
        tx.send(()).unwrap();
    });

    rx
}

async fn send_recv(conn: &mut Connection) {
    let mut send_buf = [0u8; 1024];
    let mut recv_buf = [0u8; 1024];

    for _ in 0..30 {
        getrandom(&mut send_buf).unwrap();

        conn.write_all(&send_buf).await.unwrap();
        conn.read_exact(&mut recv_buf).await.unwrap();

        assert_eq!(send_buf, recv_buf);
    }
}

async fn echo_back(conn: &mut Connection) {
    let mut buf = [0u8; 1024];

    conn.read(&mut buf).await.unwrap();
    conn.write_all(&buf).await.unwrap();
}