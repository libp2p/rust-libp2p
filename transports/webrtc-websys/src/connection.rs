//! Websys WebRTC Connection
//!
use crate::cbfutures::CbFuture;
use crate::fingerprint::Fingerprint;
use crate::stream::DataChannelConfig;
use crate::upgrade::{self};
use crate::utils;
use crate::{Error, WebRTCStream};
use futures::FutureExt;
use js_sys::Object;
use js_sys::Reflect;
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use libp2p_identity::{Keypair, PeerId};
use send_wrapper::SendWrapper;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use wasm_bindgen_futures::JsFuture;
use web_sys::{RtcConfiguration, RtcDataChannel, RtcPeerConnection};

pub const SHA2_256: u64 = 0x12;
pub const SHA2_512: u64 = 0x13;

pub struct Connection {
    // Swarm needs all types to be Send. WASM is single-threaded
    // and it is safe to use SendWrapper.
    inner: SendWrapper<ConnectionInner>,
}

struct ConnectionInner {
    peer_connection: Option<RtcPeerConnection>,
    sock_addr: SocketAddr,
    remote_fingerprint: Fingerprint,
    id_keys: Keypair,
    create_data_channel_cbfuture: CbFuture<RtcDataChannel>,
    closed: bool,
}

impl Connection {
    /// Create a new Connection
    pub fn new(sock_addr: SocketAddr, remote_fingerprint: Fingerprint, id_keys: Keypair) -> Self {
        Self {
            inner: SendWrapper::new(ConnectionInner::new(sock_addr, remote_fingerprint, id_keys)),
        }
    }

    /// Connect
    pub async fn connect(&mut self) -> Result<PeerId, Error> {
        let fut = SendWrapper::new(self.inner.connect());
        fut.await
    }

    /// Peer Connection Getter
    pub fn peer_connection(&self) -> Option<&RtcPeerConnection> {
        self.inner.peer_connection.as_ref()
    }
}

impl ConnectionInner {
    pub fn new(sock_addr: SocketAddr, remote_fingerprint: Fingerprint, id_keys: Keypair) -> Self {
        Self {
            peer_connection: None,
            sock_addr,
            remote_fingerprint,
            id_keys,
            create_data_channel_cbfuture: CbFuture::new(),
            closed: false,
        }
    }

    pub async fn connect(&mut self) -> Result<PeerId, Error> {
        let hash = match self.remote_fingerprint.to_multihash().code() {
            SHA2_256 => "sha-256",
            SHA2_512 => "sha2-512",
            _ => return Err(Error::JsError("unsupported hash".to_string())),
        };

        // let keygen_algorithm = json!({
        //     "name": "ECDSA",
        //     "namedCurve": "P-256",
        //     "hash": hash
        // });

        let algo: js_sys::Object = Object::new();
        Reflect::set(&algo, &"name".into(), &"ECDSA".into()).unwrap();
        Reflect::set(&algo, &"namedCurve".into(), &"P-256".into()).unwrap();
        Reflect::set(&algo, &"hash".into(), &hash.into()).unwrap();

        let certificate_promise = RtcPeerConnection::generate_certificate_with_object(&algo)
            .expect("certificate to be valid");

        let certificate = JsFuture::from(certificate_promise).await?; // Needs to be Send

        let mut config = RtcConfiguration::new();
        config.certificates(&certificate);

        let peer_connection = web_sys::RtcPeerConnection::new_with_configuration(&config)?;

        let ufrag = format!("libp2p+webrtc+v1/{}", utils::gen_ufrag(32));
        /*
         * OFFER
         */
        let offer = JsFuture::from(peer_connection.create_offer()).await?; // Needs to be Send
        let offer_obj = crate::sdp::offer(offer, &ufrag);
        let sld_promise = peer_connection.set_local_description(&offer_obj);
        JsFuture::from(sld_promise).await?;

        /*
         * ANSWER
         */
        let answer_obj = crate::sdp::answer(self.sock_addr, &self.remote_fingerprint, &ufrag);
        let srd_promise = peer_connection.set_remote_description(&answer_obj);
        JsFuture::from(srd_promise).await?;

        let peer_id = upgrade::outbound(
            &peer_connection,
            self.id_keys.clone(),
            self.remote_fingerprint,
        )
        .await?;

        self.peer_connection = Some(peer_connection);
        Ok(peer_id)
    }

    /// Initiates and polls a future from `create_data_channel`.
    fn poll_create_data_channel(
        &mut self,
        cx: &mut Context,
        config: DataChannelConfig,
    ) -> Poll<Result<WebRTCStream, Error>> {
        // Create Data Channel
        // take the peer_connection and DataChannelConfig and create a pollable future
        let mut dc =
            crate::stream::create_data_channel(&self.peer_connection.as_ref().unwrap(), config);

        let val = ready!(dc.poll_unpin(cx));

        let channel = WebRTCStream::new(val);

        Poll::Ready(Ok(channel))
    }

    /// Polls Incoming Peer Connections? Or Data Channels?
    pub fn poll_incoming(&mut self, cx: &mut Context) -> Poll<Result<WebRTCStream, Error>> {
        let mut dc = crate::stream::create_data_channel(
            &self.peer_connection.as_ref().unwrap(),
            DataChannelConfig::default(),
        );

        let val = ready!(dc.poll_unpin(cx));

        let channel = WebRTCStream::new(val);

        Poll::Ready(Ok(channel))
    }

    /// Closes the Peer Connection.
    ///
    /// This closes the data channels also and they will return an error
    /// if they are used.
    fn close_connection(&mut self) {
        if let (Some(conn), false) = (&self.peer_connection, self.closed) {
            conn.close();
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
    type Substream = WebRTCStream; // A Substream of a WebRTC PeerConnection is a Data Channel
    type Error = Error;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        // Inbound substreams for Browser WebRTC?
        // Can only be done through a relayed connection
        self.inner.poll_incoming(cx)
    }

    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        // Since this is not a initial handshake outbound request (ie. Dialer)
        // we need to create a new Data Channel without negotiated flag set to true
        let config = DataChannelConfig::default();
        self.inner.poll_create_data_channel(cx, config)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.close_connection();
        Poll::Ready(Ok(()))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        Poll::Pending
    }
}
