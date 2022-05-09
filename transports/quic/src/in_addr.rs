use if_watch::{IfEvent, IfWatcher};

use futures::{
    future::{BoxFuture, FutureExt},
    lock::Mutex,
    stream::{Stream, StreamExt},
};

use std::{
    io::Result,
    net::IpAddr,
    ops::DerefMut,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// Watches for interface changes.
#[derive(Clone, Debug)]
pub(crate) struct InAddr(Arc<Mutex<InAddrInner>>);

impl InAddr {
    /// If ip is specified then only one `IfEvent::Up` with IpNet(ip)/32 will be generated.
    /// If ip is unspecified then `IfEvent::Up/Down` events will be generated for all interfaces.
    pub(crate) fn new(ip: IpAddr) -> Self {
        let inner = if ip.is_unspecified() {
            let watcher = IfWatch::Pending(IfWatcher::new().boxed());
            InAddrInner::Any {
                if_watch: Box::new(watcher),
            }
        } else {
            InAddrInner::One { ip: Some(ip) }
        };
        Self(Arc::new(Mutex::new(inner)))
    }
}

/// The listening addresses of a `UdpSocket`.
#[derive(Debug)]
enum InAddrInner {
    /// The socket accepts connections on a single interface.
    One { ip: Option<IpAddr> },
    /// The socket accepts connections on all interfaces.
    Any { if_watch: Box<IfWatch> },
}

enum IfWatch {
    Pending(BoxFuture<'static, std::io::Result<IfWatcher>>),
    Ready(Box<IfWatcher>),
}

impl std::fmt::Debug for IfWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            IfWatch::Pending(_) => write!(f, "Pending"),
            IfWatch::Ready(_) => write!(f, "Ready"),
        }
    }
}

impl Stream for InAddr {
    type Item = Result<IfEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        let mut lock = me.0.lock();
        let mut guard = futures::ready!(lock.poll_unpin(cx));
        let inner = &mut *guard;

        inner.poll_next_unpin(cx)
    }
}

impl Stream for InAddrInner {
    type Item = Result<IfEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        loop {
            match me {
                // If the listener is bound to a single interface, make sure the
                // address is reported once.
                InAddrInner::One { ip } => {
                    if let Some(ip) = ip.take() {
                        return Poll::Ready(Some(Ok(IfEvent::Up(ip.into()))));
                    }
                }
                InAddrInner::Any { if_watch } => {
                    match if_watch.deref_mut() {
                        // If we listen on all interfaces, wait for `if-watch` to be ready.
                        IfWatch::Pending(f) => match futures::ready!(f.poll_unpin(cx)) {
                            Ok(watcher) => {
                                *if_watch = Box::new(IfWatch::Ready(Box::new(watcher)));
                                continue;
                            }
                            Err(err) => {
                                *if_watch = Box::new(IfWatch::Pending(IfWatcher::new().boxed()));
                                return Poll::Ready(Some(Err(err)));
                            }
                        },
                        // Consume all events for up/down interface changes.
                        IfWatch::Ready(watcher) => {
                            if let Poll::Ready(ev) = watcher.poll_unpin(cx) {
                                match ev {
                                    Ok(event) => {
                                        return Poll::Ready(Some(Ok(event)));
                                    }
                                    Err(err) => {
                                        return Poll::Ready(Some(Err(err)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            break;
        }
        Poll::Pending
    }
}
