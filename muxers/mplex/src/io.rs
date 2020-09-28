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
use std::collections::{VecDeque, hash_map::Entry};
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
    /// Whether a flush is pending due to one or more new outbound
    /// `Open` frames, before reading frames can proceed.
    pending_flush_open: bool,
    /// Pending frames to send at the next opportunity.
    ///
    /// An opportunity for sending pending frames is every flush
    /// or read operation. In the former case, sending of all
    /// pending frames must complete before the flush can complete.
    /// In the latter case, the read operation can proceed even
    /// if some or all of the pending frames cannot be sent.
    pending_frames: VecDeque<Frame<LocalStreamId>>,
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
    Open,
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
            status: Status::Open,
            io: Framed::new(io, Codec::new()).fuse(),
            buffer: Vec::with_capacity(cmp::min(max_buffer_len, 512)),
            open_substreams: Default::default(),
            pending_flush_open: false,
            pending_frames: Default::default(),
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
            Status::Open => {}
        }

        // Send any pending frames.
        ready!(self.send_pending_frames(cx))?;

        // Flush the underlying I/O stream.
        let waker = NotifierWrite::register(&self.notifier_write, cx.waker());
        match ready!(self.io.poll_flush_unpin(&mut Context::from_waker(&waker))) {
            Err(e) => Poll::Ready(self.on_error(e)),
            Ok(()) => {
                self.pending_flush_open = false;
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
            Status::Open => {}
        }

        // Note: We do not make the effort to send pending `Reset` frames
        // here, we only close (and thus flush) the underlying I/O stream.

        let waker = NotifierWrite::register(&self.notifier_write, cx.waker());
        match self.io.poll_close_unpin(&mut Context::from_waker(&waker)) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(self.on_error(e)),
            Poll::Ready(Ok(())) => {
                self.pending_frames = VecDeque::new();
                // We do not support read-after-close on the underlying
                // I/O stream, hence clearing the buffer and substreams.
                self.buffer = Default::default();
                self.open_substreams = Default::default();
                self.status = Status::Closed;
                Poll::Ready(Ok(()))
            }
        }
    }

    /// Waits for a new inbound substream, returning the corresponding `LocalStreamId`.
    pub fn poll_next_stream(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<LocalStreamId>> {
        self.guard_open()?;

        // Try to read from the buffer first.
        while let Some((pos, stream_id)) = self.buffer.iter()
            .enumerate()
            .find_map(|(pos, frame)| match frame {
                Frame::Open { stream_id } => Some((pos, stream_id.into_local())),
                _ => None
            })
        {
            if self.buffer.len() == self.config.max_buffer_len {
                // The buffer is full and no longer will be, so notify all pending readers.
                ArcWake::wake_by_ref(&self.notifier_read);
            }
            self.buffer.remove(pos);
            if let Some(id) = self.on_open(stream_id)? {
                log::debug!("New inbound stream: {}", id);
                return Poll::Ready(Ok(id));
            }
        }

        loop {
            // Wait for the next inbound `Open` frame.
            match ready!(self.poll_read_frame(cx, None))? {
                Frame::Open { stream_id } => {
                    if let Some(id) = self.on_open(stream_id.into_local())? {
                        log::debug!("New inbound stream: {}", id);
                        return Poll::Ready(Ok(id))
                    }
                }
                frame @ Frame::Data { .. } => {
                    let id = frame.local_id();
                    if self.can_read(&id) {
                        trace!("Buffering {:?} (total: {})", frame, self.buffer.len() + 1);
                        self.buffer.push(frame);
                        self.notifier_read.wake_by_id(id);
                    } else {
                        trace!("Dropping {:?} for closed or unknown substream {}", frame, id);
                    }
                }
                Frame::Close { stream_id } => {
                    self.on_close(stream_id.into_local())?;
                }
                Frame::Reset { stream_id } => {
                    self.on_reset(stream_id.into_local())
                }
            }
        }
    }

    /// Creates a new (outbound) substream, returning the allocated stream ID.
    pub fn poll_open_stream(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<LocalStreamId>> {
        self.guard_open()?;

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
                        self.pending_flush_open = true;
                        Poll::Ready(Ok(stream_id))
                    }
                    Err(e) => Poll::Ready(self.on_error(e)),
                }
            },
            Err(e) => Poll::Ready(self.on_error(e))
        }
    }

    /// Immediately drops a substream.
    ///
    /// All locally allocated resources for the dropped substream
    /// are freed and the substream becomes unavailable for both
    /// reading and writing immediately. The remote is informed
    /// based on the current state of the substream:
    ///
    /// * If the substream was open, a `Reset` frame is sent at
    ///   the next opportunity.
    /// * If the substream was half-closed, i.e. a `Close` frame
    ///   has already been sent, nothing further happens.
    /// * If the substream was half-closed by the remote, i.e.
    ///   a `Close` frame has already been received, a `Close`
    ///   frame is sent at the next opportunity.
    ///
    /// If the multiplexed stream is closed or encountered
    /// an error earlier, or there is no known substream with
    /// the given ID, this is a no-op.
    ///
    /// > **Note**: If a substream is not read until EOF,
    /// > `drop_substream` _must_ eventually be called to avoid
    /// > leaving unread frames in the receive buffer.
    pub fn drop_stream(&mut self, id: LocalStreamId) {
        // Check if the underlying stream is ok.
        match self.status {
            Status::Closed | Status::Err(_) => return,
            Status::Open => {},
        }

        // Remove any frames still buffered for that stream. The stream
        // may already be fully closed (i.e. not in `open_substreams`)
        // but still have unread buffered frames.
        self.buffer.retain(|frame| frame.local_id() != id);

        // If there is still a task waker interested in reading from that
        // stream, wake it to avoid leaving it dangling and notice that
        // the stream is gone. In contrast, wakers for write operations
        // are all woken on every new write opportunity.
        self.notifier_read.wake_by_id(id);

        // Remove the substream, scheduling pending frames as necessary.
        match self.open_substreams.remove(&id) {
            None => return,
            Some(state) => {
                // If we fell below the substream limit, notify tasks that had
                // interest in opening a substream earlier.
                let below_limit = self.open_substreams.len() == self.config.max_substreams - 1;
                if below_limit {
                    ArcWake::wake_by_ref(&self.notifier_open);
                }
                // Schedule any pending final frames to send, if necessary.
                match state {
                    SubstreamState::SendClosed => {}
                    SubstreamState::RecvClosed => {
                        if self.check_max_pending_frames().is_err() {
                            return
                        }
                        log::trace!("Pending close for stream {}", id);
                        self.pending_frames.push_front(Frame::Close { stream_id: id });
                    }
                    SubstreamState::Open => {
                        if self.check_max_pending_frames().is_err() {
                            return
                        }
                        log::trace!("Pending reset for stream {}", id);
                        self.pending_frames.push_front(Frame::Reset { stream_id: id });
                    }
                }
            }
        }
    }

    /// Writes data to a substream.
    pub fn poll_write_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId, buf: &[u8])
        -> Poll<io::Result<usize>>
    {
        self.guard_open()?;

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
        self.guard_open()?;

        // Try to read from the buffer first.
        if let Some((pos, data)) = self.buffer.iter()
            .enumerate()
            .find_map(|(pos, frame)| match frame {
                Frame::Data { stream_id, data }
                    if stream_id.into_local() == id => Some((pos, data.clone())),
                _ => None
            })
        {
            if self.buffer.len() == self.config.max_buffer_len {
                // The buffer is full and no longer will be, so notify all pending readers.
                ArcWake::wake_by_ref(&self.notifier_read);
            }
            self.buffer.remove(pos);
            return Poll::Ready(Ok(Some(data)));
        }

        loop {
            // Check if the targeted substream (if any) reached EOF.
            if !self.can_read(&id) {
                return Poll::Ready(Ok(None))
            }

            match ready!(self.poll_read_frame(cx, Some(id)))? {
                Frame::Data { data, stream_id } if stream_id.into_local() == id => {
                    return Poll::Ready(Ok(Some(data.clone())))
                },
                frame @ Frame::Open { .. } | frame @ Frame::Data { .. } => {
                    let id = frame.local_id();
                    trace!("Buffering {:?} (total: {})", frame, self.buffer.len() + 1);
                    self.buffer.push(frame);
                    self.notifier_read.wake_by_id(id);
                }
                Frame::Close { stream_id } => {
                    let stream_id = stream_id.into_local();
                    self.on_close(stream_id)?;
                    if id == stream_id {
                        return Poll::Ready(Ok(None))
                    }
                }
                Frame::Reset { stream_id } => {
                    let stream_id = stream_id.into_local();
                    self.on_reset(stream_id);
                    if id == stream_id {
                        return Poll::Ready(Ok(None))
                    }
                }
            }
        }
    }

    /// Flushes a substream.
    ///
    /// > **Note**: This is equivalent to `poll_flush()`, i.e. to flushing
    /// > all substreams, except that this operation returns an error if
    /// > the underlying I/O stream is already closed.
    pub fn poll_flush_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId)
        -> Poll<io::Result<()>>
    {
        self.guard_open()?;

        ready!(self.poll_flush(cx))?;
        trace!("Flushed substream {}", id);

        Poll::Ready(Ok(()))
    }

    /// Closes a stream for writing.
    ///
    /// > **Note**: As opposed to `poll_close()`, a flush it not implied.
    pub fn poll_close_stream(&mut self, cx: &mut Context<'_>, id: LocalStreamId)
        -> Poll<io::Result<()>>
    {
        self.guard_open()?;

        match self.open_substreams.get(&id) {
            None | Some(SubstreamState::SendClosed) => Poll::Ready(Ok(())),
            Some(&state) => {
                ready!(self.poll_send_frame(cx, || Frame::Close { stream_id: id }))?;
                if state == SubstreamState::Open {
                    debug!("Closed substream {} (half-close)", id);
                    self.open_substreams.insert(id, SubstreamState::SendClosed);
                } else if state == SubstreamState::RecvClosed {
                    debug!("Closed substream {}", id);
                    self.open_substreams.remove(&id);
                    let below_limit = self.open_substreams.len() == self.config.max_substreams - 1;
                    if below_limit {
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
                    Err(e) => Poll::Ready(self.on_error(e))
                }
            },
            Err(e) => Poll::Ready(self.on_error(e))
        }
    }

    /// Reads the next frame from the underlying I/O stream.
    ///
    /// The given `stream_id` identifies the substream in which
    /// the current task is interested and wants to be woken up for,
    /// in case new frames can be read. `None` means interest in
    /// frames for any substream.
    fn poll_read_frame(&mut self, cx: &mut Context<'_>, stream_id: Option<LocalStreamId>)
        -> Poll<io::Result<Frame<RemoteStreamId>>>
    {
        // Try to send pending frames, if there are any, without blocking,
        if let Poll::Ready(Err(e)) = self.send_pending_frames(cx) {
            return Poll::Ready(Err(e))
        }

        // Perform any pending flush before reading.
        if self.pending_flush_open {
            trace!("Executing pending flush.");
            ready!(self.poll_flush(cx))?;
            debug_assert!(!self.pending_flush_open);
        }

        // Check if the inbound frame buffer is full.
        debug_assert!(self.buffer.len() <= self.config.max_buffer_len);
        if self.buffer.len() == self.config.max_buffer_len {
            debug!("Frame buffer full ({} frames).", self.buffer.len());
            match self.config.max_buffer_behaviour {
                MaxBufferBehaviour::CloseAll => {
                    return Poll::Ready(self.on_error(io::Error::new(io::ErrorKind::Other,
                        format!("Frame buffer full ({} frames).", self.buffer.len()))))
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
        match ready!(self.io.poll_next_unpin(&mut Context::from_waker(&waker))) {
            Some(Ok(frame)) => {
                trace!("Received {:?}", frame);
                Poll::Ready(Ok(frame))
            }
            Some(Err(e)) => Poll::Ready(self.on_error(e)),
            None => Poll::Ready(self.on_error(io::ErrorKind::UnexpectedEof.into()))
        }
    }

    /// Processes an inbound `Open` frame.
    fn on_open(&mut self, id: LocalStreamId) -> io::Result<Option<LocalStreamId>> {
        if self.open_substreams.contains_key(&id) {
            debug!("Received unexpected `Open` frame for open substream {}", id);
            return self.on_error(io::Error::new(io::ErrorKind::Other,
                "Protocol error: Received `Open` frame for open substream."))
        }

        if self.open_substreams.len() >= self.config.max_substreams {
            debug!("Maximum number of substreams exceeded: {}", self.config.max_substreams);
            self.check_max_pending_frames()?;
            debug!("Pending reset for new stream {}", id);
            self.pending_frames.push_front(Frame::Reset {
                stream_id: id
            });
            return Ok(None)
        }

        self.open_substreams.insert(id, SubstreamState::Open);

        Ok(Some(id))
    }

    /// Processes an inbound `Reset` frame.
    fn on_reset(&mut self, id: LocalStreamId) {
        if let Some(state) = self.open_substreams.remove(&id) {
            debug!("Substream {} in state {:?} reset by remote.", id, state);
            let below_limit = self.open_substreams.len() == self.config.max_substreams - 1;
            if below_limit {
                ArcWake::wake_by_ref(&self.notifier_open);
            }
            // Notify tasks interested in reading, so they may read the EOF.
            NotifierRead::wake_by_id(&self.notifier_read, id);
        } else {
            trace!("Ignoring `Reset` for unknown stream {}. Possibly dropped earlier.", id);
        }
    }

    /// Processes an inbound `Close` frame.
    fn on_close(&mut self, id: LocalStreamId) -> io::Result<()> {
        if let Entry::Occupied(mut e) = self.open_substreams.entry(id) {
            match e.get() {
                SubstreamState::RecvClosed => {
                    debug!("Received unexpected `Close` frame for closed substream {}", id);
                    return self.on_error(
                        io::Error::new(io::ErrorKind::Other,
                        "Protocol error: Received `Close` frame for closed substream."))
                },
                SubstreamState::SendClosed => {
                    debug!("Substream {} closed by remote (SendClosed -> Closed).", id);
                    e.remove();
                    // Notify tasks interested in opening new streams, if we fell
                    // below the limit.
                    let below_limit = self.open_substreams.len() == self.config.max_substreams - 1;
                    if below_limit {
                        ArcWake::wake_by_ref(&self.notifier_open);
                    }
                    // Notify tasks interested in reading, so they may read the EOF.
                    NotifierRead::wake_by_id(&self.notifier_read, id);
                },
                SubstreamState::Open => {
                    debug!("Substream {} closed by remote (Open -> RecvClosed)", id);
                    e.insert(SubstreamState::RecvClosed);
                    // Notify tasks interested in reading, so they may read the EOF.
                    NotifierRead::wake_by_id(&self.notifier_read, id);
                },
            }
        } else {
            trace!("Ignoring `Close` for unknown stream {}. Possibly dropped earlier.", id);
        }

        Ok(())
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

    /// Sends pending frames, without flushing.
    fn send_pending_frames(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while let Some(frame) = self.pending_frames.pop_back() {
            if self.poll_send_frame(cx, || {
                frame.clone()
            })?.is_pending() {
                self.pending_frames.push_back(frame);
                return Poll::Pending
            }
        }

        Poll::Ready(Ok(()))
    }

    /// Records a fatal error for the multiplexed I/O stream.
    fn on_error<T>(&mut self, e: io::Error) -> io::Result<T> {
        log::debug!("Multiplexed connection failed: {:?}", e);
        self.status = Status::Err(io::Error::new(e.kind(), e.to_string()));
        self.pending_frames =  Default::default();
        self.open_substreams = Default::default();
        self.buffer = Default::default();
        Err(e)
    }

    /// Checks that the multiplexed stream has status `Ok`,
    /// i.e. is not closed and did not encounter a fatal error.
    fn guard_open(&self) -> io::Result<()> {
        match &self.status {
            Status::Closed => Err(io::Error::new(io::ErrorKind::Other, "Connection is closed")),
            Status::Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            Status::Open => Ok(())
        }
    }

    /// Checks that the permissible limit for pending outgoing frames
    /// has not been reached.
    fn check_max_pending_frames(&mut self) -> io::Result<()> {
        if self.pending_frames.len() >= self.config.max_substreams + EXTRA_PENDING_FRAMES {
            return self.on_error(io::Error::new(io::ErrorKind::Other,
                "Too many pending frames."));
        }
        Ok(())
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

/// The maximum number of pending reset or close frames to send
/// we are willing to buffer beyond the configured substream limit.
/// This extra leeway bounds resource usage while allowing some
/// back-pressure when sending out these frames.
///
/// If too many pending frames accumulate, the multiplexed stream is
/// considered unhealthy and terminates with an error.
const EXTRA_PENDING_FRAMES: usize = 1000;
