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

use std::time::Duration;
use libp2p::{self, PeerId, Transport, mplex, secio, yamux};
use libp2p::core::{either, upgrade, transport::boxed::Boxed, muxing::StreamMuxerBox};
use libp2p::transport_timeout::TransportTimeout;

/// Builds the transport that serves as a common ground for all connections.
pub fn build_transport(
	local_private_key: secio::SecioKeyPair,
	timeout: Option<Duration>,
) -> Boxed<(PeerId, StreamMuxerBox)> {
	let base = libp2p::CommonTransport::new()
		.with_upgrade(secio::SecioConfig::new(local_private_key))
		.and_then(move |out, endpoint, client_addr| {
			let upgrade = upgrade::or(
				upgrade::map(mplex::MplexConfig::new(), either::EitherOutput::First),
				upgrade::map(yamux::Config::default(), either::EitherOutput::Second),
			);
			let peer_id = out.remote_key.into_peer_id();
			let upgrade = upgrade::map(upgrade, move |muxer| (peer_id, muxer));
			upgrade::apply(out.stream, upgrade, endpoint, client_addr)
		})
		.map(|(id, muxer), _| (id, StreamMuxerBox::new(muxer)));

	if let Some(timeout) = timeout {
		TransportTimeout::new(base, timeout).boxed()
	} else {
		base.boxed()
	}
}
