pub(crate) mod noise;

use crate::stream::DataChannel;

pub(crate) use super::fingerprint::Fingerprint;
use super::stream::Stream;
use super::Error;
use super::{sdp, Connection};
use js_sys::{Object, Reflect};
use libp2p_identity::{Keypair, PeerId};
use send_wrapper::SendWrapper;
use std::net::SocketAddr;
use wasm_bindgen_futures::JsFuture;
use web_sys::{RtcConfiguration, RtcPeerConnection};

const SHA2_256: u64 = 0x12;
const SHA2_512: u64 = 0x13;

pub(crate) async fn outbound(
    sock_addr: SocketAddr,
    remote_fingerprint: Fingerprint,
    id_keys: Keypair,
) -> Result<(PeerId, Connection), Error> {
    let fut = SendWrapper::new(outbound_inner(sock_addr, remote_fingerprint, id_keys));
    fut.await
}

async fn outbound_inner(
    sock_addr: SocketAddr,
    remote_fingerprint: Fingerprint,
    id_keys: Keypair,
) -> Result<(PeerId, Connection), Error> {
    let hash = match remote_fingerprint.to_multihash().code() {
        SHA2_256 => "sha-256",
        SHA2_512 => "sha-512",
        _ => return Err(Error::JsError("unsupported hash".to_string())),
    };

    let algo: js_sys::Object = Object::new();
    Reflect::set(&algo, &"name".into(), &"ECDSA".into()).unwrap();
    Reflect::set(&algo, &"namedCurve".into(), &"P-256".into()).unwrap();
    Reflect::set(&algo, &"hash".into(), &hash.into()).unwrap();

    let certificate_promise = RtcPeerConnection::generate_certificate_with_object(&algo)
        .expect("certificate to be valid");

    let certificate = JsFuture::from(certificate_promise).await?; // Needs to be Send

    let ice: js_sys::Object = Object::new();
    Reflect::set(&ice, &"urls".into(), &"stun:stun.l.google.com:19302".into()).unwrap();

    let mut config = RtcConfiguration::default();
    // wrap certificate in a js Array first before adding it to the config object
    let certificate_arr = js_sys::Array::new();
    certificate_arr.push(&certificate);
    config.certificates(&certificate_arr);

    let peer_connection = web_sys::RtcPeerConnection::new_with_configuration(&config)?;

    // Create substream for Noise handshake
    // Must create data channel before Offer is created for it to be included in the SDP
    let handshake_data_channel = DataChannel::new_handshake(&peer_connection);

    let webrtc_stream = Stream::new(handshake_data_channel);

    let ufrag = format!("libp2p+webrtc+v1/{}", gen_ufrag(32));
    /*
     * OFFER
     */
    let offer = JsFuture::from(peer_connection.create_offer()).await?; // Needs to be Send
    let offer_obj = sdp::offer(offer, &ufrag);
    log::debug!("Offer SDP: {:?}", offer_obj);
    let sld_promise = peer_connection.set_local_description(&offer_obj);
    JsFuture::from(sld_promise)
        .await
        .expect("set_local_description to succeed");

    /*
     * ANSWER
     */
    // TODO: Update SDP Answer format for Browser WebRTC
    let answer_obj = sdp::answer(sock_addr, &remote_fingerprint, &ufrag);
    log::debug!("Answer SDP: {:?}", answer_obj);
    let srd_promise = peer_connection.set_remote_description(&answer_obj);
    JsFuture::from(srd_promise)
        .await
        .expect("set_remote_description to succeed");

    // get local_fingerprint from local RtcPeerConnection peer_connection certificate
    let local_sdp = match &peer_connection.local_description() {
        Some(description) => description.sdp(),
        None => return Err(Error::JsError("local_description is None".to_string())),
    };
    let local_fingerprint = match sdp::fingerprint(&local_sdp) {
        Ok(fingerprint) => fingerprint,
        Err(e) => return Err(Error::JsError(format!("local fingerprint error: {}", e))),
    };

    log::debug!("local_fingerprint: {:?}", local_fingerprint);
    log::debug!("remote_fingerprint: {:?}", remote_fingerprint);

    let peer_id = noise::outbound(
        id_keys,
        webrtc_stream,
        remote_fingerprint,
        local_fingerprint,
    )
    .await?;

    log::debug!("peer_id: {:?}", peer_id);

    Ok((peer_id, Connection::new(peer_connection)))
}

fn gen_ufrag(len: usize) -> String {
    let charset = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut ufrag = String::new();
    let mut buf = vec![0; len];
    getrandom::getrandom(&mut buf).unwrap();
    for i in buf {
        let idx = i as usize % charset.len();
        ufrag.push(charset.chars().nth(idx).unwrap());
    }
    ufrag
}
