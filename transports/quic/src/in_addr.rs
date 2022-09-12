use if_watch::{IfEvent, IfWatcher};

use futures::stream::Stream;

use std::{
    io::Result,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll},
};

/// Watches for interface changes.
#[derive(Debug)]
pub enum InAddr {
    /// The socket accepts connections on a single interface.
    One { ip: Option<IpAddr> },
    /// The socket accepts connections on all interfaces.
    Any { if_watch: Box<IfWatcher> },
}

impl InAddr {
    /// If ip is specified then only one `IfEvent::Up` with IpNet(ip)/32 will be generated.
    /// If ip is unspecified then `IfEvent::Up/Down` events will be generated for all interfaces.
    pub fn new(ip: IpAddr) -> Result<Self> {
        let result = if ip.is_unspecified() {
            let watcher = IfWatcher::new()?;
            InAddr::Any {
                if_watch: Box::new(watcher),
            }
        } else {
            InAddr::One { ip: Some(ip) }
        };
        Ok(result)
    }
}

impl Stream for InAddr {
    type Item = Result<IfEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        match me {
            // If the listener is bound to a single interface, make sure the
            // address is reported once.
            InAddr::One { ip } => {
                if let Some(ip) = ip.take() {
                    return Poll::Ready(Some(Ok(IfEvent::Up(ip.into()))));
                }
            }
            InAddr::Any { if_watch } => {
                // Consume all events for up/down interface changes.
                if let Poll::Ready(ev) = if_watch.poll_if_event(cx) {
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
        Poll::Pending
    }
}
