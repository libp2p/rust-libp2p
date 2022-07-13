// Copyright 2022 Parity Technologies (UK) Ltd.
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

use if_watch::{IfEvent, IfWatcher};

use futures::{
    future::{BoxFuture, FutureExt},
    stream::Stream,
};

use std::{
    io::Result,
    net::IpAddr,
    ops::DerefMut,
    pin::Pin,
    task::{Context, Poll},
};

/// Watches for interface changes.
#[derive(Debug)]
pub enum InAddr {
    /// The socket accepts connections on a single interface.
    One { ip: Option<IpAddr> },
    /// The socket accepts connections on all interfaces.
    Any { if_watch: Box<IfWatch> },
}

impl InAddr {
    /// If ip is specified then only one `IfEvent::Up` with IpNet(ip)/32 will be generated.
    /// If ip is unspecified then `IfEvent::Up/Down` events will be generated for all interfaces.
    pub fn new(ip: IpAddr) -> Self {
        if ip.is_unspecified() {
            let watcher = IfWatch::Pending(IfWatcher::new().boxed());
            InAddr::Any {
                if_watch: Box::new(watcher),
            }
        } else {
            InAddr::One { ip: Some(ip) }
        }
    }
}

pub enum IfWatch {
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
        loop {
            match me {
                // If the listener is bound to a single interface, make sure the
                // address is reported once.
                InAddr::One { ip } => {
                    if let Some(ip) = ip.take() {
                        return Poll::Ready(Some(Ok(IfEvent::Up(ip.into()))));
                    }
                }
                InAddr::Any { if_watch } => {
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
