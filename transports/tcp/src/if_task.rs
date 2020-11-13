use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
    select,
};
use if_watch::{IfEvent, IfWatcher, IpNet};
use std::{io, net::SocketAddr};

#[derive(Clone, Copy, Debug)]
pub enum IfTaskEvent {
    NewAddress(SocketAddr),
    AddressExpired(SocketAddr),
}

#[derive(Debug)]
enum Command<T> {
    RegisterListener(SocketAddr, T, mpsc::UnboundedSender<IfTaskEvent>),
    PortReuseSocket(SocketAddr, oneshot::Sender<Option<T>>),
}

#[derive(Clone, Debug)]
pub struct IfTaskHandle<T> {
    tx: mpsc::UnboundedSender<Command<T>>,
}

impl<T: Clone + Send + 'static> IfTaskHandle<T> {
    pub async fn new() -> io::Result<Self> {
        // channel is unbounded so that `register_listener` doesn't need to be async.
        let (tx, rx) = mpsc::unbounded();
        let task = Task::new(rx).await?.spawn();
        std::thread::spawn(move || async_io::block_on(task));
        Ok(Self { tx })
    }

    pub fn register_listener(
        &self,
        socket_addr: &SocketAddr,
        user_data: T,
    ) -> mpsc::UnboundedReceiver<IfTaskEvent> {
        let (tx, rx) = mpsc::unbounded();
        let cmd = Command::RegisterListener(*socket_addr, user_data, tx);
        self.tx.unbounded_send(cmd).expect("task paniced");
        rx
    }

    pub async fn port_reuse_socket(&self, socket_addr: &SocketAddr) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        let cmd = Command::PortReuseSocket(*socket_addr, tx);
        self.tx.unbounded_send(cmd).expect("task paniced");
        rx.await.expect("task paniced")
    }
}

#[derive(Debug)]
struct Task<T> {
    /// Command receiver.
    rx: mpsc::UnboundedReceiver<Command<T>>,
    /// Interface watcher.
    watcher: IfWatcher,
    /// Listening addresses.
    listening_addrs: Vec<(SocketAddr, T, mpsc::UnboundedSender<IfTaskEvent>)>,
}

impl<T: Clone + Send + 'static> Task<T> {
    async fn new(rx: mpsc::UnboundedReceiver<Command<T>>) -> io::Result<Self> {
        Ok(Self {
            rx,
            watcher: IfWatcher::new().await?,
            listening_addrs: Default::default(),
        })
    }

    async fn spawn(mut self) {
        let mut rx = self.rx.fuse();
        loop {
            select! {
                if_event = self.watcher.next().fuse() => {
                    let event = match if_event {
                        Ok(event) => event,
                        Err(err) => {
                            log::error!("{:?}", err);
                            continue;
                        }
                    };
                    self.listening_addrs.retain(|(addr, _, tx)| {
                        match event {
                            IfEvent::Up(inet) => {
                                if let Some(addr) = iface_match(&inet, &addr) {
                                    tx.unbounded_send(IfTaskEvent::NewAddress(addr)).is_ok()
                                } else {
                                    !tx.is_closed()
                                }
                            }
                            IfEvent::Down(inet) => {
                                if let Some(addr) = iface_match(&inet, &addr) {
                                    tx.unbounded_send(IfTaskEvent::AddressExpired(addr)).is_ok()
                                } else {
                                    !tx.is_closed()
                                }
                            }
                        }
                    });
                }
                cmd = rx.next() => {
                    match cmd {
                        Some(Command::RegisterListener(socket_addr, user_data, tx)) => {
                            for iface in self.watcher.iter() {
                                if let Some(addr) = iface_match(iface, &socket_addr) {
                                    tx.unbounded_send(IfTaskEvent::NewAddress(addr)).ok();
                                }
                            }
                            self.listening_addrs.push((socket_addr, user_data, tx));
                        }
                        Some(Command::PortReuseSocket(socket_addr, tx)) => {
                            let mut reuse_socket = None;
                            for (addr, user_data, _) in &self.listening_addrs {
                                if addr.ip().is_ipv4() == socket_addr.ip().is_ipv4()
                                    && addr.ip().is_loopback() == socket_addr.ip().is_loopback() {
                                    reuse_socket = Some(user_data.clone());
                                    break;
                                }
                            }
                            tx.send(reuse_socket).ok();
                        }
                        None => {}
                    }
                }
            }
        }
    }
}

fn iface_match(inet: &IpNet, socket_addr: &SocketAddr) -> Option<SocketAddr> {
    let matches = if socket_addr.ip().is_unspecified() {
        socket_addr.ip().is_ipv4() == inet.addr().is_ipv4()
    } else {
        inet.addr() == socket_addr.ip()
    };
    if matches {
        Some(SocketAddr::new(inet.addr(), socket_addr.port()))
    } else {
        None
    }
}
