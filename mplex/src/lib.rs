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

extern crate bytes;
extern crate fnv;
#[macro_use]
extern crate futures;
extern crate libp2p_core as core;
#[macro_use]
extern crate log;
extern crate parking_lot;
extern crate tokio_codec;
extern crate tokio_io;
extern crate unsigned_varint;

mod codec;

use std::{cmp, iter};
use std::io::{Read, Write, Error as IoError, ErrorKind as IoErrorKind};
use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering};
use bytes::Bytes;
use core::{ConnectionUpgrade, Endpoint, StreamMuxer};
use parking_lot::Mutex;
use fnv::{FnvHashMap, FnvHashSet};
use futures::prelude::*;
use futures::{future, stream::Fuse, task};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

// Maximum number of simultaneously-open substreams.
const MAX_SUBSTREAMS: usize = 1024;
// Maximum number of elements in the internal buffer.
const MAX_BUFFER_LEN: usize = 1024;

/// Configuration for the multiplexer.
#[derive(Debug, Clone, Default)]
pub struct MplexConfig;

impl MplexConfig {
    /// Builds the default configuration.
    #[inline]
    pub fn new() -> MplexConfig {
        Default::default()
    }
}

impl<C, Maf> ConnectionUpgrade<C, Maf> for MplexConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = Multiplex<C>;
    type MultiaddrFuture = Maf;
    type Future = future::FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;
    type UpgradeIdentifier = ();
    type NamesIter = iter::Once<(Bytes, ())>;

    #[inline]
    fn upgrade(self, i: C, _: (), endpoint: Endpoint, remote_addr: Maf) -> Self::Future {
        let out = Multiplex {
            inner: Arc::new(Mutex::new(MultiplexInner {
                error: Ok(()),
                inner: Framed::new(i, codec::Codec::new()).fuse(),
                buffer: Vec::with_capacity(32),
                opened_substreams: Default::default(),
                next_outbound_stream_id: if endpoint == Endpoint::Dialer { 0 } else { 1 },
                to_notify: Default::default(),
            }))
        };

        future::ok((out, remote_addr))
    }

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/mplex/6.7.0"), ()))
    }
}

/// Multiplexer. Implements the `StreamMuxer` trait.
pub struct Multiplex<C> {
    inner: Arc<Mutex<MultiplexInner<C>>>,
}

impl<C> Clone for Multiplex<C> {
    #[inline]
    fn clone(&self) -> Self {
        Multiplex {
            inner: self.inner.clone(),
        }
    }
}

// Struct shared throughout the implementation.
struct MultiplexInner<C> {
    // Errored that happend earlier. Should poison any attempt to use this `MultiplexError`.
    error: Result<(), IoError>,
    // Underlying stream.
    inner: Fuse<Framed<C, codec::Codec>>,
    // Buffer of elements pulled from the stream but not processed yet.
    buffer: Vec<codec::Elem>,
    // List of Ids of opened substreams. Used to filter out messages that don't belong to any
    // substream. Note that this is handled exclusively by `next_match`.
    opened_substreams: FnvHashSet<u32>,
    // Id of the next outgoing substream. Should always increase by two.
    next_outbound_stream_id: u32,
    // List of tasks to notify when a new element is inserted in `buffer` or an error happens.
    to_notify: FnvHashMap<usize, task::Task>,
}

/// Processes elements in `inner` until one matching `filter` is found.
///
/// If `NotReady` is returned, the current task is scheduled for later, just like with any `Poll`.
/// `Ready(Some())` is almost always returned. `Ready(None)` is returned if the stream is EOF.
fn next_match<C, F, O>(inner: &mut MultiplexInner<C>, mut filter: F) -> Poll<Option<O>, IoError>
where C: AsyncRead + AsyncWrite,
      F: FnMut(&codec::Elem) -> Option<O>,
{
    // If an error happened earlier, immediately return it.
    if let Err(ref err) = inner.error {
        return Err(IoError::new(err.kind(), err.to_string()));
    }

    if let Some((offset, out)) = inner.buffer.iter().enumerate().filter_map(|(n, v)| filter(v).map(|v| (n, v))).next() {
        inner.buffer.remove(offset);
        return Ok(Async::Ready(Some(out)));
    }

    loop {
        let elem = match inner.inner.poll() {
            Ok(Async::Ready(item)) => item,
            Ok(Async::NotReady) => {
                static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(0);
                task_local!{
                    static TASK_ID: usize = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
                }
                inner.to_notify.insert(TASK_ID.with(|&t| t), task::current());
                return Ok(Async::NotReady);
            },
            Err(err) => {
                let err2 = IoError::new(err.kind(), err.to_string());
                inner.error = Err(err);
                for task in inner.to_notify.drain() {
                    task.1.notify();
                }
                return Err(err2);
            },
        };

        if let Some(elem) = elem {
            trace!("Received message: {:?}", elem);

            // Handle substreams opening/closing.
            match elem {
                codec::Elem::Open { substream_id } => {
                    if (substream_id % 2) == (inner.next_outbound_stream_id % 2) {
                        inner.error = Err(IoError::new(IoErrorKind::Other, "invalid substream id opened"));
                        for task in inner.to_notify.drain() {
                            task.1.notify();
                        }
                        return Err(IoError::new(IoErrorKind::Other, "invalid substream id opened"));
                    }

                    if !inner.opened_substreams.insert(substream_id) {
                        debug!("Received open message for substream {} which was already open", substream_id)
                    }
                },
                codec::Elem::Close { substream_id, .. } | codec::Elem::Reset { substream_id, .. } => {
                    inner.opened_substreams.remove(&substream_id);
                },
                _ => ()
            }

            if let Some(out) = filter(&elem) {
                return Ok(Async::Ready(Some(out)));
            } else {
                if inner.buffer.len() >= MAX_BUFFER_LEN {
                    debug!("Reached mplex maximum buffer length");
                    inner.error = Err(IoError::new(IoErrorKind::Other, "reached maximum buffer length"));
                    for task in inner.to_notify.drain() {
                        task.1.notify();
                    }
                    return Err(IoError::new(IoErrorKind::Other, "reached maximum buffer length"));
                }

                if inner.opened_substreams.contains(&elem.substream_id()) || elem.is_open_msg() {
                    inner.buffer.push(elem);
                    for task in inner.to_notify.drain() {
                        task.1.notify();
                    }
                } else if !elem.is_close_or_reset_msg() {
                    debug!("Ignored message {:?} because the substream wasn't open", elem);
                }
            }
        } else {
            return Ok(Async::Ready(None));
        }
    }
}

// Small convenience function that tries to write `elem` to the stream.
fn poll_send<C>(inner: &mut MultiplexInner<C>, elem: codec::Elem) -> Poll<(), IoError>
where C: AsyncRead + AsyncWrite
{
    match inner.inner.start_send(elem) {
        Ok(AsyncSink::Ready) => {
            Ok(Async::Ready(()))
        },
        Ok(AsyncSink::NotReady(_)) => {
            Ok(Async::NotReady)
        },
        Err(err) => Err(err)
    }
}

impl<C> StreamMuxer for Multiplex<C>
where C: AsyncRead + AsyncWrite + 'static       // TODO: 'static :-/
{
    type Substream = Substream<C>;
    type InboundSubstream = InboundSubstream<C>;
    type OutboundSubstream = Box<Future<Item = Option<Self::Substream>, Error = IoError> + 'static>;

    #[inline]
    fn inbound(self) -> Self::InboundSubstream {
        InboundSubstream { inner: self.inner }
    }

    #[inline]
    fn outbound(self) -> Self::OutboundSubstream {
        let mut inner = self.inner.lock();

        // Assign a substream ID now.
        let substream_id = {
            let n = inner.next_outbound_stream_id;
            inner.next_outbound_stream_id += 2;
            n
        };

        // We use an RAII guard, so that we close the substream in case of an error.
        struct OpenedSubstreamGuard<C>(Option<Arc<Mutex<MultiplexInner<C>>>>, u32);
        impl<C> Drop for OpenedSubstreamGuard<C> {
            fn drop(&mut self) {
                if let Some(inner) = self.0.take() {
                    debug!("Failed to open outbound substream {}", self.1);
                    inner.lock().buffer.retain(|elem| elem.substream_id() != self.1);
                }
            }
        }
        inner.opened_substreams.insert(substream_id);
        let mut guard = OpenedSubstreamGuard(Some(self.inner.clone()), substream_id);

        // We send `Open { substream_id }`, then flush, then only produce the substream.
        let future = {
            future::poll_fn({
                let inner = self.inner.clone();
                move || {
                    let elem = codec::Elem::Open { substream_id };
                    poll_send(&mut inner.lock(), elem)
                }
            }).and_then({
                let inner = self.inner.clone();
                move |()| {
                    future::poll_fn(move || inner.lock().inner.poll_complete())
                }
            }).map(move |()| {
                debug!("Successfully opened outbound substream {}", substream_id);
                Some(Substream {
                    inner: guard.0.take().unwrap(),
                    num: substream_id,
                    current_data: Bytes::new(),
                    endpoint: Endpoint::Dialer,
                })
            })
        };

        Box::new(future) as Box<_>
    }
}

/// Future to the next incoming substream.
pub struct InboundSubstream<C> {
    inner: Arc<Mutex<MultiplexInner<C>>>,
}

impl<C> Future for InboundSubstream<C>
where C: AsyncRead + AsyncWrite
{
    type Item = Option<Substream<C>>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut inner = self.inner.lock();

        if inner.opened_substreams.len() >= MAX_SUBSTREAMS {
            debug!("Refused substream ; reached maximum number of substreams {}", MAX_SUBSTREAMS);
            return Err(IoError::new(IoErrorKind::ConnectionRefused,
                                    "exceeded maximum number of open substreams"));
        }

        let num = try_ready!(next_match(&mut inner, |elem| {
            match elem {
                codec::Elem::Open { substream_id } => Some(*substream_id),
                _ => None,
            }
        }));

        if let Some(num) = num {
            debug!("Successfully opened inbound substream {}", num);
            Ok(Async::Ready(Some(Substream {
                inner: self.inner.clone(),
                current_data: Bytes::new(),
                num,
                endpoint: Endpoint::Listener,
            })))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

/// Active substream to the remote. Implements `AsyncRead` and `AsyncWrite`.
pub struct Substream<C>
where C: AsyncRead + AsyncWrite
{
    inner: Arc<Mutex<MultiplexInner<C>>>,
    num: u32,
    // Read buffer. Contains data read from `inner` but not yet dispatched by a call to `read()`.
    current_data: Bytes,
    endpoint: Endpoint,
}

impl<C> Read for Substream<C>
where C: AsyncRead + AsyncWrite
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        loop {
            // First, transfer from `current_data`.
            if self.current_data.len() != 0 {
                let len = cmp::min(self.current_data.len(), buf.len());
                buf[..len].copy_from_slice(&self.current_data.split_to(len));
                return Ok(len);
            }

            // Try to find a packet of data in the buffer.
            let mut inner = self.inner.lock();
            let next_data_poll = next_match(&mut inner, |elem| {
                match elem {
                    &codec::Elem::Data { ref substream_id, ref data, .. } if *substream_id == self.num => {     // TODO: check endpoint?
                        Some(data.clone())
                    },
                    _ => None,
                }
            });

            // We're in a loop, so all we need to do is set `self.current_data` to the data we
            // just read and wait for the next iteration.
            match next_data_poll {
                Ok(Async::Ready(Some(data))) => self.current_data = data.freeze(),
                Ok(Async::Ready(None)) => return Ok(0),
                Ok(Async::NotReady) => {
                    // There was no data packet in the buffer about this substream ; maybe it's
                    // because it has been closed.
                    if inner.opened_substreams.contains(&self.num) {
                        return Err(IoErrorKind::WouldBlock.into());
                    } else {
                        return Ok(0);
                    }
                },
                Err(err) => return Err(err),
            }
        }
    }
}

impl<C> AsyncRead for Substream<C>
where C: AsyncRead + AsyncWrite
{
}

impl<C> Write for Substream<C>
where C: AsyncRead + AsyncWrite
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let elem = codec::Elem::Data {
            substream_id: self.num,
            data: From::from(buf),
            endpoint: self.endpoint,
        };

        let mut inner = self.inner.lock();
        match poll_send(&mut inner, elem) {
            Ok(Async::Ready(())) => Ok(buf.len()),
            Ok(Async::NotReady) => Err(IoErrorKind::WouldBlock.into()),
            Err(err) => Err(err),
        }
    }

    fn flush(&mut self) -> Result<(), IoError> {
        let mut inner = self.inner.lock();
        match inner.inner.poll_complete() {
            Ok(Async::Ready(())) => Ok(()),
            Ok(Async::NotReady) => Err(IoErrorKind::WouldBlock.into()),
            Err(err) => Err(err),
        }
    }
}

impl<C> AsyncWrite for Substream<C>
where C: AsyncRead + AsyncWrite
{
    fn shutdown(&mut self) -> Poll<(), IoError> {
        let elem = codec::Elem::Reset {
            substream_id: self.num,
            endpoint: self.endpoint,
        };

        let mut inner = self.inner.lock();
        poll_send(&mut inner, elem)
    }
}

impl<C> Drop for Substream<C>
where C: AsyncRead + AsyncWrite
{
    fn drop(&mut self) {
        let _ = self.shutdown();        // TODO: this doesn't necessarily send the close message
        self.inner.lock().buffer.retain(|elem| elem.substream_id() != self.num);
    }
}
