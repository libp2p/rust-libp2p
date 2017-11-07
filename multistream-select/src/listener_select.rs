// Copyright 2017 Parity Technologies (UK) Ltd.

// Libp2p-rs is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Libp2p-rs is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Libp2p-rs.  If not, see <http://www.gnu.org/licenses/>.

//! Contains the `listener_select_proto` code, which allows selecting a protocol thanks to
//! `multistream-select` for the listener.

use ProtocolChoiceError;
use bytes::Bytes;
use futures::{Future, Sink, Stream};
use futures::future::{err, loop_fn, Loop};

use protocol::DialerToListenerMessage;
use protocol::Listener;
use protocol::ListenerToDialerMessage;
use tokio_io::{AsyncRead, AsyncWrite};

/// Helps selecting a protocol amongst the ones supported.
///
/// This function expects a socket and an iterator of the list of supported protocols. The iterator
/// must be clonable (ie. iterable multiple times), because the list may need to be accessed
/// multiple times.
///
/// The iterator must produce tuples of the name of the protocol that is advertised to the remote,
/// a function that will check whether a remote protocol matches ours, and an identifier for the
/// protocol of type `P` (you decide what `P` is). The parameters of the function are the name
/// proposed by the remote, and the protocol name that we passed (so that you don't have to clone
/// the name).
///
/// On success, returns the socket and the identifier of the chosen protocol (of type `P`). The
/// socket now uses this protocol.
// TODO: remove the Box once -> impl Trait lands
pub fn listener_select_proto<'a, R, I, M, P>(
	inner: R,
	protocols: I,
) -> Box<Future<Item = (P, R), Error = ProtocolChoiceError> + 'a>
	where R: AsyncRead + AsyncWrite + 'a,
	      I: Iterator<Item = (Bytes, M, P)> + Clone + 'a,
	      M: FnMut(&Bytes, &Bytes) -> bool + 'a,
	      P: 'a
{
	let future = Listener::new(inner).from_err().and_then(move |listener| {

		loop_fn(listener, move |listener| {
			let protocols = protocols.clone();

			listener.into_future()
			        .map_err(|(e, _)| e.into())
			        .and_then(move |(message, listener)| match message {
				Some(DialerToListenerMessage::ProtocolsListRequest) => {
					let msg = ListenerToDialerMessage::ProtocolsListResponse {
						list: protocols.map(|(p, _, _)| p).collect(),
					};
					let fut = listener.send(msg).from_err().map(move |listener| (None, listener));
					Box::new(fut) as Box<Future<Item = _, Error = ProtocolChoiceError>>
				}
				Some(DialerToListenerMessage::ProtocolRequest { name }) => {
					let mut outcome = None;
					let mut send_back = ListenerToDialerMessage::NotAvailable;
					for (supported, mut matches, value) in protocols {
						if matches(&name, &supported) {
							send_back = ListenerToDialerMessage::ProtocolAck { name: name.clone() };
							outcome = Some(value);
							break;
						}
					}

					let fut = listener.send(send_back)
					                  .from_err()
					                  .map(move |listener| (outcome, listener));
					Box::new(fut) as Box<Future<Item = _, Error = ProtocolChoiceError>>
				}
				None => {
					Box::new(err(ProtocolChoiceError::NoProtocolFound)) as Box<_>
				}
			})
			        .map(|(outcome, listener): (_, Listener<R>)| match outcome {
				Some(outcome) => Loop::Break((outcome, listener.into_inner())),
				None => Loop::Continue(listener),
			})
		})
	});

	// The "Rust doesn't have impl Trait yet" tax.
	Box::new(future)
}
