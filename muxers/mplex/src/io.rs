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

use bytes::Bytes;
use crate::{MplexConfig, MaxBufferBehaviour};
use crate::codec::{Codec, Frame, LocalStreamId, RemoteStreamId};
use log::{debug, trace};
use fnv::FnvHashMap;
use futures::{prelude::*, ready, stream::Fuse};
use futures::task::{ArcWake, waker_ref, WakerRef};
use futures_codec::Framed;
use parking_lot::Mutex;
use std::collections::hash_map::Entry;
use std::{cmp, io, mem, sync::Arc, task::{Context, Poll, Waker}};

pub use std::io::{Result, Error, ErrorKind};

/// A multiplexed I/O stream.
pub struct Multiplexed<C> {
    /// The current operating status.
    status: Status,
    /// The underlying I/O stream.
    io: Fuse<Framed<C, Codec>>,
    /// The configuration.
    config: MplexConfig,
    /// Buffer of received frames that have not yet been consumed.
    buffer: Vec<Frame<RemoteStreamId>>,
    /// Whether a flush is pending before reading frames can proceed.
    pending_flush: bool,
    /// Streams for which a `Reset` frame should be sent.
    pending_reset: Vec<LocalStreamId>,
    /// The substreams that are considered at least half-open.
    open_substreams: FnvHashMap<LocalStreamId, SubstreamState>,
    /// The ID for the next outbound substream.
    next_outbound_stream_id: LocalStreamId,
    /// Registry of wakers for pending tasks interested in reading.
    notifier_read: Arc<NotifierRead>,
    /// Registry of wakers for pending tasks interested in writing.
    notifier_write: Arc<NotifierWrite>,
    /// Registry of wakers for pending tasks interested in opening
    /// an outbound substream, when the configured limit is reached.
    notifier_open: Arc<NotifierOpen>,
}

/// The operation status of a `Multiplexed` I/O stream.
#[derive(Debug)]
enum Status {
    /// The stream is considered open and healthy.
    Ok,
    /// The stream has been actively closed.
    Closed,
    /// The stream has encountered a fatal error.
    Err(io::Error),
}

impl<C> Multiplexed<C>
where
    C: AsyncRead + AsyncWrite + Unpin
{
    /// Creates a new multiplexed I/O stream.
    pub fn new(io: C, config: MplexConfig) -> Self {
        let max_buffer_len = config.max_buffer_len;
        Multiplexed {
            config,
            status: Status::Ok,
            io: Framed::new(io, Codec::new()).fuse(),
            buffer: Vec::with_capacity(cmp::min(max_buffer_len, 512)),
            open_substreams: Default::default(),
            pending_flush: false,
            pending_reset: Vec::new(),
            next_outbound_stream_id: LocalStreamId::dialer(0),
            notifier_read: Arc::new(NotifierRead {
                pending: Mutex::new(Default::default()),
            }),
            notifier_write: Arc::new(NotifierWrite {
                pending: Mutex::new(Default::default()),
            }),
            notifier_open: Arc::new(NotifierOpen {
                pending: Mutex::new(Default::default())
            })
        }
    }

    /// Flushes the underlying I/O stream.
    pub fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &self.status {
            Status::Closed => return Poll::Ready(Ok(())),
            Status::Err(e) => return Poll::Ready(Err(io::Error::new(e.kind(), e.to_string()))),
            Status::Ok => {}
        }

        // Send any pending reset frames.
        ready!(self.send_pending_reset(cx))?;

        // Flush the underlying I/O stream.
        let waker = NotifierWrite::register(&self.notifier_write, cx.waker());
        match ready!(self.io.poll_flush_unpin(&mut Context::from_waker(&waker))) {
            Err(e) => self.on_error(e),
            Ok(()) => {
                self.pending_flush = false;
                Poll::Ready(Ok(()))
            }
        }
    }

    /// Closes the underlying I/O stream.
    ///
    /// > **Note**: No `Close` or `Reset` frames are sent on open substreams
    /// > before closing the underlying connection. However, the connection
    /// > close implies a flush of any frames already sent.
    pub fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &self.status {
            Status::Closed => return Poll::Ready(Ok(())),
            Status::Err(e) => return Poll::Ready(Err(io::Error::new(e.kind(), e.to_string()))),
            Status::Ok => {}
        }

        // Note: We do not make the effort to send pending `Reset` frames
        // here, we only close (and thus flush) the underlying I/O stream.

        let waker = NotifierWrite::register(&self.notifier_write, cx.waker());
        match self.io.poll_close_unpin(&mut Context::from_waker(&waker)) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => self.on_error(e),
            Poll::Ready(Ok(())) => {
                self.pending_reset = Vec::new();
                // We do not support read-after-close on the underlying
                // I/O stream, hence clearing the buffer and substreams.
                self.buffer = Vec::new();
                self.open_substreams = Default::default();
                self.status = Status::Closed;
                Poll::Ready(Ok(()))
            }
        }
    }

    /// Waits for a new inbound substream, returning the corresponding `LocalStreamId`.
    pub fn poll_next_stream(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<LocalStreamId>> {
        self.guard_ok()?;

        // Wait for the next inbound `Open` frame.
        let stream_id = ready!(self.poll_recv_frame(cx, None, |frame| match frame {
            Frame::Open { stream_id } => Some(stream_id.into_local()),
            _ => None,
        }))?.expect("Unexpected end of substream.");

        debug!("New inbound substream: {:?}", stream_id);

        Poll::Ready(Ok(stream_id))
    }

    /// Creates a new (outbound) substream, returning the allocated stream ID.
    pub fn poll_open_stream(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<LocalStreamId>> {
        self.guard_ok()?;

        // Check the stream limits.
        if self.open_substreams.len() >= self.config.max_substreams {
            debug!("Maximum number of substreams reached: {}", self.config.max_substreams);
            let _ = NotifierOpen::register(&self.notifier_open, cx.waker());
            return Poll::Pending
        }

        // Send the `Open` frame.
        let waker = NotifierWrite::register(&self.notifier_write, cx.waker());
        match ready!(self.io.poll_ready_unpin(&mut Context::from_waker(&waker))) {
            Ok(()) => {
                let stream_id = self.next_outbound_stream_id();
                let frame = Frame::Open { stream_id };
                match self.io.start_send_unpin(frame) {
                    Ok(()) => {
                        self.open_substreams.insert(stream_id, SubstreamState::Open);
                        // The flush is delayed and the `Open` frame may be sent
                        // together with other frames in the same transport packet.
                        self.pending_flush = true;
                        Poll::Ready(Ok(stream_id))
                    }
                    Err(e) => self.on_error(e),
                }
            },
            Err(e) => self.on_error(e)
        }
    }


    /// Resets an open substream, immediately closing it both for reading and writing.
    ///
    /// As opposed to a regular close, resetting a stream does not give the
    /// remote the opportunity to cleanly finish writing.
    pub fn reset_stream(&mut self, id: LocalStreamId) {
        let at_limit = self.open_substreams.len() >= self.config.max_substreams;
        if self.open_substreams.remove(&id).is_some() {
            if let Err(e) = self.guard_ok() {
                log::trace!("Cannot reset stream: {:?}", e);
                return
            }
            self.buffer.retain(|elem| elem.local_id() != id);
            if at_limit && self.open_substreams.len() < self.config.max_substreams {
                // Notify tasks that had interest in opening a substream.
                ArcWake::wake_by_ref(&self.notifier_open);
            }
            if self.pending_reset.len() >= MAX_PENDING_RESETS {
                log::debug!("Too many pending resets. Ignoring reset for {:?}", id);
                return
            }
            log::debug!("Scheduling reset for stream {:?}", id);
            self.pending_reset.push(id);
        }
    }

    /// Writes data to a substream.
    pub fn poll_write_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId, buf: &[u8])
        -> Poll<io::Result<usize>>
    {
        self.guard_ok()?;

        // Check if the stream is open for writing.
        match self.open_substreams.get(&id) {
            None => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            Some(SubstreamState::SendClosed) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
            _ => {}
        }

        // Determine the size of the frame to send.
        let frame_len = cmp::min(buf.len(), self.config.split_send_size);

        // Send the data frame.
        ready!(self.poll_send_frame(cx, || {
            let data = Bytes::copy_from_slice(&buf[.. frame_len]);
            Frame::Data { stream_id: id, data }
        }))?;

        Poll::Ready(Ok(frame_len))
    }

    /// Reads data from a substream.
    pub fn poll_read_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId)
        -> Poll<io::Result<Option<Bytes>>>
    {
        self.guard_ok()?;

        self.poll_recv_frame(cx, Some(id), |frame| match frame {
            Frame::Data { data, .. } => Some(data.clone()),
            _ => None
        })
    }

    /// Flushes a substream.
    ///
    /// > **Note**: This is equivalent to `poll_flush()`, i.e. to flushing
    /// > all substreams, except that this operation is an error on if
    /// > the underlying I/O stream is already closed.
    pub fn poll_flush_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId)
        -> Poll<io::Result<()>>
    {
        self.guard_ok()?;

        ready!(self.poll_flush(cx))?;
        trace!("Flushed substream {:?}", id);

        Poll::Ready(Ok(()))
    }

    /// Closes a stream for writing.
    ///
    /// > **Note**: As opposed to `poll_close()`, a flush it not implied.
    pub fn poll_close_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId)
        -> Poll<io::Result<()>>
    {
        self.guard_ok()?;

        let at_limit = self.open_substreams.len() >= self.config.max_substreams;
        match self.open_substreams.get(&id) {
            None | Some(SubstreamState::SendClosed) => Poll::Ready(Ok(())),
            Some(&state) => {
                ready!(self.poll_send_frame(cx, || Frame::Close { stream_id: id }))?;
                if state == SubstreamState::Open {
                    debug!("Closed substream {:?} (half-close)", id);
                    self.open_substreams.insert(id, SubstreamState::SendClosed);
                } else if state == SubstreamState::RecvClosed {
                    debug!("Closed substream {:?}", id);
                    self.open_substreams.remove(&id);
                    if at_limit && self.open_substreams.len() < self.config.max_substreams {
                        ArcWake::wake_by_ref(&self.notifier_open);
                    }
                }
                Poll::Ready(Ok(()))
            }
        }
    }

    /// Sends a (lazily constructed) mplex frame on the underlying I/O stream.
    ///
    /// The frame is only constructed if the underlying sink is ready to
    /// send another frame.
    fn poll_send_frame<F>(&mut self, cx: &mut Context<'_>, frame: F)
        -> Poll<io::Result<()>>
    where
        F: FnOnce() -> Frame<LocalStreamId>
    {
        let waker = NotifierWrite::register(&self.notifier_write, cx.waker());
        match ready!(self.io.poll_ready_unpin(&mut Context::from_waker(&waker))) {
            Ok(()) => {
                let frame = frame();
                trace!("Sending {:?}", frame);
                match self.io.start_send_unpin(frame) {
                    Ok(()) => Poll::Ready(Ok(())),
                    Err(e) => self.on_error(e)
                }
            },
            Err(e) => self.on_error(e)
        }
    }

    /// Polls the underlying I/O stream for the next frame chosen
    /// by the given selection function.
    ///
    /// Returns `Pending` if no frame satisfying the selection function
    /// is available, but the stream or substream is still open or
    /// half-closed (i.e. readable). All skipped frames are buffered and
    /// tasks that previously registered interest in the streams for which
    /// the skipped frames were received are notified.
    ///
    /// If `Some` stream ID is given, `Ok(None)` signals EOF for
    /// that substream. If `None` is given for the stream ID,
    /// `Ok(None)` is never returned. If the underlying connection
    /// encounters EOF, a corresponding error is returned.
    fn poll_recv_frame<F, O>(
        &mut self,
        cx: &mut Context<'_>,
        stream_id: Option<LocalStreamId>,
        mut select: F,
    ) -> Poll<io::Result<Option<O>>>
    where
        F: FnMut(&Frame<RemoteStreamId>) -> Option<O>,
    {
        // Try to send pending reset frames, if there are any, without blocking,
        if let Poll::Ready(Err(e)) = self.send_pending_reset(cx) {
            return Poll::Ready(Err(e))
        }

        // Perform any pending flush before reading.
        if self.pending_flush {
            trace!("Executing pending flush.");
            ready!(self.poll_flush(cx))?;
            debug_assert!(!self.pending_flush);
        }

        // Refine the selection function such that the given function
        // only gets frames for the targeted stream.
        let mut select = |f: &Frame<RemoteStreamId>|
            if stream_id.map_or(true, |id| id == f.local_id()) {
                select(f)
            } else {
                None
            };

        // Try to read from the buffer first.
        if let Some((pos, out)) = self.buffer.iter()
            .enumerate()
            .find_map(|(pos, frame)| select(frame).map(|out| (pos, out)))
        {
            // The buffer was full and no longer is, so notify all pending readers.
            if self.buffer.len() == self.config.max_buffer_len {
                ArcWake::wake_by_ref(&self.notifier_read);
            }
            self.buffer.remove(pos);
            return Poll::Ready(Ok(Some(out)));
        }

        loop {
            // Check if the targeted substream (if any) reached EOF.
            if let Some(id) = &stream_id {
                match self.open_substreams.get(id) {
                    Some(SubstreamState::RecvClosed) | None => return Poll::Ready(Ok(None)),
                    _ => {}
                }
            }

            // Check if the inbound frame buffer is full.
            debug_assert!(self.buffer.len() <= self.config.max_buffer_len);
            if self.buffer.len() == self.config.max_buffer_len {
                debug!("Frame buffer full ({} frames).", self.buffer.len());
                match self.config.max_buffer_behaviour {
                    MaxBufferBehaviour::CloseAll => {
                        return self.on_error(io::Error::new(io::ErrorKind::Other,
                            format!("Frame buffer full ({} frames).", self.buffer.len())))
                    },
                    MaxBufferBehaviour::Block => {
                        // If there are any pending tasks for frames in the buffer,
                        // use this opportunity to try to wake one of them.
                        let mut woken = false;
                        for frame in self.buffer.iter() {
                            woken = self.notifier_read.wake_by_id(frame.local_id());
                            if woken {
                                // The current task is still interested in another frame,
                                // so we register it for a wakeup some time after the
                                // already `woken` task.
                                let _ = NotifierRead::register(&self.notifier_read, cx.waker(), stream_id);
                                break
                            }
                        }
                        if !woken {
                            // No task was woken, thus the current task _must_ poll
                            // again to guarantee (an attempt at) making progress.
                            cx.waker().clone().wake();
                        }
                        return Poll::Pending
                    },
                }
            }

            // Try to read another frame from the underlying I/O stream.
            let waker = NotifierRead::register(&self.notifier_read, cx.waker(), stream_id);
            let frame = match ready!(self.io.poll_next_unpin(&mut Context::from_waker(&waker))) {
                Some(Ok(frame)) => frame,
                Some(Err(e)) => return self.on_error(e),
                None => return self.on_error(io::ErrorKind::UnexpectedEof.into())
            };

            trace!("Received: {:?}", frame);

            // Handle Open/Close/Reset frames.
            match frame {
                Frame::Open { stream_id } => {
                    let id = stream_id.into_local();

                    if self.open_substreams.contains_key(&id) {
                        debug!("Ignoring received `Open` Frame for open substream: {:?}", id);
                        continue
                    }

                    if self.open_substreams.len() >= self.config.max_substreams {
                        debug!("Maximum number of substreams exceeded: {}", self.config.max_substreams);
                        if self.pending_reset.len() >= MAX_PENDING_RESETS {
                            return self.on_error(io::Error::new(io::ErrorKind::Other,
                                "Too many pending stream resets."))
                        }
                        self.pending_reset.push(stream_id.into_local());
                        continue
                    }

                    self.open_substreams.insert(id, SubstreamState::Open);
                }
                Frame::Close { stream_id } => {
                    let at_limit = self.open_substreams.len() >= self.config.max_substreams;
                    if let Entry::Occupied(mut e) = self.open_substreams.entry(stream_id.into_local()) {
                        match e.get() {
                            SubstreamState::RecvClosed => {
                                trace!("Received redundant `Close` frame.");
                            },
                            SubstreamState::SendClosed => {
                                debug!("Substream closed by remote: {:?}", stream_id);
                                e.remove();
                                if at_limit && self.open_substreams.len() < self.config.max_substreams {
                                    ArcWake::wake_by_ref(&self.notifier_open);
                                }
                            },
                            SubstreamState::Open => {
                                debug!("Substream half-closed by remote: {:?}", stream_id);
                                e.insert(SubstreamState::RecvClosed);
                            },
                        }
                    }
                }
                Frame::Reset { stream_id } => {
                    let at_limit = self.open_substreams.len() >= self.config.max_substreams;
                    if let Some(state) = self.open_substreams.remove(&stream_id.into_local()) {
                        debug!("Substream {:?} in state {:?} reset by remote", stream_id, state);
                        if at_limit && self.open_substreams.len() < self.config.max_substreams {
                            ArcWake::wake_by_ref(&self.notifier_open);
                        }
                    } else {
                        trace!("Received reset for unknown stream: {:?}", stream_id);
                    }
                }
                _ => ()
            }

            if let Some(frame) = select(&frame) {
                // The caller is interested in this frame.
                return Poll::Ready(Ok(Some(frame)));
            } else {
                // The caller is not interested in this frame, so we need to buffer
                // it and wake those tasks who are interested.
                let id = frame.local_id();
                if self.can_read(&id) || frame.is_open() {
                    trace!("Buffering frame: {:?} (total: {})", frame, self.buffer.len() + 1);
                    self.buffer.push(frame);
                    self.notifier_read.wake_by_id(id);
                } else if frame.is_data() {
                    debug!("Dropping {:?} for closed or unknown substream: {:?}", frame, id);
                }
            }
        }
    }

    /// Generates the next outbound stream ID.
    fn next_outbound_stream_id(&mut self) -> LocalStreamId {
        let id = self.next_outbound_stream_id;
        self.next_outbound_stream_id = self.next_outbound_stream_id.next();
        id
    }

    /// Checks whether a substream is open for reading.
    fn can_read(&self, id: &LocalStreamId) -> bool {
        match self.open_substreams.get(id) {
            Some(SubstreamState::Open) | Some(SubstreamState::SendClosed) => true,
            _ => false,
        }
    }

    /// Sends pending `Reset` frames, without flushing.
    fn send_pending_reset(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while let Some(id) = self.pending_reset.pop() {
            if self.poll_send_frame(cx, || Frame::Reset { stream_id: id })?.is_pending() {
                self.pending_reset.push(id);
                return Poll::Pending
            }
        }

        Poll::Ready(Ok(()))
    }

    /// Records a fatal error for the multiplexed I/O stream.
    fn on_error<T>(&mut self, e: io::Error) -> Poll<io::Result<T>> {
        self.status = Status::Err(io::Error::new(e.kind(), e.to_string()));
        self.pending_reset =  Vec::new();
        self.open_substreams = Default::default();
        self.buffer = Default::default();
        Poll::Ready(Err(e))
    }

    /// Checks that the multiplexed stream has status `Ok`,
    /// i.e. is not closed and did not encounter a fatal error.
    fn guard_ok(&self) -> io::Result<()> {
        match &self.status {
            Status::Closed => Err(io::Error::new(io::ErrorKind::Other, "Connection is closed")),
            Status::Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            Status::Ok => Ok(())
        }
    }
}

/// The operating states of a substream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SubstreamState {
    /// An `Open` frame has been received or sent.
    Open,
    /// A `Close` frame has been sent, but the stream is still open
    /// for reading (half-close).
    SendClosed,
    /// A `Close` frame has been received but the stream is still
    /// open for writing (remote half-close).
    RecvClosed
}

struct NotifierRead {
    /// List of wakers to wake when read operations can proceed
    /// on a substream (or in general, for the key `None`).
    pending: Mutex<FnvHashMap<Option<LocalStreamId>, Waker>>,
}

struct NotifierWrite {
    /// List of wakers to wake when write operations on the
    /// underlying I/O stream can proceed.
    pending: Mutex<Vec<Waker>>,
}

impl NotifierWrite {
    /// Registers interest of a task in writing to some substream.
    ///
    /// The returned waker should be passed to an I/O write operation
    /// that schedules a wakeup, if necessary.
    #[must_use]
    fn register<'a>(self: &'a Arc<Self>, waker: &Waker) -> WakerRef<'a> {
        let mut pending = self.pending.lock();
        if pending.iter().all(|w| !w.will_wake(waker)) {
            pending.push(waker.clone());
        }
        waker_ref(self)
    }
}

impl ArcWake for NotifierWrite {
    fn wake_by_ref(this: &Arc<Self>) {
        let wakers = mem::replace(&mut *this.pending.lock(), Default::default());
        for waker in wakers {
            waker.wake();
        }
    }
}

impl NotifierRead {
    /// Registers interest of a task in reading from a particular
    /// stream, or any stream if `stream` is `None`.
    ///
    /// The returned waker should be passed to an I/O read operation
    /// that schedules a wakeup, if necessary.
    #[must_use]
    fn register<'a>(self: &'a Arc<Self>, waker: &Waker, stream: Option<LocalStreamId>)
        -> WakerRef<'a>
    {
        let mut pending = self.pending.lock();
        pending.insert(stream, waker.clone());
        waker_ref(self)
    }

    /// Wakes the last task that has previously registered interest
    /// in reading data from a particular stream (or any stream).
    ///
    /// Returns `true` if a task has been woken.
    fn wake_by_id(&self, id: LocalStreamId) -> bool {
        let mut woken = false;
        let mut pending = self.pending.lock();

        if let Some(waker) = pending.remove(&None) {
            waker.wake();
            woken = true;
        }

        if let Some(waker) = pending.remove(&Some(id)) {
            waker.wake();
            woken = true;
        }

        woken
    }
}

impl ArcWake for NotifierRead {
    fn wake_by_ref(this: &Arc<Self>) {
        let wakers = mem::replace(&mut *this.pending.lock(), Default::default());
        for (_, waker) in wakers {
            waker.wake();
        }
    }
}

struct NotifierOpen {
    /// List of wakers to wake when a new substream can be opened.
    pending: Mutex<Vec<Waker>>,
}

impl NotifierOpen {
    /// Registers interest of a task in opening a new substream.
    fn register<'a>(self: &'a Arc<Self>, waker: &Waker) -> WakerRef<'a> {
        let mut pending = self.pending.lock();
        if pending.iter().all(|w| !w.will_wake(waker)) {
            pending.push(waker.clone());
        }
        waker_ref(self)
    }
}

impl ArcWake for NotifierOpen {
    fn wake_by_ref(this: &Arc<Self>) {
        let wakers = mem::replace(&mut *this.pending.lock(), Default::default());
        for waker in wakers {
            waker.wake();
        }
    }
}


/// The maximum number of pending stream resets we are willing
/// to buffer. If this limit is exceeded as a result of an
/// inbound `Open` frame that exceeds the substream limits,
/// the remote is too ill-behaved and the multiplexed stream
/// terminates with an error.
const MAX_PENDING_RESETS: usize = 1000;
