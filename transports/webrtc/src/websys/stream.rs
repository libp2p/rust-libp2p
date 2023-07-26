//! The Substream over the Connection
use super::cbfutures::CbFuture;
use futures::{AsyncRead, AsyncWrite, FutureExt};
use send_wrapper::SendWrapper;
use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use wasm_bindgen::prelude::*;
use web_sys::{
    MessageEvent, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit, RtcDataChannelState,
    RtcDataChannelType, RtcPeerConnection,
};

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    fn warn(s: &str);
}
// Max message size that can be sent to the DataChannel
const MAX_MESSAGE_SIZE: u32 = 16 * 1024;

// How much can be buffered to the DataChannel at once
const MAX_BUFFERED_AMOUNT: u32 = 16 * 1024 * 1024;

// How long time we wait for the 'bufferedamountlow' event to be emitted
const BUFFERED_AMOUNT_LOW_TIMEOUT: u32 = 30 * 1000;

#[derive(Debug)]
pub struct DataChannelConfig {
    negotiated: bool,
    id: u16,
    binary_type: RtcDataChannelType,
}

impl Default for DataChannelConfig {
    fn default() -> Self {
        Self {
            negotiated: false,
            id: 0,
            /// We set our default to Arraybuffer
            binary_type: RtcDataChannelType::Arraybuffer, // Blob is the default in the Browser,
        }
    }
}

/// Builds a Data Channel with selected options and given peer connection
///
///
impl DataChannelConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn negotiated(&mut self, negotiated: bool) -> &mut Self {
        self.negotiated = negotiated;
        self
    }

    /// Set a custom id for the Data Channel
    pub fn id(&mut self, id: u16) -> &mut Self {
        self.id = id;
        self
    }

    // Set the binary type for the Data Channel
    // TODO: We'll likely never use this, all channels are created with Arraybuffer?
    // pub fn binary_type(&mut self, binary_type: RtcDataChannelType) -> &mut Self {
    //     self.binary_type = binary_type;
    //     self
    // }

    /// Opens a WebRTC DataChannel for [RtcPeerConnection] with selected [DataChannelConfig]
    /// We can cretae `ondatachannel` future before building this
    /// then await it after building this
    pub fn open(&self, peer_connection: &RtcPeerConnection) -> RtcDataChannel {
        const LABEL: &str = "";

        let dc = match self.negotiated {
            true => {
                let mut data_channel_dict = RtcDataChannelInit::new();
                data_channel_dict.negotiated(true).id(self.id);
                peer_connection
                    .create_data_channel_with_data_channel_dict(LABEL, &data_channel_dict)
            }
            false => peer_connection.create_data_channel(LABEL),
        };
        dc.set_binary_type(self.binary_type);
        dc
    }
}

/// Substream over the Connection
pub struct WebRTCStream {
    inner: SendWrapper<StreamInner>,
}

impl WebRTCStream {
    /// Create a new Substream
    pub fn new(channel: RtcDataChannel) -> Self {
        Self {
            inner: SendWrapper::new(StreamInner::new(channel)),
        }
    }
}

#[derive(Debug, Clone)]
struct StreamInner {
    channel: RtcDataChannel,
    onclose_fut: CbFuture<()>,
    // incoming_data: FusedJsPromise,
    // message_queue: Vec<u8>,
    // ondatachannel_fut: CbFuture<RtcDataChannel>,
}

impl StreamInner {
    pub fn new(channel: RtcDataChannel) -> Self {
        // On Open
        let onopen_fut = CbFuture::new();
        let onopen_cback_clone = onopen_fut.clone();

        channel.set_onopen(Some(
            Closure::<dyn FnMut(_)>::new(move |_ev: RtcDataChannelEvent| {
                // TODO: Send any queued messages
                console_log!("Data Channel opened. onopen_callback");
                onopen_cback_clone.publish(());
            })
            .as_ref()
            .unchecked_ref(),
        ));

        // On Close
        let onclose_fut = CbFuture::new();
        let onclose_cback_clone = onclose_fut.clone();

        channel.set_onclose(Some(
            Closure::<dyn FnMut(_)>::new(move |_ev: RtcDataChannelEvent| {
                // TODO: Set state to closed?
                // TODO: Set futures::Stream Poll::Ready(None)?
                console_log!("Data Channel closed. onclose_callback");
                onclose_cback_clone.publish(());
            })
            .as_ref()
            .unchecked_ref(),
        ));

        /*
         * On Error
         */
        let onerror_fut = CbFuture::new();
        let onerror_cback_clone = onerror_fut.clone();

        channel.set_onerror(Some(
            Closure::<dyn FnMut(_)>::new(move |_ev: RtcDataChannelEvent| {
                console_log!("Data Channel error. onerror_callback");
                onerror_cback_clone.publish(());
            })
            .as_ref()
            .unchecked_ref(),
        ));

        /*
         * On Message
         */
        let onmessage_fut = CbFuture::new();
        let onmessage_cback_clone = onmessage_fut.clone();

        let onmessage_callback = Closure::<dyn FnMut(_)>::new(move |ev: MessageEvent| {
            // TODO: Use Protobuf decoder?
            let data = ev.data();
            let data = js_sys::Uint8Array::from(data);
            let data = data.to_vec();
            console_log!("onmessage: {:?}", data);
            // TODO: Howto? Should this feed a queue? futures::Stream?
            onmessage_cback_clone.publish(data);
        });

        Self {
            channel,
            onclose_fut,
            // incoming_data: FusedJsPromise::new(),
            // message_queue: Vec::new(),
            // ondatachannel_fut: CbFuture::new(),
        }
    }
}

pub fn create_data_channel(
    conn: &RtcPeerConnection,
    config: DataChannelConfig,
) -> CbFuture<RtcDataChannel> {
    // peer_connection.set_ondatachannel is callback based
    // but we need a Future we can poll
    // so we convert this callback into a Future by using [CbFuture]

    // 1. create the ondatachannel callbackFuture
    // 2. build the channel with the DataChannelConfig
    // 3. await the ondatachannel callbackFutures
    // 4. Now we have a ready DataChannel
    let ondatachannel_fut = CbFuture::new();
    let cback_clone = ondatachannel_fut.clone();

    // set up callback and futures
    // set_onopen callback to wake the Rust Future
    let ondatachannel_callback = Closure::<dyn FnMut(_)>::new(move |ev: RtcDataChannelEvent| {
        let dc2 = ev.channel();
        console_log!("pc2.ondatachannel!: {:?}", dc2.label());

        cback_clone.publish(dc2);
    });

    conn.set_ondatachannel(Some(ondatachannel_callback.as_ref().unchecked_ref()));

    let _dc = config.open(conn);

    ondatachannel_fut
}

impl AsyncRead for WebRTCStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        // self.inner.poll_read(cx, buf)
        let mut onmessage_fut = CbFuture::new();
        let cback_clone = onmessage_fut.clone();

        let onmessage_callback = Closure::<dyn FnMut(_)>::new(move |ev: MessageEvent| {
            let data = ev.data();
            let data = js_sys::Uint8Array::from(data);
            let data = data.to_vec();
            console_log!("onmessage: {:?}", data);
            cback_clone.publish(data);
        });

        self.inner
            .channel
            .set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));

        // poll onmessage_fut
        let data = ready!(onmessage_fut.poll_unpin(cx));
        let data_len = data.len();
        let buf_len = buf.len();
        let len = std::cmp::min(data_len, buf_len);
        buf[..len].copy_from_slice(&data[..len]);
        Poll::Ready(Ok(len))
    }
}

impl AsyncWrite for WebRTCStream {
    /// Attempt to write bytes from buf into the object.
    /// On success, returns Poll::Ready(Ok(num_bytes_written)).
    /// If the object is not ready for writing,
    /// the method returns Poll::Pending and
    /// arranges for the current task (via cx.waker().wake_by_ref())
    /// to receive a notification when the object becomes writable
    /// or is closed.
    ///
    /// In WebRTC DataChannels, we can always write to the channel
    /// so we don't need to poll the channel for writability
    /// as long as the channel is open
    ///
    /// So we need access to the channel or peer_connection,
    /// and the State of the Channel (DataChannel.readyState)
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.inner.channel.ready_state() {
            RtcDataChannelState::Connecting => {
                // TODO: Buffer message queue
                console_log!("DataChannel is Connecting");
                Poll::Pending
            }
            RtcDataChannelState::Open => {
                console_log!("DataChannel is Open");
                // let data = js_sys::Uint8Array::from(buf);
                let _ = self.inner.channel.send_with_u8_array(&buf);
                Poll::Ready(Ok(buf.len()))
            }
            RtcDataChannelState::Closing => {
                console_log!("DataChannel is Closing");
                Poll::Pending
            }
            RtcDataChannelState::Closed => {
                console_log!("DataChannel is Closed");
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "DataChannel is Closed",
                )))
            }
            RtcDataChannelState::__Nonexhaustive => {
                console_log!("DataChannel is __Nonexhaustive");
                Poll::Pending
            }
        }
        // data_channel.send(...)
        // self.inner.poll_write(cx, buf)
    }

    /// Attempt to flush the object, ensuring that any buffered data reach their destination.
    ///
    /// On success, returns Poll::Ready(Ok(())).
    ///
    /// If flushing cannot immediately complete, this method returns Poll::Pending and arranges for the current task (via cx.waker().wake_by_ref()) to receive a notification when the object can make progress towards flushing.
    // Flush means that we want to send all the data we have buffered to the
    // remote. We don't buffer anything, so we can just return Ready.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        // can only flush if the channel is open
        match self.inner.channel.ready_state() {
            RtcDataChannelState::Open => {
                // TODO: send data
                console_log!("DataChannel is Open");
                Poll::Ready(Ok(()))
            }
            _ => {
                console_log!("DataChannel is not Open, cannot flush");
                Poll::Pending
            }
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.inner.channel.close();
        ready!(self.inner.onclose_fut.poll_unpin(cx));
        Poll::Ready(Ok(()))
    }
}
