use std::{
    collections::{hash_map::Entry, VecDeque},
    future::Future,
    pin::Pin,
    sync::LazyLock,
    task::{Context, Poll},
};

use fnv::FnvHashMap;
use futures::{
    channel::mpsc,
    future::{self, Ready},
    prelude::*,
};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, Transport, TransportError, TransportEvent},
};
use parking_lot::Mutex;
use rand::random;
use rw_stream_sink::RwStreamSink;

use crate::common::{BluetoothAddr, BluetoothTransportError};

type ChannelSender = mpsc::Sender<(Channel<Vec<u8>>, BluetoothAddr)>;
type ChannelReceiver = mpsc::Receiver<(Channel<Vec<u8>>, BluetoothAddr)>;

static HUB: LazyLock<Hub> = LazyLock::new(|| Hub(Mutex::new(FnvHashMap::default())));

struct Hub(Mutex<fnv::FnvHashMap<BluetoothAddr, ChannelSender>>);

impl Hub {
    fn register_addr(
        &self,
        requested: Option<BluetoothAddr>,
    ) -> Option<(ChannelReceiver, BluetoothAddr)> {
        let mut hub = self.0.lock();

        let addr = if let Some(addr) = requested {
            if hub.contains_key(&addr) {
                return None;
            }
            addr
        } else {
            loop {
                let candidate = random_local_addr();
                if !hub.contains_key(&candidate) {
                    break candidate;
                }
            }
        };

        let (tx, rx) = mpsc::channel(2);
        match hub.entry(addr) {
            Entry::Occupied(_) => return None,
            Entry::Vacant(entry) => {
                entry.insert(tx);
            }
        }

        Some((rx, addr))
    }

    fn unregister_addr(&self, addr: &BluetoothAddr) -> Option<ChannelSender> {
        self.0.lock().remove(addr)
    }

    fn get(&self, addr: &BluetoothAddr) -> Option<ChannelSender> {
        self.0.lock().get(addr).cloned()
    }
}

#[derive(Default)]
pub struct BluetoothTransport {
    listeners: VecDeque<Pin<Box<Listener>>>,
}

impl BluetoothTransport {
    pub fn new() -> Self {
        Self::default()
    }
}

pub type Channel<T> = RwStreamSink<Chan<T>>;

pub struct Chan<T = Vec<u8>> {
    incoming: mpsc::Receiver<T>,
    outgoing: mpsc::Sender<T>,
    dial_addr: Option<BluetoothAddr>,
}

impl<T> Unpin for Chan<T> {}

impl<T> Stream for Chan<T> {
    type Item = Result<T, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Stream::poll_next(Pin::new(&mut self.incoming), cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(item)) => Poll::Ready(Some(Ok(item))),
            Poll::Ready(None) => Poll::Ready(None),
        }
    }
}

impl<T> Sink<T> for Chan<T> {
    type Error = std::io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.outgoing), cx).map_err(map_channel_err)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.outgoing), item).map_err(map_channel_err)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.outgoing), cx).map_err(map_channel_err)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.outgoing), cx).map_err(map_channel_err)
    }
}

fn map_channel_err(error: mpsc::SendError) -> std::io::Error {
    if error.is_full() {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, error)
    } else {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "channel closed")
    }
}

impl<T> Drop for Chan<T> {
    fn drop(&mut self) {
        if let Some(addr) = self.dial_addr {
            let channel_sender = HUB.unregister_addr(&addr);
            debug_assert!(channel_sender.is_some());
        }
    }
}

pub struct DialFuture {
    dial_addr: BluetoothAddr,
    sender: ChannelSender,
    channel_to_send: Option<Channel<Vec<u8>>>,
    channel_to_return: Option<Channel<Vec<u8>>>,
}

impl DialFuture {
    fn new(remote: BluetoothAddr) -> Option<Self> {
        let sender = HUB.get(&remote)?;

        let (_dial_receiver, dial_addr) = HUB
            .register_addr(None)
            .expect("random bluetooth address generation to succeed");

        let (a_tx, a_rx) = mpsc::channel(4096);
        let (b_tx, b_rx) = mpsc::channel(4096);

        Some(DialFuture {
            dial_addr,
            sender,
            channel_to_send: Some(RwStreamSink::new(Chan {
                incoming: a_rx,
                outgoing: b_tx,
                dial_addr: None,
            })),
            channel_to_return: Some(RwStreamSink::new(Chan {
                incoming: b_rx,
                outgoing: a_tx,
                dial_addr: Some(dial_addr),
            })),
        })
    }
}

impl Future for DialFuture {
    type Output = Result<Channel<Vec<u8>>, BluetoothTransportError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.sender.poll_ready(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(_)) => return Poll::Ready(Err(BluetoothTransportError::Unreachable)),
        }

        let channel_to_send = self
            .channel_to_send
            .take()
            .expect("Future should not be polled after completion");
        let dial_addr = self.dial_addr;
        if self
            .sender
            .start_send((channel_to_send, dial_addr))
            .is_err()
        {
            return Poll::Ready(Err(BluetoothTransportError::Unreachable));
        }

        Poll::Ready(Ok(self
            .channel_to_return
            .take()
            .expect("Future should not be polled after completion")))
    }
}

impl Transport for BluetoothTransport {
    type Output = Channel<Vec<u8>>;
    type Error = BluetoothTransportError;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = DialFuture;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        let requested_addr =
            parse_bluetooth_addr(&addr).map_err(|_| TransportError::MultiaddrNotSupported(addr))?;

        let (receiver, actual_addr) = match requested_addr {
            Some(addr) => HUB
                .register_addr(Some(addr))
                .ok_or(TransportError::Other(BluetoothTransportError::AlreadyInUse))?,
            None => HUB
                .register_addr(None)
                .ok_or(TransportError::Other(BluetoothTransportError::Unreachable))?,
        };

        let listen_addr = actual_addr.to_multiaddr();
        let listener = Listener {
            id,
            addr: listen_addr.clone(),
            receiver,
            tell_listen_addr: true,
            registered_addr: actual_addr,
        };

        self.listeners.push_back(Box::pin(listener));

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(index) = self.listeners.iter().position(|listener| listener.id == id) {
            let listener = self.listeners.get_mut(index).expect("index valid");
            let val_in = HUB.unregister_addr(&listener.registered_addr);
            debug_assert!(val_in.is_some());
            listener.receiver.close();
            true
        } else {
            false
        }
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let remote = match parse_bluetooth_addr(&addr) {
            Ok(Some(addr)) => addr,
            _ => return Err(TransportError::MultiaddrNotSupported(addr)),
        };

        DialFuture::new(remote).ok_or(TransportError::Other(BluetoothTransportError::Unreachable))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let mut remaining = self.listeners.len();
        while let Some(mut listener) = self.listeners.pop_back() {
            if listener.tell_listen_addr {
                listener.tell_listen_addr = false;
                let listen_addr = listener.addr.clone();
                let listener_id = listener.id;
                self.listeners.push_front(listener);
                return Poll::Ready(TransportEvent::NewAddress {
                    listen_addr,
                    listener_id,
                });
            }

            let event = match Stream::poll_next(Pin::new(&mut listener.receiver), cx) {
                Poll::Pending => None,
                Poll::Ready(Some((channel, dial_addr))) => Some(TransportEvent::Incoming {
                    listener_id: listener.id,
                    upgrade: future::ready(Ok(channel)),
                    local_addr: listener.addr.clone(),
                    send_back_addr: dial_addr.to_multiaddr(),
                }),
                Poll::Ready(None) => {
                    return Poll::Ready(TransportEvent::ListenerClosed {
                        listener_id: listener.id,
                        reason: Ok(()),
                    });
                }
            };

            self.listeners.push_front(listener);
            if let Some(event) = event {
                return Poll::Ready(event);
            }

            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }

        Poll::Pending
    }
}

pub struct Listener {
    id: ListenerId,
    addr: Multiaddr,
    receiver: ChannelReceiver,
    tell_listen_addr: bool,
    registered_addr: BluetoothAddr,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = HUB.unregister_addr(&self.registered_addr);
    }
}

fn parse_bluetooth_addr(addr: &Multiaddr) -> Result<Option<BluetoothAddr>, ()> {
    let mut protocols = addr.iter();
    match protocols.next() {
        Some(Protocol::Memory(value)) => match protocols.next() {
            None | Some(Protocol::P2p(_)) => {
                if value == 0 {
                    Ok(None)
                } else {
                    BluetoothAddr::from_u64(value).map(Some).ok_or(())
                }
            }
            _ => Err(()),
        },
        _ => Err(()),
    }
}

fn random_local_addr() -> BluetoothAddr {
    loop {
        let raw: u64 = random();
        let bytes = raw.to_be_bytes();
        let mut addr = [0u8; 6];
        addr.copy_from_slice(&bytes[2..]);
        addr[0] |= 0x02; // locally administered
        addr[0] &= 0xfe; // unicast
        if addr.iter().any(|b| *b != 0) {
            return BluetoothAddr::new(addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{
        executor::block_on,
        io::{AsyncReadExt, AsyncWriteExt},
    };
    use libp2p_core::transport::{DialOpts, ListenerId, TransportEvent};
    use libp2p_core::{transport::Transport, Multiaddr};
    use std::str::FromStr;

    #[test]
    fn dial_and_accept() {
        let mut listener = BluetoothTransport::new();
        let listen_addr: Multiaddr = BluetoothAddr::from_str("02:00:00:00:00:01")
            .unwrap()
            .to_multiaddr();
        let listener_id = ListenerId::next();
        listener
            .listen_on(listener_id, listen_addr.clone())
            .unwrap();

        // Consume the initial NewAddress event.
        let event = block_on(futures::future::poll_fn(|cx| {
            Pin::new(&mut listener).poll(cx)
        }));
        matches!(event, TransportEvent::NewAddress { .. })
            .then_some(())
            .expect("listener announces new address");

        let mut dialer = BluetoothTransport::new();
        let dial_opts = DialOpts::unknown_peer_id()
            .address(listen_addr.clone())
            .build();
        let dial_future = dialer.dial(listen_addr.clone(), dial_opts).unwrap();

        let mut dial_conn = block_on(dial_future).unwrap();

        let mut listener_conn = loop {
            let event = block_on(futures::future::poll_fn(|cx| {
                Pin::new(&mut listener).poll(cx)
            }));
            if let TransportEvent::Incoming { upgrade, .. } = event {
                break block_on(upgrade).unwrap();
            }
        };

        block_on(dial_conn.write_all(b"ping")).unwrap();
        let mut buf = [0u8; 4];
        block_on(listener_conn.read_exact(&mut buf)).unwrap();
        assert_eq!(&buf, b"ping");
    }
}
