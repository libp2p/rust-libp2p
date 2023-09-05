//! Websys WebRTC Peer Connection
//!
//! Creates and manages the [RtcPeerConnection](https://developer.mozilla.org/en-US/docs/Web/API/RTCPeerConnection)
use crate::stream::DropListener;

use super::{Error, Stream};
use futures::channel;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use js_sys::{Object, Reflect};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use libp2p_webrtc_utils::fingerprint::Fingerprint;
use send_wrapper::SendWrapper;
use std::pin::Pin;
use std::task::Waker;
use std::task::{ready, Context, Poll};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit, RtcDataChannelType,
    RtcSessionDescriptionInit,
};

/// A WebRTC Connection
pub struct Connection {
    // Swarm needs all types to be Send. WASM is single-threaded
    // and it is safe to use SendWrapper.
    inner: SendWrapper<ConnectionInner>,
}

impl Connection {
    /// Create a new WebRTC Connection
    pub(crate) fn new(peer_connection: RtcPeerConnection) -> Self {
        Self {
            inner: SendWrapper::new(ConnectionInner::new(peer_connection)),
        }
    }
}

/// Inner Connection that is wrapped in [SendWrapper]
struct ConnectionInner {
    /// The [RtcPeerConnection] that is used for the WebRTC Connection
    peer_connection: RtcPeerConnection,
    /// Whether the connection is closed
    closed: bool,
    /// A channel that signals incoming data channels
    rx_ondatachannel: channel::mpsc::Receiver<RtcDataChannel>,

    /// A list of futures, which, once completed, signal that a [`Stream`] has been dropped.
    /// Currently unimplemented, will be implemented in a future PR.
    drop_listeners: FuturesUnordered<crate::stream::DropListener>,
    /// Currently unimplemented, will be implemented in a future PR.
    no_drop_listeners_waker: Option<Waker>,
}

impl ConnectionInner {
    /// Create a new inner WebRTC Connection
    fn new(peer_connection: RtcPeerConnection) -> Self {
        // An ondatachannel Future enables us to poll for incoming data channel events in poll_incoming
        let (mut tx_ondatachannel, rx_ondatachannel) = channel::mpsc::channel(4); // we may get more than one data channel opened on a single peer connection

        // Wake the Future in the ondatachannel callback
        let ondatachannel_callback =
            Closure::<dyn FnMut(_)>::new(move |ev: RtcDataChannelEvent| {
                let dc2 = ev.channel();
                log::trace!("ondatachannel_callback triggered");
                match tx_ondatachannel.try_send(dc2) {
                    Ok(_) => log::trace!("ondatachannel_callback sent data channel"),
                    Err(e) => log::error!(
                        "ondatachannel_callback: failed to send data channel: {:?}",
                        e
                    ),
                }
            });

        peer_connection
            .inner
            .set_ondatachannel(Some(ondatachannel_callback.as_ref().unchecked_ref()));
        ondatachannel_callback.forget();

        Self {
            peer_connection,
            closed: false,
            drop_listeners: FuturesUnordered::default(),
            no_drop_listeners_waker: None,
            rx_ondatachannel,
        }
    }

    /// Initiates and polls a future from `create_data_channel`.
    /// Takes the RtcPeerConnection and creates a regular DataChannel
    fn poll_create_data_channel(&mut self, _cx: &mut Context) -> Poll<Stream> {
        log::trace!("Creating outbound data channel");

        let (stream, drop_listener) = self.peer_connection.new_stream();

        self.drop_listeners.push(drop_listener);
        if let Some(waker) = self.no_drop_listeners_waker.take() {
            waker.wake()
        }

        Poll::Ready(stream)
    }

    /// Polls the ondatachannel callback for inbound data channel stream.
    ///
    /// To poll for inbound WebRTCStreams, we need to poll for the ondatachannel callback
    /// We only get that callback for inbound data channels on our connections.
    /// This callback is converted to a future using channel, which we can poll here
    fn poll_ondatachannel(&mut self, cx: &mut Context) -> Poll<Result<Stream, Error>> {
        match ready!(self.rx_ondatachannel.poll_next_unpin(cx)) {
            Some(dc) => {
                // Create a WebRTC Stream from the Data Channel
                let (channel, drop_listener) = Stream::new(dc);

                self.drop_listeners.push(drop_listener);
                if let Some(waker) = self.no_drop_listeners_waker.take() {
                    waker.wake()
                }

                log::trace!("connection::poll_ondatachannel ready");
                Poll::Ready(Ok(channel))
            }
            None => {
                self.no_drop_listeners_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    /// Poll the Connection
    ///
    /// Mostly handles Dropped Listeners
    fn poll(&mut self, cx: &mut Context) -> Poll<Result<StreamMuxerEvent, Error>> {
        loop {
            match ready!(self.drop_listeners.poll_next_unpin(cx)) {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    log::debug!("a DropListener failed: {e}")
                }
                None => {
                    self.no_drop_listeners_waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }
            }
        }
    }

    /// Closes the Peer Connection.
    ///
    /// This closes the data channels also and they will return an error
    /// if they are used.
    fn close_connection(&mut self) {
        if !self.closed {
            log::trace!("connection::close_connection");
            self.peer_connection.inner.close();
            self.closed = true;
        }
    }
}

impl Drop for ConnectionInner {
    fn drop(&mut self) {
        self.close_connection();
    }
}

/// WebRTC native multiplexing
/// Allows users to open substreams
impl StreamMuxer for Connection {
    type Substream = Stream; // A Stream of a WebRTC PeerConnection is a Data Channel
    type Error = Error;

    /// Polls for inbound connections by waiting for the ondatachannel callback to be triggered
    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        // Inbound substream is signalled by an ondatachannel event
        self.inner.poll_ondatachannel(cx)
    }

    // We create the Data Channel here from the Peer Connection
    // then wait for the Data Channel to be opened
    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let stream = ready!(self.inner.poll_create_data_channel(cx));

        Poll::Ready(Ok(stream))
    }

    /// Closes the Peer Connection.
    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        log::trace!("connection::poll_close");

        self.inner.close_connection();
        Poll::Ready(Ok(()))
    }

    /// Polls the connection
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        self.inner.poll(cx)
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

        let mut config = RtcConfiguration::default();
        // wrap certificate in a js Array first before adding it to the config object
        let certificate_arr = js_sys::Array::new();
        certificate_arr.push(&certificate);
        config.certificates(&certificate_arr);

        let inner = web_sys::RtcPeerConnection::new_with_configuration(&config)?;

        Ok(Self { inner })
    }

    pub(crate) fn new_handshake_stream(&self) -> (Stream, DropListener) {
        Stream::new(self.new_data_channel(true))
    }

    pub(crate) fn new_stream(&self) -> (Stream, DropListener) {
        Stream::new(self.new_data_channel(false))
    }

    fn new_data_channel(&self, negotiated: bool) -> RtcDataChannel {
        const LABEL: &str = "";

        let dc = match negotiated {
            true => {
                let mut options = RtcDataChannelInit::new();
                options.negotiated(true).id(0); // id is only ever set to zero when negotiated is true

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
            .unwrap()
            .as_string()
            .unwrap();

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
            .ok_or_else(|| Error::JsError("No local description".to_string()))?
            .sdp();

        let fingerprint = parse_fingerprint(sdp)
            .ok_or_else(|| Error::JsError("No fingerprint in SDP".to_string()))?;

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
            return Some(Fingerprint::from(arr));
        }
    }
    None
}

#[cfg(test)]
mod sdp_tests {
    use super::*;

    #[test]
    fn test_fingerprint() {
        let sdp: &str = "v=0\r\no=- 0 0 IN IP6 ::1\r\ns=-\r\nc=IN IP6 ::1\r\nt=0 0\r\na=ice-lite\r\nm=application 61885 UDP/DTLS/SCTP webrtc-datachannel\r\na=mid:0\r\na=setup:passive\r\na=ice-ufrag:libp2p+webrtc+v1/YwapWySn6fE6L9i47PhlB6X4gzNXcgFs\r\na=ice-pwd:libp2p+webrtc+v1/YwapWySn6fE6L9i47PhlB6X4gzNXcgFs\r\na=fingerprint:sha-256 A8:17:77:1E:02:7E:D1:2B:53:92:70:A6:8E:F9:02:CC:21:72:3A:92:5D:F4:97:5F:27:C4:5E:75:D4:F4:31:89\r\na=sctp-port:5000\r\na=max-message-size:16384\r\na=candidate:1467250027 1 UDP 1467250027 ::1 61885 typ host\r\n";
        let fingerprint = match parse_fingerprint(sdp) {
            Some(fingerprint) => fingerprint,
            None => panic!("No fingerprint found"),
        };
        assert_eq!(fingerprint.algorithm(), "sha-256");
        assert_eq!(fingerprint.to_sdp_format(), "A8:17:77:1E:02:7E:D1:2B:53:92:70:A6:8E:F9:02:CC:21:72:3A:92:5D:F4:97:5F:27:C4:5E:75:D4:F4:31:89");
    }
}
