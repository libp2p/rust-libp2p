use std::{
    cmp::min,
    io,
    pin::Pin,
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use bytes::BytesMut;
use futures::{AsyncRead, AsyncWrite, task::AtomicWaker};
use libp2p_webrtc_utils::MAX_MSG_LEN;
use wasm_bindgen::prelude::*;
use web_sys::{Event, MessageEvent, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelState};

/// [`PollDataChannel`] is a wrapper around [`RtcDataChannel`] which implements [`AsyncRead`] and
/// [`AsyncWrite`].
#[derive(Debug, Clone)]
pub(crate) struct PollDataChannel {
    /// The [`RtcDataChannel`] being wrapped.
    inner: RtcDataChannel,

    new_data_waker: Rc<AtomicWaker>,
    read_buffer: Rc<Mutex<BytesMut>>,

    /// Waker for when we are waiting for the DC to be opened.
    open_waker: Rc<AtomicWaker>,

    /// Waker for when we are waiting to write (again) to the DC because we previously exceeded the
    /// [`MAX_MSG_LEN`] threshold.
    write_waker: Rc<AtomicWaker>,

    /// Waker for when we are waiting for the DC to be closed.
    close_waker: Rc<AtomicWaker>,

    /// Whether we've been overloaded with data by the remote.
    ///
    /// This is set to `true` in case `read_buffer` overflows, i.e. the remote is sending us
    /// messages faster than we can read them. In that case, we return an [`std::io::Error`]
    /// from [`AsyncRead`] or [`AsyncWrite`], depending which one gets called earlier.
    /// Failing these will (very likely),
    /// cause the application developer to drop the stream which resets it.
    overloaded: Rc<AtomicBool>,

    /// Owns the JavaScript callbacks for the shared lifetime of all clones.
    _event_handlers: Rc<EventHandlers>,
}

/// Keeps the event callbacks alive and detaches them when the final stream clone is dropped.
#[derive(Debug)]
struct EventHandlers {
    inner: RtcDataChannel,
    _on_open_closure: Closure<dyn FnMut(RtcDataChannelEvent)>,
    _on_write_closure: Closure<dyn FnMut(Event)>,
    _on_close_closure: Closure<dyn FnMut(Event)>,
    _on_message_closure: Closure<dyn FnMut(MessageEvent)>,
}

impl Drop for EventHandlers {
    fn drop(&mut self) {
        // The browser may queue an event after Rust drops a stream. Detach each handler before
        // wasm-bindgen releases its Closure so a queued event cannot invoke a dropped closure.
        self.inner.set_onopen(None);
        self.inner.set_onbufferedamountlow(None);
        self.inner.set_onclose(None);
        self.inner.set_onmessage(None);
    }
}

/// Wakes outside the Web API callback that triggered the event.
///
/// Calling `wake` synchronously from an `RtcDataChannel` callback can re-enter the executor
/// while wasm-bindgen still mutably borrows that callback closure.
fn defer_wake(waker: Rc<AtomicWaker>) {
    wasm_bindgen_futures::spawn_local(async move {
        waker.wake();
    });
}

impl PollDataChannel {
    pub(crate) fn new(inner: RtcDataChannel) -> Self {
        let open_waker = Rc::new(AtomicWaker::new());
        let on_open_closure = Closure::new({
            let open_waker = open_waker.clone();

            move |_: RtcDataChannelEvent| {
                tracing::trace!("DataChannel opened");
                defer_wake(open_waker.clone());
            }
        });
        inner.set_onopen(Some(on_open_closure.as_ref().unchecked_ref()));

        let write_waker = Rc::new(AtomicWaker::new());
        inner.set_buffered_amount_low_threshold(0);
        let on_write_closure = Closure::new({
            let write_waker = write_waker.clone();

            move |_: Event| {
                tracing::trace!("DataChannel available for writing (again)");
                defer_wake(write_waker.clone());
            }
        });
        inner.set_onbufferedamountlow(Some(on_write_closure.as_ref().unchecked_ref()));

        let close_waker = Rc::new(AtomicWaker::new());
        let on_close_closure = Closure::new({
            let close_waker = close_waker.clone();

            move |_: Event| {
                tracing::trace!("DataChannel closed");
                defer_wake(close_waker.clone());
            }
        });
        inner.set_onclose(Some(on_close_closure.as_ref().unchecked_ref()));

        let new_data_waker = Rc::new(AtomicWaker::new());
        // We purposely don't use `with_capacity`
        // so we don't eagerly allocate `MAX_READ_BUFFER` per stream.
        let read_buffer = Rc::new(Mutex::new(BytesMut::new()));
        let overloaded = Rc::new(AtomicBool::new(false));

        let on_message_closure = Closure::<dyn FnMut(_)>::new({
            let new_data_waker = new_data_waker.clone();
            let read_buffer = read_buffer.clone();
            let overloaded = overloaded.clone();

            move |ev: MessageEvent| {
                let data = js_sys::Uint8Array::new(&ev.data());

                let mut read_buffer = read_buffer.lock().unwrap();

                if read_buffer.len() + data.length() as usize > MAX_MSG_LEN {
                    overloaded.store(true, Ordering::SeqCst);
                    tracing::warn!("Remote is overloading us with messages, resetting stream",);
                    return;
                }

                read_buffer.extend_from_slice(&data.to_vec());
                defer_wake(new_data_waker.clone());
            }
        });
        inner.set_onmessage(Some(on_message_closure.as_ref().unchecked_ref()));

        Self {
            inner: inner.clone(),
            new_data_waker,
            read_buffer,
            open_waker,
            write_waker,
            close_waker,
            overloaded,
            _event_handlers: Rc::new(EventHandlers {
                inner,
                _on_open_closure: on_open_closure,
                _on_write_closure: on_write_closure,
                _on_close_closure: on_close_closure,
                _on_message_closure: on_message_closure,
            }),
        }
    }

    /// Returns the [RtcDataChannelState] of the [RtcDataChannel]
    fn ready_state(&self) -> RtcDataChannelState {
        self.inner.ready_state()
    }

    /// Returns the current [RtcDataChannel] BufferedAmount
    fn buffered_amount(&self) -> usize {
        self.inner.buffered_amount() as usize
    }

    /// Whether the data channel is ready for reading or writing.
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.ready_state() {
            RtcDataChannelState::Connecting => {
                self.open_waker.register(cx.waker());
                return Poll::Pending;
            }
            RtcDataChannelState::Closing | RtcDataChannelState::Closed => {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            RtcDataChannelState::Open | RtcDataChannelState::__Invalid => {}
            _ => {}
        }

        if self.overloaded.load(Ordering::SeqCst) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "remote overloaded us with messages",
            )));
        }

        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for PollDataChannel {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        futures::ready!(this.poll_ready(cx))?;

        let mut read_buffer = this.read_buffer.lock().unwrap();

        if read_buffer.is_empty() {
            this.new_data_waker.register(cx.waker());
            return Poll::Pending;
        }

        // Ensure that we:
        // - at most return what the caller can read (`buf.len()`)
        // - at most what we have (`read_buffer.len()`)
        let split_index = min(buf.len(), read_buffer.len());

        let bytes_to_return = read_buffer.split_to(split_index);
        let len = bytes_to_return.len();
        buf[..len].copy_from_slice(&bytes_to_return);

        Poll::Ready(Ok(len))
    }
}

impl AsyncWrite for PollDataChannel {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        futures::ready!(this.poll_ready(cx))?;

        debug_assert!(this.buffered_amount() <= MAX_MSG_LEN);
        let remaining_space = MAX_MSG_LEN - this.buffered_amount();

        if remaining_space == 0 {
            this.write_waker.register(cx.waker());
            return Poll::Pending;
        }

        let bytes_to_send = min(buf.len(), remaining_space);

        if this
            .inner
            .send_with_u8_array(&buf[..bytes_to_send])
            .is_err()
        {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        Poll::Ready(Ok(bytes_to_send))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.buffered_amount() == 0 {
            return Poll::Ready(Ok(()));
        }

        self.write_waker.register(cx.waker());
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.ready_state() == RtcDataChannelState::Closed {
            return Poll::Ready(Ok(()));
        }

        if self.ready_state() != RtcDataChannelState::Closing {
            self.inner.close();
        }

        self.close_waker.register(cx.waker());
        Poll::Pending
    }
}
