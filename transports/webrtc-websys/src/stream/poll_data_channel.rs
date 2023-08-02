// Copied from webrtc::data::data_channel::poll_data_channel.rs
// use self::error::Result;
// use crate::{
//     error::Error, message::message_channel_ack::*, message::message_channel_open::*, message::*,
// };
use crate::cbfutures::{CbFuture, CbStream};
use crate::error::Error;
use futures::{AsyncRead, AsyncWrite, FutureExt, StreamExt};
use log::debug;
use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::result::Result;
use std::task::{ready, Context, Poll};
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelState};

/// Default capacity of the temporary read buffer used by [`webrtc_sctp::stream::PollStream`].
const DEFAULT_READ_BUF_SIZE: usize = 8192;

/// State of the read `Future` in [`PollStream`].
enum ReadFut {
    /// Nothing in progress.
    Idle,
    /// Reading data from the underlying stream.
    Reading(Pin<Box<dyn Future<Output = Result<Vec<u8>, Error>> + Send>>),
    /// Finished reading, but there's unread data in the temporary buffer.
    RemainingData(Vec<u8>),
}

impl ReadFut {
    /// Gets a mutable reference to the future stored inside `Reading(future)`.
    ///
    /// # Panics
    ///
    /// Panics if `ReadFut` variant is not `Reading`.
    fn get_reading_mut(
        &mut self,
    ) -> &mut Pin<Box<dyn Future<Output = Result<Vec<u8>, Error>> + Send>> {
        match self {
            ReadFut::Reading(ref mut fut) => fut,
            _ => panic!("expected ReadFut to be Reading"),
        }
    }
}

/// A wrapper around around [`RtcDataChannel`], which implements [`AsyncRead`] and
/// [`AsyncWrite`].
///
/// Both `poll_read` and `poll_write` calls allocate temporary buffers, which results in an
/// additional overhead.
pub struct PollDataChannel {
    data_channel: RtcDataChannel,

    onmessage_fut: CbStream<Vec<u8>>,
    onopen_fut: CbFuture<()>,
    onbufferedamountlow_fut: CbFuture<()>,

    read_buf_cap: usize,
}

impl PollDataChannel {
    /// Constructs a new `PollDataChannel`.
    pub fn new(data_channel: RtcDataChannel) -> Self {
        // On Open
        let onopen_fut = CbFuture::new();
        let onopen_cback_clone = onopen_fut.clone();

        let onopen_callback = Closure::<dyn FnMut(_)>::new(move |_ev: RtcDataChannelEvent| {
            // TODO: Send any queued messages
            debug!("Data Channel opened");
            onopen_cback_clone.publish(());
        });

        data_channel.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
        onopen_callback.forget();

        // On Close --  never needed, poll_close() doesn't use it.
        // let onclose_fut = CbFuture::new();
        // let onclose_cback_clone = onclose_fut.clone();

        // let onclose_callback = Closure::<dyn FnMut(_)>::new(move |_ev: RtcDataChannelEvent| {
        //     // TODO: Set state to closed?
        //     // TODO: Set futures::Stream Poll::Ready(None)?
        //     debug!("Data Channel closed. onclose_callback");
        //     onclose_cback_clone.publish(());
        // });

        // data_channel.set_onclose(Some(onclose_callback.as_ref().unchecked_ref()));
        // onclose_callback.forget();

        /*
         * On Error
         */
        let onerror_fut = CbFuture::new();
        let onerror_cback_clone = onerror_fut.clone();

        let onerror_callback = Closure::<dyn FnMut(_)>::new(move |ev: RtcDataChannelEvent| {
            debug!("Data Channel error");
            onerror_cback_clone.publish(());
        });

        data_channel.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
        onerror_callback.forget();

        /*
         * On Message Stream
         */
        let onmessage_fut = CbStream::new();
        let onmessage_cback_clone = onmessage_fut.clone();

        let onmessage_callback = Closure::<dyn FnMut(_)>::new(move |ev: MessageEvent| {
            // TODO: Use Protobuf decoder?
            let data = ev.data();
            // The convert fron Js ArrayBuffer to Vec<u8>
            let data = js_sys::Uint8Array::new(&data).to_vec();
            debug!("onmessage data: {:?}", data);
            // TODO: publish(None) to close the stream when the channel closes
            onmessage_cback_clone.publish(data);
        });

        data_channel.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        onmessage_callback.forget();

        /*
         * Convert `RTCDataChannel: bufferedamountlow event` Low Event Callback to Future
         */
        let onbufferedamountlow_fut = CbFuture::new();
        let onbufferedamountlow_cback_clone = onbufferedamountlow_fut.clone();

        let onbufferedamountlow_callback =
            Closure::<dyn FnMut(_)>::new(move |_ev: RtcDataChannelEvent| {
                debug!("bufferedamountlow event");
                onbufferedamountlow_cback_clone.publish(());
            });

        data_channel
            .set_onbufferedamountlow(Some(onbufferedamountlow_callback.as_ref().unchecked_ref()));
        onbufferedamountlow_callback.forget();

        Self {
            data_channel,
            onmessage_fut,
            onopen_fut,
            onbufferedamountlow_fut,
            read_buf_cap: DEFAULT_READ_BUF_SIZE,
        }
    }

    /// Get back the inner data_channel.
    pub fn into_inner(self) -> RtcDataChannel {
        self.data_channel
    }

    /// Obtain a clone of the inner data_channel.
    pub fn clone_inner(&self) -> RtcDataChannel {
        self.data_channel.clone()
    }

    /// Set the capacity of the temporary read buffer (default: 8192).
    pub fn set_read_buf_capacity(&mut self, capacity: usize) {
        self.read_buf_cap = capacity
    }

    /// Get Ready State of [RtcDataChannel]
    pub fn ready_state(&self) -> RtcDataChannelState {
        self.data_channel.ready_state()
    }

    /// Poll onopen_fut
    pub fn poll_onopen(&mut self, cx: &mut Context) -> Poll<()> {
        self.onopen_fut.poll_unpin(cx)
    }

    /// Send data buffer
    pub fn send(&self, data: &[u8]) -> Result<(), Error> {
        debug!("send: {:?}", data);
        self.data_channel.send_with_u8_array(data)?;
        Ok(())
    }

    /// StreamIdentifier returns the Stream identifier associated to the stream.
    pub fn stream_identifier(&self) -> u16 {
        // let label = self.data_channel.id(); // not available (yet), see https://github.com/rustwasm/wasm-bindgen/issues/3542

        // label is "" so it's not unique
        // FIXME: After the above issue is fixed, use the label instead of the stream id
        let label = self.data_channel.label();
        let b = label.as_bytes();
        let mut stream_id: u16 = 0;
        b.iter().enumerate().for_each(|(i, &b)| {
            stream_id += (b as u16) << (8 * i);
        });
        stream_id
    }
}

impl AsyncRead for PollDataChannel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        if let Some(data) = ready!(self.onmessage_fut.poll_next_unpin(cx)) {
            let data_len = data.len();
            let buf_len = buf.len();
            debug!("poll_read [{:?} of {} bytes]", data_len, buf_len);
            let len = std::cmp::min(data_len, buf_len);
            buf[..len].copy_from_slice(&data[..len]);
            Poll::Ready(Ok(len))
        } else {
            // if None, the stream is exhausted, no data to read
            Poll::Ready(Ok(0))
        }
    }
}

impl AsyncWrite for PollDataChannel {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        debug!("poll_write: [{:?}]", buf.len());
        // As long as the data channel is open we can write
        // So, poll on open state to confirm the channel is open
        if self.data_channel.ready_state() != RtcDataChannelState::Open {
            // poll onopen
            ready!(self.onopen_fut.poll_unpin(cx));
        }

        // Now that the channel is open, send the data
        match self.data_channel.send_with_u8_array(buf) {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(e) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Error sending data: {:?}", e),
            ))),
        }
    }

    /// Attempt to flush the object, ensuring that any buffered data reach their destination.
    /// On success, returns Poll::Ready(Ok(())).
    /// If flushing cannot immediately complete, this method returns Poll::Pending and arranges for the current task (via cx.waker().wake_by_ref()) to receive a notification when the object can make progress towards flushing.
    ///
    /// With RtcDataChannel, there no native future to await for flush to complete.
    /// However, Whenever this value decreases to fall to or below the value specified in the
    /// bufferedAmountLowThreshold property, the user agent fires the bufferedamountlow event.
    ///
    /// We can therefore create a callback future called `onbufferedamountlow_fut` to listen for `bufferedamountlow` event and wake the task
    /// The default `bufferedAmountLowThreshold` value is 0.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        debug!("poll_flush");

        // if bufferedamountlow empty,return ready
        if self.data_channel.buffered_amount() == 0 {
            debug!("0 buffered_amount");
            return Poll::Ready(Ok(()));
        }

        debug!(
            "buffered_amount is {:?}",
            self.data_channel.buffered_amount()
        );

        // Otherwise, wait for the event to occur, so poll on onbufferedamountlow_fut
        match self.onbufferedamountlow_fut.poll_unpin(cx) {
            Poll::Ready(()) => {
                debug!("flushed");
                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                debug!("pending");
                Poll::Pending
            }
        }
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        // close the data channel
        debug!("poll_close");
        self.data_channel.close();
        Poll::Ready(Ok(()))
    }
}

impl Clone for PollDataChannel {
    fn clone(&self) -> PollDataChannel {
        PollDataChannel::new(self.clone_inner())
    }
}

impl fmt::Debug for PollDataChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollDataChannel")
            .field("data_channel", &self.data_channel)
            .field("read_buf_cap", &self.read_buf_cap)
            .finish()
    }
}

impl AsRef<RtcDataChannel> for PollDataChannel {
    fn as_ref(&self) -> &RtcDataChannel {
        &self.data_channel
    }
}
