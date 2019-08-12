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

use std::{cmp, iter, mem};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::sync::{atomic::AtomicUsize, atomic::Ordering, Arc};
use bytes::Bytes;
use libp2p_core::{
    Endpoint,
    StreamMuxer,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, Negotiated},
};
use log::{debug, trace};
use parking_lot::Mutex;
use fnv::{FnvHashMap, FnvHashSet};
use futures::{prelude::*, executor, future, stream::Fuse, task, task_local, try_ready};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

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
        C: AsyncRead + AsyncWrite
    {
        let max_buffer_len = self.max_buffer_len;
        Multiplex {
            inner: Mutex::new(MultiplexInner {
                error: Ok(()),
                inner: executor::spawn(Framed::new(i, codec::Codec::new()).fuse()),
                config: self,
                buffer: Vec::with_capacity(cmp::min(max_buffer_len, 512)),
                opened_substreams: Default::default(),
                next_outbound_stream_id: 0,
                notifier_read: Arc::new(Notifier {
                    to_notify: Mutex::new(Default::default()),
                }),
                notifier_write: Arc::new(Notifier {
                    to_notify: Mutex::new(Default::default()),
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
    C: AsyncRead + AsyncWrite,
{
    type Output = Multiplex<Negotiated<C>>;
    type Error = IoError;
    type Future = future::FutureResult<Self::Output, IoError>;

    fn upgrade_inbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ok(self.upgrade(socket))
    }
}

impl<C> OutboundUpgrade<C> for MplexConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = Multiplex<Negotiated<C>>;
    type Error = IoError;
    type Future = future::FutureResult<Self::Output, IoError>;

    fn upgrade_outbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ok(self.upgrade(socket))
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
    inner: executor::Spawn<Fuse<Framed<C, codec::Codec>>>,
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
    /// List of tasks to notify when a read event happens on the underlying stream.
    notifier_read: Arc<Notifier>,
    /// List of tasks to notify when a write event happens on the underlying stream.
    notifier_write: Arc<Notifier>,
    /// If true, the connection has been shut down. We need to be careful not to accidentally
    /// call `Sink::poll_complete` or `Sink::start_send` after `Sink::close`.
    is_shutdown: bool,
    /// If true, the remote has sent data to us.
    is_acknowledged: bool,
}

struct Notifier {
    /// List of tasks to notify.
    to_notify: Mutex<FnvHashMap<usize, task::Task>>,
}

impl executor::Notify for Notifier {
    fn notify(&self, _: usize) {
        let tasks = mem::replace(&mut *self.to_notify.lock(), Default::default());
        for (_, task) in tasks {
            task.notify();
        }
    }
}

// TODO: replace with another system
static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(0);
task_local!{
    static TASK_ID: usize = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
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
/// If `NotReady` is returned, the current task is scheduled for later, just like with any `Poll`.
/// `Ready(Some())` is almost always returned. An error is returned if the stream is EOF.
fn next_match<C, F, O>(inner: &mut MultiplexInner<C>, mut filter: F) -> Poll<O, IoError>
where C: AsyncRead + AsyncWrite,
      F: FnMut(&codec::Elem) -> Option<O>,
{
    // If an error happened earlier, immediately return it.
    if let Err(ref err) = inner.error {
        return Err(IoError::new(err.kind(), err.to_string()));
    }

    if let Some((offset, out)) = inner.buffer.iter().enumerate().filter_map(|(n, v)| filter(v).map(|v| (n, v))).next() {
        // The buffer was full and no longer is, so let's notify everything.
        if inner.buffer.len() == inner.config.max_buffer_len {
            executor::Notify::notify(&*inner.notifier_read, 0);
        }

        inner.buffer.remove(offset);
        return Ok(Async::Ready(out));
    }

    loop {
        // Check if we reached max buffer length first.
        debug_assert!(inner.buffer.len() <= inner.config.max_buffer_len);
        if inner.buffer.len() == inner.config.max_buffer_len {
            debug!("Reached mplex maximum buffer length");
            match inner.config.max_buffer_behaviour {
                MaxBufferBehaviour::CloseAll => {
                    inner.error = Err(IoError::new(IoErrorKind::Other, "reached maximum buffer length"));
                    return Err(IoError::new(IoErrorKind::Other, "reached maximum buffer length"));
                },
                MaxBufferBehaviour::Block => {
                    inner.notifier_read.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
                    return Ok(Async::NotReady);
                },
            }
        }

        inner.notifier_read.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
        let elem = match inner.inner.poll_stream_notify(&inner.notifier_read, 0) {
            Ok(Async::Ready(Some(item))) => item,
            Ok(Async::Ready(None)) => return Err(IoErrorKind::BrokenPipe.into()),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(err) => {
                let err2 = IoError::new(err.kind(), err.to_string());
                inner.error = Err(err);
                return Err(err2);
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
            return Ok(Async::Ready(out));
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
fn poll_send<C>(inner: &mut MultiplexInner<C>, elem: codec::Elem) -> Poll<(), IoError>
where C: AsyncRead + AsyncWrite
{
    if inner.is_shutdown {
        return Err(IoError::new(IoErrorKind::Other, "connection is shut down"))
    }
    inner.notifier_write.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
    match inner.inner.start_send_notify(elem, &inner.notifier_write, 0) {
        Ok(AsyncSink::Ready) => Ok(Async::Ready(())),
        Ok(AsyncSink::NotReady(_)) => Ok(Async::NotReady),
        Err(err) => Err(err)
    }
}

impl<C> StreamMuxer for Multiplex<C>
where C: AsyncRead + AsyncWrite
{
    type Substream = Substream;
    type OutboundSubstream = OutboundSubstream;
    type Error = IoError;

    fn poll_inbound(&self) -> Poll<Self::Substream, IoError> {
        let mut inner = self.inner.lock();

        if inner.opened_substreams.len() >= inner.config.max_substreams {
            debug!("Refused substream; reached maximum number of substreams {}", inner.config.max_substreams);
            return Err(IoError::new(IoErrorKind::ConnectionRefused,
                                    "exceeded maximum number of open substreams"));
        }

        let num = try_ready!(next_match(&mut inner, |elem| {
            match elem {
                codec::Elem::Open { substream_id } => Some(*substream_id),
                _ => None,
            }
        }));

        debug!("Successfully opened inbound substream {}", num);
        Ok(Async::Ready(Substream {
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

    fn poll_outbound(&self, substream: &mut Self::OutboundSubstream) -> Poll<Self::Substream, IoError> {
        loop {
            let mut inner = self.inner.lock();

            let polling = match substream.state {
                OutboundSubstreamState::SendElem(ref elem) => {
                    poll_send(&mut inner, elem.clone())
                },
                OutboundSubstreamState::Flush => {
                    if inner.is_shutdown {
                        return Err(IoError::new(IoErrorKind::Other, "connection is shut down"))
                    }
                    let inner = &mut *inner; // Avoids borrow errors
                    inner.notifier_write.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
                    inner.inner.poll_flush_notify(&inner.notifier_write, 0)
                },
                OutboundSubstreamState::Done => {
                    panic!("Polling outbound substream after it's been succesfully open");
                },
            };

            match polling {
                Ok(Async::Ready(())) => (),
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady)
                },
                Err(err) => {
                    debug!("Failed to open outbound substream {}", substream.num);
                    inner.buffer.retain(|elem| {
                        elem.substream_id() != substream.num || elem.endpoint() == Some(Endpoint::Dialer)
                    });
                    return Err(err)
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
                    return Ok(Async::Ready(Substream {
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

    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
        false
    }

    fn read_substream(&self, substream: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, IoError> {
        loop {
            // First, transfer from `current_data`.
            if !substream.current_data.is_empty() {
                let len = cmp::min(substream.current_data.len(), buf.len());
                buf[..len].copy_from_slice(&substream.current_data.split_to(len));
                return Ok(Async::Ready(len));
            }

            // If the remote writing side is closed, return EOF.
            if !substream.remote_open {
                return Ok(Async::Ready(0));
            }

            // Try to find a packet of data in the buffer.
            let mut inner = self.inner.lock();
            let next_data_poll = next_match(&mut inner, |elem| {
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
            match next_data_poll? {
                Async::Ready(Some(data)) => substream.current_data = data,
                Async::Ready(None) => {
                    substream.remote_open = false;
                    return Ok(Async::Ready(0));
                },
                Async::NotReady => {
                    // There was no data packet in the buffer about this substream; maybe it's
                    // because it has been closed.
                    if inner.opened_substreams.contains(&(substream.num, substream.endpoint)) {
                        return Ok(Async::NotReady)
                    } else {
                        return Ok(Async::Ready(0))
                    }
                },
            }
        }
    }

    fn write_substream(&self, substream: &mut Self::Substream, buf: &[u8]) -> Poll<usize, IoError> {
        if !substream.local_open {
            return Err(IoErrorKind::BrokenPipe.into());
        }

        let mut inner = self.inner.lock();

        let to_write = cmp::min(buf.len(), inner.config.split_send_size);

        let elem = codec::Elem::Data {
            substream_id: substream.num,
            data: From::from(&buf[..to_write]),
            endpoint: substream.endpoint,
        };

        match poll_send(&mut inner, elem)? {
            Async::Ready(()) => Ok(Async::Ready(to_write)),
            Async::NotReady => Ok(Async::NotReady)
        }
    }

    fn flush_substream(&self, _substream: &mut Self::Substream) -> Poll<(), IoError> {
        let mut inner = self.inner.lock();
        if inner.is_shutdown {
            return Err(IoError::new(IoErrorKind::Other, "connection is shut down"))
        }

        let inner = &mut *inner; // Avoids borrow errors
        inner.notifier_write.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
        inner.inner.poll_flush_notify(&inner.notifier_write, 0)
    }

    fn shutdown_substream(&self, sub: &mut Self::Substream) -> Poll<(), IoError> {
        if !sub.local_open {
            return Ok(Async::Ready(()));
        }

        let elem = codec::Elem::Close {
            substream_id: sub.num,
            endpoint: sub.endpoint,
        };

        let mut inner = self.inner.lock();
        let result = poll_send(&mut inner, elem);
        if let Ok(Async::Ready(())) = result {
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
    fn close(&self) -> Poll<(), IoError> {
        let inner = &mut *self.inner.lock();
        inner.notifier_write.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
        try_ready!(inner.inner.close_notify(&inner.notifier_write, 0));
        inner.is_shutdown = true;
        Ok(Async::Ready(()))
    }

    #[inline]
    fn flush_all(&self) -> Poll<(), IoError> {
        let inner = &mut *self.inner.lock();
        if inner.is_shutdown {
            return Ok(Async::Ready(()))
        }
        inner.notifier_write.to_notify.lock().insert(TASK_ID.with(|&t| t), task::current());
        inner.inner.poll_flush_notify(&inner.notifier_write, 0)
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
