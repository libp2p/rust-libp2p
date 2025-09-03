//! A libp2p connection backed by an [RtcPeerConnection](https://developer.mozilla.org/en-US/docs/Web/API/RTCPeerConnection).

use std::{
    pin::Pin,
    task::{ready, Context, Poll, Waker},
};

use futures::{channel::mpsc, stream::FuturesUnordered, StreamExt};
use js_sys::{Object, Reflect};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use libp2p_webrtc_utils::Fingerprint;
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit, RtcDataChannelType,
    RtcSessionDescriptionInit,
};

use super::{Error, Stream};
use crate::stream::DropListener;

/// A WebRTC Connection.
///
/// All connections need to be [`Send`] which is why some fields are wrapped in [`SendWrapper`].
/// This is safe because WASM is single-threaded.
pub struct Connection {
    /// The [RtcPeerConnection] that is used for the WebRTC Connection
    inner: SendWrapper<RtcPeerConnection>,

    /// Whether the connection is closed
    closed: bool,
    /// An [`mpsc::channel`] for all inbound data channels.
    ///
    /// Because the browser's WebRTC API is event-based, we need to use a channel to obtain all
    /// inbound data channels.
    inbound_data_channels: SendWrapper<mpsc::Receiver<RtcDataChannel>>,
    /// A list of futures, which, once completed, signal that a [`Stream`] has been dropped.
    drop_listeners: FuturesUnordered<DropListener>,
    no_drop_listeners_waker: Option<Waker>,

    _ondatachannel_closure: SendWrapper<Closure<dyn FnMut(RtcDataChannelEvent)>>,
}

impl Connection {
    /// Create a new inner WebRTC Connection
    pub(crate) fn new(peer_connection: RtcPeerConnection) -> Self {
        // An ondatachannel Future enables us to poll for incoming data channel events in
        // poll_incoming
        let (mut tx_ondatachannel, rx_ondatachannel) = mpsc::channel(4); // we may get more than one data channel opened on a single peer connection

        let ondatachannel_closure = Closure::new(move |ev: RtcDataChannelEvent| {
            tracing::trace!("New data channel");

            if let Err(e) = tx_ondatachannel.try_send(ev.channel()) {
                if e.is_full() {
                    tracing::warn!("Remote is opening too many data channels, we can't keep up!");
                    return;
                }

                if e.is_disconnected() {
                    tracing::warn!("Receiver is gone, are we shutting down?");
                }
            }
        });
        peer_connection
            .inner
            .set_ondatachannel(Some(ondatachannel_closure.as_ref().unchecked_ref()));

        Self {
            inner: SendWrapper::new(peer_connection),
            closed: false,
            drop_listeners: FuturesUnordered::default(),
            no_drop_listeners_waker: None,
            inbound_data_channels: SendWrapper::new(rx_ondatachannel),
            _ondatachannel_closure: SendWrapper::new(ondatachannel_closure),
        }
    }

    fn new_stream_from_data_channel(&mut self, data_channel: RtcDataChannel) -> Stream {
        let (stream, drop_listener) = Stream::new(data_channel);

        self.drop_listeners.push(drop_listener);
        if let Some(waker) = self.no_drop_listeners_waker.take() {
            waker.wake()
        }
        stream
    }

    /// Closes the Peer Connection.
    ///
    /// This closes the data channels also and they will return an error
    /// if they are used.
    fn close_connection(&mut self) {
        if !self.closed {
            tracing::trace!("connection::close_connection");
            self.inner.inner.close();
            self.closed = true;
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close_connection();
    }
}

/// WebRTC native multiplexing
/// Allows users to open substreams
impl StreamMuxer for Connection {
    type Substream = Stream;
    type Error = Error;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match ready!(self.inbound_data_channels.poll_next_unpin(cx)) {
            Some(data_channel) => {
                let stream = self.new_stream_from_data_channel(data_channel);

                Poll::Ready(Ok(stream))
            }
            None => {
                // This only happens if the [`RtcPeerConnection::ondatachannel`] closure gets freed
                // which means we are most likely shutting down the connection.
                tracing::debug!("`Sender` for inbound data channels has been dropped");
                Poll::Ready(Err(Error::Connection("connection closed".to_owned())))
            }
        }
    }

    fn poll_outbound(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        tracing::trace!("Creating outbound data channel");

        let data_channel = self.inner.new_regular_data_channel();
        let stream = self.new_stream_from_data_channel(data_channel);

        Poll::Ready(Ok(stream))
    }

    /// Closes the Peer Connection.
    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        tracing::trace!("connection::poll_close");

        self.close_connection();
        Poll::Ready(Ok(()))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        loop {
            match ready!(self.drop_listeners.poll_next_unpin(cx)) {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    tracing::debug!("a DropListener failed: {e}")
                }
                None => {
                    self.no_drop_listeners_waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }
            }
        }
    }
}

pub(crate) struct RtcPeerConnection {
    inner: web_sys::RtcPeerConnection,
}

impl RtcPeerConnection {
    pub(crate) async fn new(algorithm: String) -> Result<Self, Error> {
        let algo: Object = Object::new();
        Reflect::set(&algo, &"name".into(), &"ECDSA".into()).unwrap();
        Reflect::set(&algo, &"namedCurve".into(), &"P-256".into()).unwrap();
        Reflect::set(&algo, &"hash".into(), &algorithm.into()).unwrap();

        let certificate_promise =
            web_sys::RtcPeerConnection::generate_certificate_with_object(&algo)
                .expect("certificate to be valid");

        let certificate = JsFuture::from(certificate_promise).await?;

        let config = RtcConfiguration::default();
        // wrap certificate in a js Array first before adding it to the config object
        let certificate_arr = js_sys::Array::new();
        certificate_arr.push(&certificate);
        config.set_certificates(&certificate_arr);

        let inner = web_sys::RtcPeerConnection::new_with_configuration(&config)?;

        Ok(Self { inner })
    }

    /// Creates the stream for the initial noise handshake.
    ///
    /// The underlying data channel MUST have `negotiated` set to `true` and carry the ID 0.
    pub(crate) fn new_handshake_stream(&self) -> (Stream, DropListener) {
        Stream::new(self.new_data_channel(true))
    }

    /// Creates a regular data channel for when the connection is already established.
    pub(crate) fn new_regular_data_channel(&self) -> RtcDataChannel {
        self.new_data_channel(false)
    }

    fn new_data_channel(&self, negotiated: bool) -> RtcDataChannel {
        const LABEL: &str = "";

        let dc = match negotiated {
            true => {
                let options = RtcDataChannelInit::new();
                options.set_negotiated(true);
                options.set_id(0); // id is only ever set to zero when negotiated is true

                self.inner
                    .create_data_channel_with_data_channel_dict(LABEL, &options)
            }
            false => self.inner.create_data_channel(LABEL),
        };
        dc.set_binary_type(RtcDataChannelType::Arraybuffer); // Hardcoded here, it's the only type we use

        dc
    }

    pub(crate) async fn create_offer(&self) -> Result<String, Error> {
        let offer = JsFuture::from(self.inner.create_offer()).await?;

        let offer = Reflect::get(&offer, &JsValue::from_str("sdp"))
            .expect("sdp should be valid")
            .as_string()
            .expect("sdp string should be valid string");

        Ok(offer)
    }

    pub(crate) async fn set_local_description(
        &self,
        sdp: RtcSessionDescriptionInit,
    ) -> Result<(), Error> {
        let promise = self.inner.set_local_description(&sdp);
        JsFuture::from(promise).await?;

        Ok(())
    }

    pub(crate) fn local_fingerprint(&self) -> Result<Fingerprint, Error> {
        let sdp = &self
            .inner
            .local_description()
            .ok_or_else(|| Error::Js("No local description".to_string()))?
            .sdp();

        let fingerprint =
            parse_fingerprint(sdp).ok_or_else(|| Error::Js("No fingerprint in SDP".to_string()))?;

        Ok(fingerprint)
    }

    pub(crate) async fn set_remote_description(
        &self,
        sdp: RtcSessionDescriptionInit,
    ) -> Result<(), Error> {
        let promise = self.inner.set_remote_description(&sdp);
        JsFuture::from(promise).await?;

        Ok(())
    }
}

/// Parse Fingerprint from a SDP.
fn parse_fingerprint(sdp: &str) -> Option<Fingerprint> {
    // split the sdp by new lines / carriage returns
    let lines = sdp.split("\r\n");

    // iterate through the lines to find the one starting with a=fingerprint:
    // get the value after the first space
    // return the value as a Fingerprint
    for line in lines {
        if line.starts_with("a=fingerprint:") {
            let fingerprint = line.split(' ').nth(1).unwrap();
            let bytes = hex::decode(fingerprint.replace(':', "")).unwrap();
            let arr: [u8; 32] = bytes.as_slice().try_into().unwrap();
            return Some(Fingerprint::raw(arr));
        }
    }
    None
}

#[cfg(test)]
mod sdp_tests {
    use super::*;

    #[test]
    fn test_fingerprint() {
        let sdp = "v=0\r\no=- 0 0 IN IP6 ::1\r\ns=-\r\nc=IN IP6 ::1\r\nt=0 0\r\na=ice-lite\r\nm=application 61885 UDP/DTLS/SCTP webrtc-datachannel\r\na=mid:0\r\na=setup:passive\r\na=ice-ufrag:libp2p+webrtc+v1/YwapWySn6fE6L9i47PhlB6X4gzNXcgFs\r\na=ice-pwd:libp2p+webrtc+v1/YwapWySn6fE6L9i47PhlB6X4gzNXcgFs\r\na=fingerprint:sha-256 A8:17:77:1E:02:7E:D1:2B:53:92:70:A6:8E:F9:02:CC:21:72:3A:92:5D:F4:97:5F:27:C4:5E:75:D4:F4:31:89\r\na=sctp-port:5000\r\na=max-message-size:16384\r\na=candidate:1467250027 1 UDP 1467250027 ::1 61885 typ host\r\n";

        let fingerprint = parse_fingerprint(sdp).unwrap();

        assert_eq!(fingerprint.algorithm(), "sha-256");
        assert_eq!(fingerprint.to_sdp_format(), "A8:17:77:1E:02:7E:D1:2B:53:92:70:A6:8E:F9:02:CC:21:72:3A:92:5D:F4:97:5F:27:C4:5E:75:D4:F4:31:89");
    }
}
