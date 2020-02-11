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

mod codec;

use std::{cmp, iter, mem, pin::Pin, task::Context, task::Poll};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::sync::Arc;
use std::task::Waker;
use bytes::Bytes;
use libp2p_core::{
    Endpoint,
    StreamMuxer,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
};
use log::{debug, trace};
use parking_lot::Mutex;
use fnv::FnvHashSet;
use futures::{prelude::*, future, ready, stream::Fuse};
use futures::task::{ArcWake, waker_ref};
use futures_codec::Framed;

/// Configuration for the multiplexer.
#[derive(Debug, Clone)]
pub struct MplexConfig {
    /// Maximum number of simultaneously-open substreams.
    max_substreams: usize,
    /// Maximum number of elements in the internal buffer.
    max_buffer_len: usize,
    /// Behaviour when the buffer size limit is reached.
    max_buffer_behaviour: MaxBufferBehaviour,
    /// When sending data, split it into frames whose maximum size is this value
    /// (max 1MByte, as per the Mplex spec).
    split_send_size: usize,
}

impl MplexConfig {
    /// Builds the default configuration.
    #[inline]
    pub fn new() -> MplexConfig {
        Default::default()
    }

    /// Sets the maximum number of simultaneously opened substreams, after which an error is
    /// generated and the connection closes.
    ///
    /// A limit is necessary in order to avoid DoS attacks.
    #[inline]
    pub fn max_substreams(&mut self, max: usize) -> &mut Self {
        self.max_substreams = max;
        self
    }

    /// Sets the maximum number of pending incoming messages.
    ///
    /// A limit is necessary in order to avoid DoS attacks.
    #[inline]
    pub fn max_buffer_len(&mut self, max: usize) -> &mut Self {
        self.max_buffer_len = max;
        self
    }

    /// Sets the behaviour when the maximum buffer length has been reached.
    ///
    /// See the documentation of `MaxBufferBehaviour`.
    #[inline]
    pub fn max_buffer_len_behaviour(&mut self, behaviour: MaxBufferBehaviour) -> &mut Self {
        self.max_buffer_behaviour = behaviour;
        self
    }

    /// Sets the frame size used when sending data. Capped at 1Mbyte as per the
    /// Mplex spec.
    pub fn split_send_size(&mut self, size: usize) -> &mut Self {
        let size = cmp::min(size, codec::MAX_FRAME_SIZE);
        self.split_send_size = size;
        self
    }

    #[inline]
    fn upgrade<C>(self, i: C) -> Multiplex<C>
    where
        C: AsyncRead + AsyncWrite + Unpin
    {
        let max_buffer_len = self.max_buffer_len;
        Multiplex {
            inner: Mutex::new(MultiplexInner {
                error: Ok(()),
                inner: Framed::new(i, codec::Codec::new()).fuse(),
                config: self,
                buffer: Vec::with_capacity(cmp::min(max_buffer_len, 512)),
                opened_substreams: Default::default(),
                next_outbound_stream_id: 0,
                notifier_read: Arc::new(Notifier {
                    to_wake: Mutex::new(Default::default()),
                }),
                notifier_write: Arc::new(Notifier {
                    to_wake: Mutex::new(Default::default()),
                }),
                is_shutdown: false,
                is_acknowledged: false,
            })
        }
    }
}

impl Default for MplexConfig {
    #[inline]
    fn default() -> MplexConfig {
        MplexConfig {
            max_substreams: 128,
            max_buffer_len: 4096,
            max_buffer_behaviour: MaxBufferBehaviour::CloseAll,
            split_send_size: 1024,
        }
    }
}

/// Behaviour when the maximum length of the buffer is reached.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MaxBufferBehaviour {
    /// Produce an error on all the substreams.
    CloseAll,
    /// No new message will be read from the underlying connection if the buffer is full.
    ///
    /// This can potentially introduce a deadlock if you are waiting for a message from a substream
    /// before processing the messages received on another substream.
    Block,
}

impl UpgradeInfo for MplexConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/mplex/6.7.0")
    }
}

impl<C> InboundUpgrade<C> for MplexConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Multiplex<C>;
    type Error = IoError;
    type Future = future::Ready<Result<Self::Output, IoError>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(self.upgrade(socket)))
    }
}

impl<C> OutboundUpgrade<C> for MplexConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Multiplex<C>;
    type Error = IoError;
    type Future = future::Ready<Result<Self::Output, IoError>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(self.upgrade(socket)))
    }
}

/// Multiplexer. Implements the `StreamMuxer` trait.
pub struct Multiplex<C> {
    inner: Mutex<MultiplexInner<C>>,
}

// Struct shared throughout the implementation.
struct MultiplexInner<C> {
    // Error that happened earlier. Should poison any attempt to use this `MultiplexError`.
    error: Result<(), IoError>,
    // Underlying stream.
    inner: Fuse<Framed<C, codec::Codec>>,
    /// The original configuration.
    config: MplexConfig,
    // Buffer of elements pulled from the stream but not processed yet.
    buffer: Vec<codec::Elem>,
    // List of Ids of opened substreams. Used to filter out messages that don't belong to any
    // substream. Note that this is handled exclusively by `next_match`.
    // The `Endpoint` value denotes who initiated the substream from our point of view
    // (see note [StreamId]).
    opened_substreams: FnvHashSet<(u32, Endpoint)>,
    // Id of the next outgoing substream.
    next_outbound_stream_id: u32,
    /// List of wakers to wake when a read event happens on the underlying stream.
    notifier_read: Arc<Notifier>,
    /// List of wakers to wake when a write event happens on the underlying stream.
    notifier_write: Arc<Notifier>,
    /// If true, the connection has been shut down. We need to be careful not to accidentally
    /// call `Sink::poll_complete` or `Sink::start_send` after `Sink::close`.
    is_shutdown: bool,
    /// If true, the remote has sent data to us.
    is_acknowledged: bool,
}

struct Notifier {
    /// List of wakers to wake.
    to_wake: Mutex<Vec<Waker>>,
}

impl Notifier {
    fn insert(&self, waker: &Waker) {
        let mut to_wake = self.to_wake.lock();
        if to_wake.iter().all(|w| !w.will_wake(waker)) {
            to_wake.push(waker.clone());
        }
    }
}

impl ArcWake for Notifier {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let wakers = mem::replace(&mut *arc_self.to_wake.lock(), Default::default());
        for waker in wakers {
            waker.wake();
        }
    }
}

// Note [StreamId]: mplex no longer partitions stream IDs into odd (for initiators) and
// even ones (for receivers). Streams are instead identified by a number and whether the flag
// is odd (for receivers) or even (for initiators). `Open` frames do not have a flag, but are
// sent unidirectional. As a consequence, we need to remember if the stream was initiated by us
// or remotely and we store the information from our point of view, i.e. receiving an `Open` frame
// is stored as `(<u32>, Listener)`, sending an `Open` frame as `(<u32>, Dialer)`. Receiving
// a `Data` frame with flag `MessageReceiver` (= 1) means that we initiated the stream, so the
// entry has been stored as `(<u32>, Dialer)`. So, when looking up streams based on frames
// received, we have to invert the `Endpoint`, except for `Open`.

/// Processes elements in `inner` until one matching `filter` is found.
///
/// If `Pending` is returned, the waker is kept and notified later, just like with any `Poll`.
/// `Ready(Ok())` is almost always returned. An error is returned if the stream is EOF.
fn next_match<C, F, O>(inner: &mut MultiplexInner<C>, cx: &mut Context, mut filter: F) -> Poll<Result<O, IoError>>
where C: AsyncRead + AsyncWrite + Unpin,
      F: FnMut(&codec::Elem) -> Option<O>,
{
    // If an error happened earlier, immediately return it.
    if let Err(ref err) = inner.error {
        return Poll::Ready(Err(IoError::new(err.kind(), err.to_string())));
    }

    if let Some((offset, out)) = inner.buffer.iter().enumerate().filter_map(|(n, v)| filter(v).map(|v| (n, v))).next() {
        // Found a matching entry in the existing buffer!

        // The buffer was full and no longer is, so let's notify everything.
        if inner.buffer.len() == inner.config.max_buffer_len {
            ArcWake::wake_by_ref(&inner.notifier_read);
        }

        inner.buffer.remove(offset);
        return Poll::Ready(Ok(out));
    }

    loop {
        // Check if we reached max buffer length first.
        debug_assert!(inner.buffer.len() <= inner.config.max_buffer_len);
        if inner.buffer.len() == inner.config.max_buffer_len {
            debug!("Reached mplex maximum buffer length");
            match inner.config.max_buffer_behaviour {
                MaxBufferBehaviour::CloseAll => {
                    inner.error = Err(IoError::new(IoErrorKind::Other, "reached maximum buffer length"));
                    return Poll::Ready(Err(IoError::new(IoErrorKind::Other, "reached maximum buffer length")));
                },
                MaxBufferBehaviour::Block => {
                    inner.notifier_read.insert(cx.waker());
                    return Poll::Pending
                },
            }
        }

        inner.notifier_read.insert(cx.waker());
        let elem = match Stream::poll_next(Pin::new(&mut inner.inner), &mut Context::from_waker(&waker_ref(&inner.notifier_read))) {
            Poll::Ready(Some(Ok(item))) => item,
            Poll::Ready(None) => return Poll::Ready(Err(IoErrorKind::BrokenPipe.into())),
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Err(err))) => {
                let err2 = IoError::new(err.kind(), err.to_string());
                inner.error = Err(err);
                return Poll::Ready(Err(err2));
            },
        };

        trace!("Received message: {:?}", elem);
        inner.is_acknowledged = true;

        // Handle substreams opening/closing.
        match elem {
            codec::Elem::Open { substream_id } => {
                if !inner.opened_substreams.insert((substream_id, Endpoint::Listener)) {
                    debug!("Received open message for substream {} which was already open", substream_id)
                }
            }
            codec::Elem::Close { substream_id, endpoint, .. } | codec::Elem::Reset { substream_id, endpoint, .. } => {
                inner.opened_substreams.remove(&(substream_id, !endpoint));
            }
            _ => ()
        }

        if let Some(out) = filter(&elem) {
            return Poll::Ready(Ok(out));
        } else {
            let endpoint = elem.endpoint().unwrap_or(Endpoint::Dialer);
            if inner.opened_substreams.contains(&(elem.substream_id(), !endpoint)) || elem.is_open_msg() {
                inner.buffer.push(elem);
            } else if !elem.is_close_or_reset_msg() {
                debug!("Ignored message {:?} because the substream wasn't open", elem);
            }
        }
    }
}

// Small convenience function that tries to write `elem` to the stream.
fn poll_send<C>(inner: &mut MultiplexInner<C>, cx: &mut Context, elem: codec::Elem) -> Poll<Result<(), IoError>>
where C: AsyncRead + AsyncWrite + Unpin
{
    if inner.is_shutdown {
        return Poll::Ready(Err(IoError::new(IoErrorKind::Other, "connection is shut down")))
    }

    inner.notifier_write.insert(cx.waker());

    match Sink::poll_ready(Pin::new(&mut inner.inner), &mut Context::from_waker(&waker_ref(&inner.notifier_write))) {
        Poll::Ready(Ok(())) => {
            match Sink::start_send(Pin::new(&mut inner.inner), elem) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(err) => Poll::Ready(Err(err))
            }
        },
        Poll::Pending => Poll::Pending,
        Poll::Ready(Err(err)) => Poll::Ready(Err(err))
    }
}

impl<C> StreamMuxer for Multiplex<C>
where C: AsyncRead + AsyncWrite + Unpin
{
    type Substream = Substream;
    type OutboundSubstream = OutboundSubstream;
    type Error = IoError;

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, IoError>> {
        let mut inner = self.inner.lock();

        if inner.opened_substreams.len() >= inner.config.max_substreams {
            debug!("Refused substream; reached maximum number of substreams {}", inner.config.max_substreams);
            return Poll::Ready(Err(IoError::new(IoErrorKind::ConnectionRefused,
                                    "exceeded maximum number of open substreams")));
        }

        let num = ready!(next_match(&mut inner, cx, |elem| {
            match elem {
                codec::Elem::Open { substream_id } => Some(*substream_id),
                _ => None,
            }
        }));

        let num = match num {
            Ok(n) => n,
            Err(err) => return Poll::Ready(Err(err)),
        };

        debug!("Successfully opened inbound substream {}", num);
        Poll::Ready(Ok(Substream {
            current_data: Bytes::new(),
            num,
            endpoint: Endpoint::Listener,
            local_open: true,
            remote_open: true,
        }))
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        let mut inner = self.inner.lock();

        // Assign a substream ID now.
        let substream_id = {
            let n = inner.next_outbound_stream_id;
            inner.next_outbound_stream_id = inner.next_outbound_stream_id.checked_add(1)
                .expect("Mplex substream ID overflowed");
            n
        };

        inner.opened_substreams.insert((substream_id, Endpoint::Dialer));

        OutboundSubstream {
            num: substream_id,
            state: OutboundSubstreamState::SendElem(codec::Elem::Open { substream_id }),
        }
    }

    fn poll_outbound(&self, cx: &mut Context, substream: &mut Self::OutboundSubstream) -> Poll<Result<Self::Substream, IoError>> {
        loop {
            let mut inner = self.inner.lock();

            let polling = match substream.state {
                OutboundSubstreamState::SendElem(ref elem) => {
                    poll_send(&mut inner, cx, elem.clone())
                },
                OutboundSubstreamState::Flush => {
                    if inner.is_shutdown {
                        return Poll::Ready(Err(IoError::new(IoErrorKind::Other, "connection is shut down")))
                    }
                    let inner = &mut *inner; // Avoids borrow errors
                    inner.notifier_write.insert(cx.waker());
                    Sink::poll_flush(Pin::new(&mut inner.inner), &mut Context::from_waker(&waker_ref(&inner.notifier_write)))
                },
                OutboundSubstreamState::Done => {
                    panic!("Polling outbound substream after it's been succesfully open");
                },
            };

            match polling {
                Poll::Ready(Ok(())) => (),
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => {
                    debug!("Failed to open outbound substream {}", substream.num);
                    inner.buffer.retain(|elem| {
                        elem.substream_id() != substream.num || elem.endpoint() == Some(Endpoint::Dialer)
                    });
                    return Poll::Ready(Err(err));
                },
            };

            drop(inner);

            // Going to next step.
            match substream.state {
                OutboundSubstreamState::SendElem(_) => {
                    substream.state = OutboundSubstreamState::Flush;
                },
                OutboundSubstreamState::Flush => {
                    debug!("Successfully opened outbound substream {}", substream.num);
                    substream.state = OutboundSubstreamState::Done;
                    return Poll::Ready(Ok(Substream {
                        num: substream.num,
                        current_data: Bytes::new(),
                        endpoint: Endpoint::Dialer,
                        local_open: true,
                        remote_open: true,
                    }));
                },
                OutboundSubstreamState::Done => unreachable!(),
            }
        }
    }

    #[inline]
    fn destroy_outbound(&self, _substream: Self::OutboundSubstream) {
        // Nothing to do.
    }

    fn read_substream(&self, cx: &mut Context, substream: &mut Self::Substream, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        loop {
            // First, transfer from `current_data`.
            if !substream.current_data.is_empty() {
                let len = cmp::min(substream.current_data.len(), buf.len());
                buf[..len].copy_from_slice(&substream.current_data.split_to(len));
                return Poll::Ready(Ok(len));
            }

            // If the remote writing side is closed, return EOF.
            if !substream.remote_open {
                return Poll::Ready(Ok(0));
            }

            // Try to find a packet of data in the buffer.
            let mut inner = self.inner.lock();
            let next_data_poll = next_match(&mut inner, cx, |elem| {
                match elem {
                    codec::Elem::Data { substream_id, endpoint, data, .. }
                        if *substream_id == substream.num && *endpoint != substream.endpoint => // see note [StreamId]
                    {
                        Some(Some(data.clone()))
                    }
                    codec::Elem::Close { substream_id, endpoint }
                        if *substream_id == substream.num && *endpoint != substream.endpoint => // see note [StreamId]
                    {
                        Some(None)
                    }
                    _ => None
                }
            });

            // We're in a loop, so all we need to do is set `substream.current_data` to the data we
            // just read and wait for the next iteration.
            match next_data_poll {
                Poll::Ready(Ok(Some(data))) => substream.current_data = data,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Ready(Ok(None)) => {
                    substream.remote_open = false;
                    return Poll::Ready(Ok(0));
                },
                Poll::Pending => {
                    // There was no data packet in the buffer about this substream; maybe it's
                    // because it has been closed.
                    if inner.opened_substreams.contains(&(substream.num, substream.endpoint)) {
                        return Poll::Pending
                    } else {
                        return Poll::Ready(Ok(0))
                    }
                },
            }
        }
    }

    fn write_substream(&self, cx: &mut Context, substream: &mut Self::Substream, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        if !substream.local_open {
            return Poll::Ready(Err(IoErrorKind::BrokenPipe.into()));
        }

        let mut inner = self.inner.lock();

        let to_write = cmp::min(buf.len(), inner.config.split_send_size);

        let elem = codec::Elem::Data {
            substream_id: substream.num,
            data: Bytes::copy_from_slice(&buf[..to_write]),
            endpoint: substream.endpoint,
        };

        match poll_send(&mut inner, cx, elem) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(to_write)),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn flush_substream(&self, cx: &mut Context, _substream: &mut Self::Substream) -> Poll<Result<(), IoError>> {
        let mut inner = self.inner.lock();
        if inner.is_shutdown {
            return Poll::Ready(Err(IoError::new(IoErrorKind::Other, "connection is shut down")))
        }

        let inner = &mut *inner; // Avoids borrow errors
        inner.notifier_write.insert(cx.waker());
        Sink::poll_flush(Pin::new(&mut inner.inner), &mut Context::from_waker(&waker_ref(&inner.notifier_write)))
    }

    fn shutdown_substream(&self, cx: &mut Context, sub: &mut Self::Substream) -> Poll<Result<(), IoError>> {
        if !sub.local_open {
            return Poll::Ready(Ok(()));
        }

        let elem = codec::Elem::Close {
            substream_id: sub.num,
            endpoint: sub.endpoint,
        };

        let mut inner = self.inner.lock();
        let result = poll_send(&mut inner, cx, elem);
        if let Poll::Ready(Ok(())) = result {
            sub.local_open = false;
        }
        result
    }

    fn destroy_substream(&self, sub: Self::Substream) {
        self.inner.lock().buffer.retain(|elem| {
            elem.substream_id() != sub.num || elem.endpoint() == Some(sub.endpoint)
        })
    }

    fn is_remote_acknowledged(&self) -> bool {
        self.inner.lock().is_acknowledged
    }

    #[inline]
    fn close(&self, cx: &mut Context) -> Poll<Result<(), IoError>> {
        let inner = &mut *self.inner.lock();
        inner.notifier_write.insert(cx.waker());
        match Sink::poll_close(Pin::new(&mut inner.inner), &mut Context::from_waker(&waker_ref(&inner.notifier_write))) {
            Poll::Ready(Ok(())) => {
                inner.is_shutdown = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }

    #[inline]
    fn flush_all(&self, cx: &mut Context) -> Poll<Result<(), IoError>> {
        let inner = &mut *self.inner.lock();
        if inner.is_shutdown {
            return Poll::Ready(Ok(()))
        }
        inner.notifier_write.insert(cx.waker());
        Sink::poll_flush(Pin::new(&mut inner.inner), &mut Context::from_waker(&waker_ref(&inner.notifier_write)))
    }
}

/// Active attempt to open an outbound substream.
pub struct OutboundSubstream {
    /// Substream number.
    num: u32,
    state: OutboundSubstreamState,
}

enum OutboundSubstreamState {
    /// We need to send `Elem` on the underlying stream.
    SendElem(codec::Elem),
    /// We need to flush the underlying stream.
    Flush,
    /// The substream is open and the `OutboundSubstream` is now useless.
    Done,
}

/// Active substream to the remote.
pub struct Substream {
    /// Substream number.
    num: u32,
    // Read buffer. Contains data read from `inner` but not yet dispatched by a call to `read()`.
    current_data: Bytes,
    endpoint: Endpoint,
    /// If true, our writing side is still open.
    local_open: bool,
    /// If true, the remote writing side is still open.
    remote_open: bool,
}
