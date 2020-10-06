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
use futures::task::{AtomicWaker, ArcWake, waker_ref, WakerRef};
use futures_codec::Framed;
use parking_lot::Mutex;
use smallvec::SmallVec;
use std::collections::VecDeque;
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
    /// The buffer of new inbound substreams that have not yet
    /// been drained by `poll_next_stream`.
    open_buffer: VecDeque<LocalStreamId>,
    /// Whether a flush is pending due to one or more new outbound
    /// `Open` frames, before reading frames can proceed.
    pending_flush_open: bool,
    /// The stream that currently blocks reading for all streams
    /// due to a full buffer, if any. Only applicable for use
    /// with [`MaxBufferBehaviour::Block`].
    blocked_stream: Option<LocalStreamId>,
    /// Pending frames to send at the next opportunity.
    ///
    /// An opportunity for sending pending frames is every flush
    /// or read operation. In the former case, sending of all
    /// pending frames must complete before the flush can complete.
    /// In the latter case, the read operation can proceed even
    /// if some or all of the pending frames cannot be sent.
    pending_frames: VecDeque<Frame<LocalStreamId>>,
    /// The managed substreams.
    substreams: FnvHashMap<LocalStreamId, SubstreamState>,
    /// The ID for the next outbound substream.
    next_outbound_stream_id: LocalStreamId,
    /// Registry of wakers for pending tasks interested in reading.
    notifier_read: Arc<NotifierRead>,
    /// Registry of wakers for pending tasks interested in writing.
    notifier_write: Arc<NotifierWrite>,
    /// Registry of wakers for pending tasks interested in opening
    /// an outbound substream, when the configured limit is reached.
    ///
    /// As soon as the number of substreams drops below this limit,
    /// these tasks are woken.
    notifier_open: NotifierOpen,
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
        Multiplexed {
            config,
            status: Status::Open,
            io: Framed::new(io, Codec::new()).fuse(),
            open_buffer: Default::default(),
            substreams: Default::default(),
            pending_flush_open: false,
            pending_frames: Default::default(),
            blocked_stream: None,
            next_outbound_stream_id: LocalStreamId::dialer(0),
            notifier_read: Arc::new(NotifierRead {
                read_stream: Mutex::new(Default::default()),
                next_stream: AtomicWaker::new(),
            }),
            notifier_write: Arc::new(NotifierWrite {
                pending: Mutex::new(Default::default()),
            }),
            notifier_open: NotifierOpen {
                pending: Default::default()
            }
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
                self.open_buffer = Default::default();
                self.substreams = Default::default();
                self.status = Status::Closed;
                Poll::Ready(Ok(()))
            }
        }
    }

    /// Waits for a new inbound substream, returning the corresponding `LocalStreamId`.
    pub fn poll_next_stream(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<LocalStreamId>> {
        self.guard_open()?;

        // Try to read from the buffer first.
        if let Some(stream_id) = self.open_buffer.pop_back() {
            return Poll::Ready(Ok(stream_id));
        }

        debug_assert!(self.open_buffer.is_empty());

        loop {
            // Wait for the next inbound `Open` frame.
            match ready!(self.poll_read_frame(cx, None))? {
                Frame::Open { stream_id } => {
                    if let Some(id) = self.on_open(stream_id)? {
                        return Poll::Ready(Ok(id))
                    }
                }
                Frame::Data { stream_id, data } => {
                    let id = stream_id.into_local();
                    if let Some(state) = self.substreams.get_mut(&id) {
                        if let Some(buf) = state.recv_buf_open() {
                            trace!("Buffering {:?} for stream {} (total: {})", data, id, buf.len() + 1);
                            buf.push(data);
                            self.notifier_read.wake_read_stream(id);
                        } else {
                            trace!("Dropping data {:?} for closed or reset substream {}", data, id);
                        }
                    } else {
                        trace!("Dropping data {:?} for unknown substream {}", data, id);
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
        if self.substreams.len() >= self.config.max_substreams {
            debug!("Maximum number of substreams reached: {}", self.config.max_substreams);
            self.notifier_open.register(cx.waker());
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
                        self.substreams.insert(stream_id, SubstreamState::Open {
                            buf: Default::default()
                        });
                        log::debug!("New outbound substream: {} (total {})",
                            stream_id, self.substreams.len());
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
    /// > **Note**: All substreams obtained via `poll_next_stream`
    /// > or `poll_open_stream` must eventually be "dropped" by
    /// > calling this method when they are no longer used.
    pub fn drop_stream(&mut self, id: LocalStreamId) {
        // Check if the underlying stream is ok.
        match self.status {
            Status::Closed | Status::Err(_) => return,
            Status::Open => {},
        }

        // If there is still a task waker interested in reading from that
        // stream, wake it to avoid leaving it dangling and notice that
        // the stream is gone. In contrast, wakers for write operations
        // are all woken on every new write opportunity.
        self.notifier_read.wake_read_stream(id);

        // Remove the substream, scheduling pending frames as necessary.
        match self.substreams.remove(&id) {
            None => return,
            Some(state) => {
                // If we fell below the substream limit, notify tasks that had
                // interest in opening an outbound substream earlier.
                let below_limit = self.substreams.len() == self.config.max_substreams - 1;
                if below_limit {
                    self.notifier_open.wake_all();
                }
                // Schedule any pending final frames to send, if necessary.
                match state {
                    SubstreamState::Closed { .. } => {}
                    SubstreamState::SendClosed { .. } => {}
                    SubstreamState::Reset { .. } => {}
                    SubstreamState::RecvClosed { .. } => {
                        if self.check_max_pending_frames().is_err() {
                            return
                        }
                        log::trace!("Pending close for stream {}", id);
                        self.pending_frames.push_front(Frame::Close { stream_id: id });
                    }
                    SubstreamState::Open { .. } => {
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
        match self.substreams.get(&id) {
            None | Some(SubstreamState::Reset { .. }) =>
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            Some(SubstreamState::SendClosed { .. }) | Some(SubstreamState::Closed { .. }) =>
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
            Some(SubstreamState::Open { .. }) | Some(SubstreamState::RecvClosed { .. }) => {
                // Substream is writeable. Continue.
            }
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
        if let Some(state) = self.substreams.get_mut(&id) {
            let buf = state.recv_buf();
            if !buf.is_empty() {
                if self.blocked_stream == Some(id) {
                    // Unblock reading new frames.
                    self.blocked_stream = None;
                    ArcWake::wake_by_ref(&self.notifier_read);
                }
                let data = buf.remove(0);
                return Poll::Ready(Ok(Some(data)))
            }
            // If the stream buffer "spilled" onto the heap, free that memory.
            buf.shrink_to_fit();
        }

        loop {
            // Check if the targeted substream (if any) reached EOF.
            if !self.can_read(&id) {
                // Note: Contrary to what is recommended by the spec, we must
                // return "EOF" also when the stream has been reset by the
                // remote, as the `StreamMuxer::read_substream` contract only
                // permits errors on "terminal" conditions, e.g. if the connection
                // has been closed or on protocol misbehaviour.
                return Poll::Ready(Ok(None))
            }

            // Check if there is a blocked stream.
            if let Some(blocked_id) = &self.blocked_stream {
                // We have a blocked stream and cannot continue reading
                // new frames for any stream, until frames are taken from
                // the blocked stream's buffer. Try to wake pending readers
                // of the blocked stream.
                if !self.notifier_read.wake_read_stream(*blocked_id) {
                    // No task dedicated to the blocked stream woken, so schedule
                    // this task again to have a chance at progress.
                    cx.waker().clone().wake();
                } else if blocked_id != &id {
                    // We woke some other task, but are still interested in
                    // reading from the current stream.
                    let _ = NotifierRead::register_read_stream(&self.notifier_read, cx.waker(), id);
                }
                return Poll::Pending
            }

            // Read the next frame.
            match ready!(self.poll_read_frame(cx, Some(id)))? {
                Frame::Data { data, stream_id } if stream_id.into_local() == id => {
                    return Poll::Ready(Ok(Some(data.clone())))
                },
                Frame::Data { stream_id, data } => {
                    // The data frame is for a different stream than the one
                    // currently being polled, so it needs to be buffered and
                    // the interested tasks notified.
                    let id = stream_id.into_local();
                    if let Some(state) = self.substreams.get_mut(&id) {
                        if let Some(buf) = state.recv_buf_open() {
                            debug_assert!(buf.len() <= self.config.max_buffer_len);
                            trace!("Buffering {:?} for stream {} (total: {})", data, id, buf.len() + 1);
                            buf.push(data);
                            self.notifier_read.wake_read_stream(id);
                            if buf.len() > self.config.max_buffer_len {
                                debug!("Frame buffer of stream {} is full.", id);
                                match self.config.max_buffer_behaviour {
                                    MaxBufferBehaviour::ResetStream => {
                                        let buf = buf.clone();
                                        self.check_max_pending_frames()?;
                                        self.substreams.insert(id, SubstreamState::Reset { buf });
                                        debug!("Pending reset for stream {}", id);
                                        self.pending_frames.push_front(Frame::Reset {
                                            stream_id: id
                                        });
                                    }
                                    MaxBufferBehaviour::Block => {
                                        self.blocked_stream = Some(id);
                                    }
                                }
                            }
                        } else {
                            trace!("Dropping data {:?} for closed substream {}", data, id);
                        }
                    } else {
                        trace!("Dropping data {:?} for unknown substream {}", data, id);
                    }
                }
                frame @ Frame::Open { .. } => {
                    if let Some(id) = self.on_open(frame.remote_id())? {
                        self.open_buffer.push_front(id);
                        trace!("Buffered new inbound stream {} (total: {})", id, self.open_buffer.len());
                        self.notifier_read.wake_next_stream();
                    }
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

        match self.substreams.remove(&id) {
            None => Poll::Ready(Ok(())),
            Some(SubstreamState::SendClosed { buf }) => {
                self.substreams.insert(id, SubstreamState::SendClosed { buf });
                Poll::Ready(Ok(()))
            }
            Some(SubstreamState::Closed { buf }) => {
                self.substreams.insert(id, SubstreamState::Closed { buf });
                Poll::Ready(Ok(()))
            }
            Some(SubstreamState::Reset { buf }) => {
                self.substreams.insert(id, SubstreamState::Reset { buf });
                Poll::Ready(Ok(()))
            }
            Some(SubstreamState::Open { buf }) => {
                if self.poll_send_frame(cx, || Frame::Close { stream_id: id })?.is_pending() {
                    self.substreams.insert(id, SubstreamState::Open { buf });
                    Poll::Pending
                } else {
                    debug!("Closed substream {} (half-close)", id);
                    self.substreams.insert(id, SubstreamState::SendClosed { buf });
                    Poll::Ready(Ok(()))
                }
            }
            Some(SubstreamState::RecvClosed { buf }) => {
                if self.poll_send_frame(cx, || Frame::Close { stream_id: id })?.is_pending() {
                    self.substreams.insert(id, SubstreamState::RecvClosed { buf });
                    Poll::Pending
                } else {
                    debug!("Closed substream {}", id);
                    self.substreams.insert(id, SubstreamState::Closed { buf });
                    Poll::Ready(Ok(()))
                }
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

        // Try to read another frame from the underlying I/O stream.
        let waker = match stream_id {
            Some(id) => NotifierRead::register_read_stream(&self.notifier_read, cx.waker(), id),
            None => NotifierRead::register_next_stream(&self.notifier_read, cx.waker())
        };
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
    fn on_open(&mut self, id: RemoteStreamId) -> io::Result<Option<LocalStreamId>> {
        let id = id.into_local();

        if self.substreams.contains_key(&id) {
            debug!("Received unexpected `Open` frame for open substream {}", id);
            return self.on_error(io::Error::new(io::ErrorKind::Other,
                "Protocol error: Received `Open` frame for open substream."))
        }

        if self.substreams.len() >= self.config.max_substreams {
            debug!("Maximum number of substreams exceeded: {}", self.config.max_substreams);
            self.check_max_pending_frames()?;
            debug!("Pending reset for new stream {}", id);
            self.pending_frames.push_front(Frame::Reset {
                stream_id: id
            });
            return Ok(None)
        }

        self.substreams.insert(id, SubstreamState::Open {
            buf: Default::default()
        });

        log::debug!("New inbound substream: {} (total {})", id, self.substreams.len());

        Ok(Some(id))
    }

    /// Processes an inbound `Reset` frame.
    fn on_reset(&mut self, id: LocalStreamId) {
        if let Some(state) = self.substreams.remove(&id) {
            match state {
                SubstreamState::Closed { .. } => {
                    trace!("Ignoring reset for mutually closed substream {}.", id);
                }
                SubstreamState::Reset { .. } => {
                    trace!("Ignoring redundant reset for already reset substream {}", id);
                }
                SubstreamState::RecvClosed { buf } |
                SubstreamState::SendClosed { buf } |
                SubstreamState::Open { buf } => {
                    debug!("Substream {} reset by remote.", id);
                    self.substreams.insert(id, SubstreamState::Reset { buf });
                    // Notify tasks interested in reading from that stream,
                    // so they may read the EOF.
                    NotifierRead::wake_read_stream(&self.notifier_read, id);
                }
            }
        } else {
            trace!("Ignoring `Reset` for unknown substream {}. Possibly dropped earlier.", id);
        }
    }

    /// Processes an inbound `Close` frame.
    fn on_close(&mut self, id: LocalStreamId) -> io::Result<()> {
        if let Some(state) = self.substreams.remove(&id) {
            match state {
                SubstreamState::RecvClosed { .. } | SubstreamState::Closed { .. } => {
                    debug!("Received unexpected `Close` frame for closed substream {}", id);
                    return self.on_error(
                        io::Error::new(io::ErrorKind::Other,
                        "Protocol error: Received `Close` frame for closed substream."))
                },
                SubstreamState::Reset { buf } => {
                    debug!("Ignoring `Close` frame for already reset substream {}", id);
                    self.substreams.insert(id, SubstreamState::Reset { buf });
                }
                SubstreamState::SendClosed { buf } => {
                    debug!("Substream {} closed by remote (SendClosed -> Closed).", id);
                    self.substreams.insert(id, SubstreamState::Closed { buf });
                    // Notify tasks interested in reading, so they may read the EOF.
                    self.notifier_read.wake_read_stream(id);
                },
                SubstreamState::Open { buf } => {
                    debug!("Substream {} closed by remote (Open -> RecvClosed)", id);
                    self.substreams.insert(id, SubstreamState::RecvClosed { buf });
                    // Notify tasks interested in reading, so they may read the EOF.
                    self.notifier_read.wake_read_stream(id);
                },
            }
        } else {
            trace!("Ignoring `Close` for unknown substream {}. Possibly dropped earlier.", id);
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
        match self.substreams.get(id) {
            Some(SubstreamState::Open { .. }) | Some(SubstreamState::SendClosed { .. }) => true,
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
        self.substreams = Default::default();
        self.open_buffer = Default::default();
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

type RecvBuf = SmallVec<[Bytes; 10]>;

/// The operating states of a substream.
#[derive(Clone, Debug)]
enum SubstreamState {
    /// An `Open` frame has been received or sent.
    Open { buf: RecvBuf },
    /// A `Close` frame has been sent, but the stream is still open
    /// for reading (half-close).
    SendClosed { buf: RecvBuf },
    /// A `Close` frame has been received but the stream is still
    /// open for writing (remote half-close).
    RecvClosed { buf: RecvBuf },
    /// A `Close` frame has been sent and received but the stream
    /// has not yet been dropped and may still have buffered
    /// frames to read.
    Closed { buf: RecvBuf },
    /// The stream has been reset by the local or remote peer but has
    /// not yet been dropped and may still have buffered frames to read.
    Reset { buf: RecvBuf }
}

impl SubstreamState {
    /// Mutably borrows the substream's receive buffer.
    fn recv_buf(&mut self) -> &mut RecvBuf {
        match self {
            SubstreamState::Open { buf } => buf,
            SubstreamState::SendClosed { buf } => buf,
            SubstreamState::RecvClosed { buf } => buf,
            SubstreamState::Closed { buf } => buf,
            SubstreamState::Reset { buf } => buf,
        }
    }

    /// Mutably borrows the substream's receive buffer if the substream
    /// is still open for reading, `None` otherwise.
    fn recv_buf_open(&mut self) -> Option<&mut RecvBuf> {
        match self {
            SubstreamState::Open { buf } => Some(buf),
            SubstreamState::SendClosed { buf } => Some(buf),
            SubstreamState::RecvClosed { .. } => None,
            SubstreamState::Closed { .. } => None,
            SubstreamState::Reset { .. } => None,
        }
    }
}

struct NotifierRead {
    /// The waker of the currently pending task that last
    /// called `poll_next_stream`, if any.
    next_stream: AtomicWaker,
    /// The wakers of currently pending tasks that last
    /// called `poll_read_stream` for a particular substream.
    read_stream: Mutex<FnvHashMap<LocalStreamId, Waker>>,
}

impl NotifierRead {
    /// Registers a task to be woken up when new `Data` frames for a particular
    /// stream can be read.
    ///
    /// The returned waker should be passed to an I/O read operation
    /// that schedules a wakeup, if the operation is pending.
    #[must_use]
    fn register_read_stream<'a>(self: &'a Arc<Self>, waker: &Waker, id: LocalStreamId)
        -> WakerRef<'a>
    {
        let mut pending = self.read_stream.lock();
        pending.insert(id, waker.clone());
        waker_ref(self)
    }

    /// Registers a task to be woken up when new `Open` frames can be read.
    ///
    /// The returned waker should be passed to an I/O read operation
    /// that schedules a wakeup, if the operation is pending.
    #[must_use]
    fn register_next_stream<'a>(self: &'a Arc<Self>, waker: &Waker) -> WakerRef<'a> {
        self.next_stream.register(waker);
        waker_ref(self)
    }

    /// Wakes the task pending on `poll_read_stream` for the
    /// specified stream, if any.
    fn wake_read_stream(&self, id: LocalStreamId) -> bool {
        let mut pending = self.read_stream.lock();

        if let Some(waker) = pending.remove(&id) {
            waker.wake();
            return true
        }

        false
    }

    /// Wakes the task pending on `poll_next_stream`, if any.
    fn wake_next_stream(&self) {
        self.next_stream.wake();
    }
}

impl ArcWake for NotifierRead {
    fn wake_by_ref(this: &Arc<Self>) {
        let wakers = mem::replace(&mut *this.read_stream.lock(), Default::default());
        for (_, waker) in wakers {
            waker.wake();
        }
        this.wake_next_stream();
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
    /// that schedules a wakeup if the operation is pending.
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
    /// Wakers of pending tasks interested in creating new
    /// outbound substreams.
    pending: Vec<Waker>,
}

impl NotifierOpen {
    /// Registers interest of a task in opening a new outbound substream.
    fn register(&mut self, waker: &Waker) {
        if self.pending.iter().all(|w| !w.will_wake(waker)) {
            self.pending.push(waker.clone());
        }
    }

    fn wake_all(&mut self) {
        let wakers = mem::replace(&mut self.pending, Default::default());
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
