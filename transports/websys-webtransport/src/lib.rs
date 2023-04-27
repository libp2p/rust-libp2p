///! WebTransport is an active draft for the W3C! Expect breaking changes! 
/// This crate uses the draft from April 5th, 2023 https://www.w3.org/TR/2023/WD-webtransport-20230405/
/// 

use futures::{Future, future::Ready, FutureExt};
use js_sys::{Array, Object};
use libp2p_core::{
    transport::{ListenerId, TransportError, TransportEvent},
    Multiaddr, Transport, multiaddr::Protocol,
};
use parking_lot::Mutex;
// use send_wrapper::SendWrapper;
use wasm_bindgen::JsValue;
use web_sys::{WebTransport, WebTransportOptions};

/// Multihash code for sha-256
/// https://github.com/multiformats/multicodec/blob/master/table.csv#L9
const SHA_256: u64 = 0x12;

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct WebTransportTransport;

#[derive(Default, Debug)]
struct WebTransportAddress {
    address: String,
    port: u16,
    certhashes: Vec<Vec<u8>>,
}

impl WebTransportAddress {
    fn new(multiaddr: Multiaddr) -> Result<Self, String> {
        let mut wt_address = Self::default();
        let mut proto: Vec<_> = multiaddr.into_iter().collect();

        // strip any certhash suffixes
        let num_certhashes = 0;
        while let Some(Protocol::Certhash(mh)) = proto.iter().rev().next() {
            // current only valid hashing algorithm is sha-256
            // https://www.w3.org/TR/2023/WD-webtransport-20230405/#dom-webtransportoptions-servercertificatehashes
            match mh.code() {
                SHA_256 => {
                    wt_address.certhashes.push(mh.digest().to_vec());
                },
                _ => (),
            }
        }
        for _ in 0..num_certhashes {
            proto.pop();
        }

        // back to multiaddr
        let mut multiaddr = Multiaddr::from_iter(proto.into_iter());

        // strip quic/webtransport/ suffix
        if !multiaddr.ends_with(&libp2p_core::multiaddr::multiaddr!(Quic, WebTransport)) {
            return Err(format!("Not a WebTransport Address."))
        }
        multiaddr.pop();
        multiaddr.pop();


        // strip address suffix 
        let port = multiaddr.pop();
        let address = multiaddr.pop();
        if let (Some(address), Some(Protocol::Udp(port))) = (address, port) {
            wt_address.port = port;
            wt_address.address = match address {
                Protocol::Ip4(ip) => ip.to_string(),
                Protocol::Ip6(ip) => ip.to_string(),
                Protocol::Dns(d) => d.to_string(),
                Protocol::Dns4(d) => d.to_string(),
                Protocol::Dns6(d) => d.to_string(),
                Protocol::Dnsaddr(d) => d.to_string(),
                // TODO: DNS names
                p => return Err(format!("Unexpected Protocol {}, only use IPv4/6", p)),
            }
        }

        if multiaddr.iter().next().is_some() {
            return Err(format!("Unexpected Multiaddr prefixes {}, only direct WebTransport connections are supported (for now)", multiaddr))
        }

        Ok(wt_address)
    }

    fn to_url(self) -> (String, Vec<Vec<u8>>) {
        (format!("https://{}:{}/.well-known/libp2p-webtransport?type=noise", self.address, self.port), self.certhashes)
    }
}

fn certhashes_to_jsvalue(certhashes: Vec<Vec<u8>>) -> Array {
    let arr = Array::new_with_length(certhashes.len() as u32);
    for cert in certhashes {
        let value = js_sys::Object::default();

        let js_cert = js_sys::Uint8Array::default();
        js_cert.copy_from(&cert);

        Object::define_property(&value, &"algorithm".into(), &JsValue::from("sha-256").into());
        Object::define_property(&value, &"value".into(), &js_cert.buffer());
        arr.push(&value.into());
    }
    arr
}    

impl WebTransportTransport {
    
}


impl Transport for WebTransportTransport {
    type Output = Connection;

    type Error = Error;

    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(&mut self, _addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        Err(TransportError::Other(Error::NotSupported))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let wt_url = WebTransportAddress::new(addr).map_err(|s| TransportError::Other(Error::InvalidMultiaddr(s)))?;

        let (url, certhashes) = wt_url.to_url();

        // TODO untested
        Ok(async move {
            let mut options = WebTransportOptions::default();
            if !certhashes.is_empty() {
                options.server_certificate_hashes(&certhashes_to_jsvalue(certhashes));
            }

            let socket = WebTransport::new_with_options(&url, &options).map_err(|e| Error::JsError(format!("{e:?}")))?;

            Ok(Connection::new(socket))
        }.boxed())
    }

    fn dial_as_listener(
        &mut self,
        _addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::Other(Error::NotSupported))
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        todo!()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("js function error {0}")]
    JsError(String),
    #[error("operation not supported")]
    NotSupported,
    #[error("multiaddr not supported for WebTransport: {0}")]
    InvalidMultiaddr(String),
}

/// A Websocket connection created by the [`WebsocketTransport`].
pub struct Connection {
    /// We need to use Mutex as libp2p requires this to be Send.
    shared: Arc<Mutex<Shared>>,
}

impl Connection {
    fn new(socket: WebTransport) -> Self {
        Self { shared: Arc::new(Mutex::new(Shared {
            ready: false,
            closed: false,
            error: false,
            ojb: socket,
        }))}
    }
}

/// TODO
pub struct Shared {
    ready: bool,
    closed: bool,
    error: bool,
    ojb: WebTransport,
}
