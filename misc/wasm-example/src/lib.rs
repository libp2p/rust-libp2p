// Copyright 2019 Parity Technologies (UK) Ltd.
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

use futures::prelude::*;
use libp2p::{
    core::PublicKey, core::Transport, core::upgrade::InboundUpgradeExt, core::upgrade::OutboundUpgradeExt,
    secio,
};
use rand_core::{RngCore, SeedableRng};
use std::time::Duration;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn start(multiaddr_constructor: JsValue, transport: JsValue) -> JsValue {
    std::panic::set_hook(Box::new(|panic_info| {
        web_sys::console::log_1(&JsValue::from_str("Panic!"));
        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            web_sys::console::log_1(&JsValue::from_str(s));
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            web_sys::console::log_1(&JsValue::from_str(s));
        } else {
            web_sys::console::log_1(&JsValue::from_str("Unknown message type"));
        }
        if let Some(location) = panic_info.location() {
            web_sys::console::log_1(&JsValue::from_str(&location.file()));
            web_sys::console::log_1(&JsValue::from_f64(location.line() as f64));
        }
    }));

    // Create a random key for ourselves.
    let local_key = secio::SecioKeyPair::secp256k1_generated().unwrap();
    web_sys::console::log_1(&JsValue::from_str(&format!("Pubkey = {:?}", local_key.to_public_key())));
    let local_peer_id = local_key.to_peer_id();
    web_sys::console::log_1(&JsValue::from_str(&format!("Peerid = {}", local_peer_id.to_base58())));

    let transport = libp2p::js_transport::JsTransport::new(transport, multiaddr_constructor)
        .with_upgrade(libp2p::secio::SecioConfig::new(local_key))
        .and_then(move |out, endpoint| {
            let peer_id = out.remote_key.into_peer_id();
            let peer_id2 = peer_id.clone();
            let upgrade = libp2p::core::upgrade::SelectUpgrade::new(libp2p::yamux::Config::default(), libp2p::mplex::MplexConfig::new())
                // TODO: use a single `.map` instead of two maps
                .map_inbound(move |muxer| (peer_id, muxer))
                .map_outbound(move |muxer| (peer_id2, muxer));

            libp2p::core::upgrade::apply(out.stream, upgrade, endpoint)
                .map(|(id, muxer)| (id, libp2p::core::muxing::StreamMuxerBox::new(muxer)))
        })
        .with_timeout(Duration::from_secs(20));

    // Create a swarm to manage peers and events.
    let mut swarm = {
        // Create a Kademlia behaviour.
        // Note that normally the Kademlia process starts by performing lots of request in order
        // to insert our local node in the DHT. However here we use `without_init` because this
        // example is very ephemeral and we don't want to pollute the DHT. In a real world
        // application, you want to use `new` instead.
        let mut behaviour = libp2p::kad::Kademlia::without_init(local_peer_id.clone());
        behaviour.add_address(&"QmXmgU6RCiVGAyAuaP3QYSYMQfxHqz48ia5ZNvtPN2VCud".parse().unwrap(), "/ip4/206.189.241.107/tcp/30433/ws".parse().unwrap());
        /*behaviour.add_address(&"QmSoLer265NRgSp2LA3dPaeykiS1J6DifTC88f5uVQKNAd".parse().unwrap(), "/dns4/ams-1.bootstrap.libp2p.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"QmSoLMeWqB7YGVLJN3pNLQpmmEk35v6wYtsMGLzSr5QBU3".parse().unwrap(), "/dns4/lon-1.bootstrap.libp2p.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"QmSoLPppuBtQSGwKDZT2M73ULpjvfd3aZ6ha4oFGL1KrGM".parse().unwrap(), "/dns4/sfo-3.bootstrap.libp2p.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"QmSoLSafTMBsPKadTEgaXctDQVcqN88CNLHXMkTNwMKPnu".parse().unwrap(), "/dns4/sgp-1.bootstrap.libp2p.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"QmSoLueR4xBeUbY9WZ9xGUUxunbKWcrNFTDAadQJmocnWm".parse().unwrap(), "/dns4/nyc-1.bootstrap.libp2p.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64".parse().unwrap(), "/dns4/nyc-2.bootstrap.libp2p.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"QmZMxNdpMkewiVZLMRxaNxUeZpDUb34pWjZ1kZvsd16Zic".parse().unwrap(), "/dns4/node0.preload.ipfs.io/tcp/443/wss".parse().unwrap());
        behaviour.add_address(&"Qmbut9Ywz9YEDrz8ySBSgWyJk41Uvm2QJPhwDJzJyGFsD6".parse().unwrap(), "/dns4/node1.preload.ipfs.io/tcp/443/wss".parse().unwrap());*/
        libp2p::core::Swarm::new(transport, behaviour, local_peer_id)
    };

    // Order Kademlia to search for a peer.
    let mut csprng = rand_chacha::ChaChaRng::from_seed([0; 32]);
    let to_search = PublicKey::Secp256k1((0..32).map(|_| csprng.next_u32() as u8).collect()).into_peer_id();
    //println!("Searching for {:?}", to_search);
    swarm.find_node(to_search);

    // Kick it off!
    let future = futures::future::poll_fn(move || -> Result<_, JsValue> {
        loop {
            match swarm.poll().expect("Error while polling swarm") {
                Async::Ready(Some(ev @ libp2p::kad::KademliaOut::FindNodeResult { .. })) => {
                    let out = format!("Result: {:#?}", ev);
                    return Ok(Async::Ready(JsValue::from_str(&out)));
                },
                Async::Ready(Some(_)) => (),
                Async::Ready(None) | Async::NotReady => break,
            }
        }

        Ok(Async::NotReady)
    });

    wasm_bindgen_futures::future_to_promise(future).into()
}
