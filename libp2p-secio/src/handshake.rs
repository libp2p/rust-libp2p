// Copyright 2017 Parity Technologies (UK) Ltd.
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

use algo_support;
use bytes::BytesMut;
use codec::{full_codec, FullCodec};
use crypto::aes::{ctr, KeySize};
use error::SecioError;
use futures::Future;
use futures::future;
use futures::sink::Sink;
use futures::stream::Stream;
use keys_proto::{PublicKey as PublicKeyProtobuf, KeyType as KeyTypeProtobuf};
use protobuf::Message as ProtobufMessage;
use protobuf::core::parse_from_bytes as protobuf_parse_from_bytes;
use ring::{agreement, digest, rand};
use ring::agreement::EphemeralPrivateKey;
use ring::hmac::{SigningKey, SigningContext, VerificationKey};
use ring::rand::SecureRandom;
use ring::signature::{RSAKeyPair, RSASigningState, RSA_PKCS1_SHA256, RSA_PKCS1_2048_8192_SHA256};
use ring::signature::verify as signature_verify;
use std::cmp::{self, Ordering};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::sync::Arc;
use structs_proto::{Propose, Exchange};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited;
use untrusted::Input as UntrustedInput;

/// Performs a handshake on the given socket.
///
/// This function expects that the remote is identified with `remote_public_key`, and the remote
/// will expect that we are identified with `local_public_key`. Obviously `local_private_key` must
/// be paired with `local_public_key`. Any mismatch somewhere will produce a `SecioError`.
///
/// On success, returns an object that implements the `Sink` and `Stream` trait whose items are
/// buffers of data, plus the public key of the remote.
pub fn handshake<'a, S: 'a>(
	socket: S,
	local_public_key: Vec<u8>,
	local_private_key: Arc<RSAKeyPair>,
) -> Box<Future<Item = (FullCodec<S>, Vec<u8>), Error = SecioError> + 'a>
	where S: AsyncRead + AsyncWrite
{
	// TODO: could be rewritten as a coroutine once coroutines land in stable Rust

	// This struct contains the whole context of a handshake, and is filled progressively
	// throughout the various parts of the handshake.
	struct HandshakeContext {
		// Filled with this function's parameters.
		local_public_key: Vec<u8>,
		local_private_key: Arc<RSAKeyPair>,

		rng: rand::SystemRandom,
		// Locally-generated random number. The array size can be changed without any repercussion.
		local_nonce: [u8; 16],

		// Our local proposition's raw bytes.
		local_public_key_in_protobuf_bytes: Vec<u8>,
		local_proposition_bytes: Vec<u8>,

		// The remote proposition's raw bytes.
		remote_proposition_bytes: BytesMut,
		remote_public_key_in_protobuf_bytes: Vec<u8>,
		remote_public_key: Vec<u8>,

		// The remote peer's version of `local_nonce`.
		// If the NONCE size is actually part of the protocol, we can change this to a fixed-size
		// array instead of a `Vec`.
		remote_nonce: Vec<u8>,

		// Set to `ordering(
		//             hash(concat(remote-pubkey, local-none)),
		//             hash(concat(local-pubkey, remote-none))
		//         )`.
		// `Ordering::Equal` is an invalid value (as it would mean we're talking to ourselves).
		//
		// Since everything is symmetrical, this value is used to determine what should be ours
		// and what should be the remote's.
		hashes_ordering: Ordering,

		// Crypto algorithms chosen for the communication.
		chosen_exchange: Option<&'static agreement::Algorithm>,
		// We only support AES for now, so store just a key size.
		chosen_cipher: Option<KeySize>,
		chosen_hash: Option<&'static digest::Algorithm>,

		// Ephemeral key generated for the handshake and then thrown away.
		local_tmp_priv_key: Option<EphemeralPrivateKey>,
		local_tmp_pub_key: [u8; agreement::PUBLIC_KEY_MAX_LEN],
	}

	let context = HandshakeContext {
		local_public_key: local_public_key,
		local_private_key: local_private_key,
		rng: rand::SystemRandom::new(),
		local_nonce: Default::default(),
		local_public_key_in_protobuf_bytes: Vec::new(),
		local_proposition_bytes: Vec::new(),
		remote_proposition_bytes: BytesMut::new(),
		remote_public_key_in_protobuf_bytes: Vec::new(),
		remote_public_key: Vec::new(),
		remote_nonce: Vec::new(),
		hashes_ordering: Ordering::Equal,
		chosen_exchange: None,
		chosen_cipher: None,
		chosen_hash: None,
		local_tmp_priv_key: None,
		local_tmp_pub_key: [0; agreement::PUBLIC_KEY_MAX_LEN],
	};

	// The handshake messages all start with a 4-bytes message length prefix.
	let socket =
		length_delimited::Builder::new().big_endian().length_field_length(4).new_framed(socket);

	let future = future::ok::<_, SecioError>(context)
		// Generate our nonce.
		.and_then(|mut context| {
			context.rng.fill(&mut context.local_nonce)
				.map_err(|_| SecioError::NonceGenerationFailed)?;
			trace!(target: "libp2p-secio", "starting handshake ; local pubkey = {:?} ; \
											local nonce = {:?}",
				   context.local_public_key, context.local_nonce);
			Ok(context)
		})

		// Send our proposition with our nonce, public key and supported protocols.
		.and_then(|mut context| {
			let mut public_key = PublicKeyProtobuf::new();
			public_key.set_Type(KeyTypeProtobuf::RSA);
			public_key.set_Data(context.local_public_key.clone());
			context.local_public_key_in_protobuf_bytes = public_key.write_to_bytes().unwrap();

			let mut proposition = Propose::new();
			proposition.set_rand(context.local_nonce.clone().to_vec());
			proposition.set_pubkey(context.local_public_key_in_protobuf_bytes.clone());
			proposition.set_exchanges(algo_support::exchanges::PROPOSITION_STRING.into());
			proposition.set_ciphers(algo_support::ciphers::PROPOSITION_STRING.into());
			proposition.set_hashes(algo_support::hashes::PROPOSITION_STRING.into());
			let proposition_bytes = proposition.write_to_bytes().unwrap();
			context.local_proposition_bytes = proposition_bytes.clone();

			trace!(target: "libp2p-secio", "sending proposition to remote");

			socket.send(BytesMut::from(proposition_bytes.clone()))
				.from_err()
				.map(|s| (s, context))
		})

		// Receive the remote's proposition.
		.and_then(move |(socket, mut context)| {
			socket.into_future()
				.map_err(|(e, _)| e.into())
				.and_then(move |(prop_raw, socket)| {
					match prop_raw {
						Some(p) => context.remote_proposition_bytes = p,
						None => {
							let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
							debug!(target: "libp2p-secio", "unexpected eof while waiting for \
															remote's proposition");
							return Err(err.into())
						},
					};

					let mut prop = match protobuf_parse_from_bytes::<Propose>(&context.remote_proposition_bytes) {
						Ok(prop) => prop,
						Err(_) => {
							debug!(target: "libp2p-secio", "failed to parse remote's proposition \
															protobuf message");
							return Err(SecioError::HandshakeParsingFailure);
						}
					};
					context.remote_public_key_in_protobuf_bytes = prop.take_pubkey();
					let mut pubkey = {
						let bytes = &context.remote_public_key_in_protobuf_bytes;
						match protobuf_parse_from_bytes::<PublicKeyProtobuf>(bytes) {
							Ok(p) => p,
							Err(_) => {
								debug!(target: "libp2p-secio", "failed to parse remote's \
																proposition's pubkey protobuf");
								return Err(SecioError::HandshakeParsingFailure);
							},
						}
					};

					// TODO: For now we suppose that the key is in the RSA format because that's
					//       the only thing the Go and JS implementations support.
					match pubkey.get_Type() {
						KeyTypeProtobuf::RSA => (),
						format => {
							let err = IoError::new(IoErrorKind::Other, "unsupported protocol");
							debug!(target: "libp2p-secio", "unsupported remote pubkey format {:?}",
								   format);
							return Err(err.into());
						},
					};
					context.remote_public_key = pubkey.take_Data();
					context.remote_nonce = prop.take_rand();
					trace!(target: "libp2p-secio", "received proposition from remote ; \
													pubkey = {:?} ; nonce = {:?}",
						   context.remote_public_key, context.remote_nonce);
					Ok((prop, socket, context))
				})
		})

		// Decide which algorithms to use (thanks to the remote's proposition).
		.and_then(move |(remote_prop, socket, mut context)| {
			// In order to determine which protocols to use, we compute two hashes and choose
			// based on which hash is larger.
			context.hashes_ordering = {
				let oh1 = {
					let mut ctx = digest::Context::new(&digest::SHA256);
					ctx.update(&context.remote_public_key_in_protobuf_bytes);
					ctx.update(&context.local_nonce);
					ctx.finish()
				};

				let oh2 = {
					let mut ctx = digest::Context::new(&digest::SHA256);
					ctx.update(&context.local_public_key_in_protobuf_bytes);
					ctx.update(&context.remote_nonce);
					ctx.finish()
				};

				oh1.as_ref().cmp(&oh2.as_ref())
			};

			context.chosen_exchange = {
				let list = &remote_prop.get_exchanges();
				Some(match algo_support::exchanges::select_best(context.hashes_ordering, list) {
					Ok(a) => a,
					Err(err) => {
						debug!(target: "libp2p-secio", "failed to select an exchange protocol");
						return Err(err);
					}
				})
			};
			context.chosen_cipher = {
				let list = &remote_prop.get_ciphers();
				Some(match algo_support::ciphers::select_best(context.hashes_ordering, list) {
					Ok(a) => a,
					Err(err) => {
						debug!(target: "libp2p-secio", "failed to select a cipher protocol");
						return Err(err);
					}
				})
			};
			context.chosen_hash = {
				let list = &remote_prop.get_hashes();
				Some(match algo_support::hashes::select_best(context.hashes_ordering, list) {
					Ok(a) => a,
					Err(err) => {
						debug!(target: "libp2p-secio", "failed to select a hash protocol");
						return Err(err);
					}
				})
			};

			Ok((socket, context))
		})

		// Generate an ephemeral key for the negotiation.
		.and_then(|(socket, context)| {
			match EphemeralPrivateKey::generate(&agreement::ECDH_P256, &context.rng) {
				Ok(tmp_priv_key) => Ok((socket, context, tmp_priv_key)),
				Err(_) => {
					debug!(target: "libp2p-secio", "failed to generate ECDH key");
					Err(SecioError::EphemeralKeyGenerationFailed)
				},
			}
		})

		// Send the ephemeral pub key to the remote in an `Exchange` struct. The `Exchange` also
		// contains a signature of the two propositions encoded with our static public key.
		.and_then(|(socket, mut context, tmp_priv)| {
			let exchange = {
				let local_tmp_pub_key = &mut context.local_tmp_pub_key[..tmp_priv.public_key_len()];
				tmp_priv.compute_public_key(local_tmp_pub_key).unwrap();
				context.local_tmp_priv_key = Some(tmp_priv);

				let mut data_to_sign = context.local_proposition_bytes.clone();
				data_to_sign.extend_from_slice(&context.remote_proposition_bytes);
				data_to_sign.extend_from_slice(local_tmp_pub_key);

				let mut exchange = Exchange::new();
				exchange.set_epubkey(local_tmp_pub_key.to_vec());
				exchange.set_signature({
					let mut state = match RSASigningState::new(context.local_private_key.clone()) {
						Ok(s) => s,
						Err(_) => {
							debug!(target: "libp2p-secio", "failed to sign local exchange");
							return Err(SecioError::SigningFailure);
						},
					};
					let mut signature = vec![0; context.local_private_key.public_modulus_len()];
					match state.sign(&RSA_PKCS1_SHA256, &context.rng, &data_to_sign,
									 &mut signature)
					{
						Ok(_) => (),
						Err(_) => {
							debug!(target: "libp2p-secio", "failed to sign local exchange");
							return Err(SecioError::SigningFailure);
						},
					};

					signature
				});
				exchange
			};

			let local_exch = exchange.write_to_bytes()
				.expect("can only fail if the protobuf msg is malformed, which can't happen for \
						 this message in particular");
			Ok((BytesMut::from(local_exch), socket, context))
		})

		// Send our local `Exchange`.
		.and_then(|(local_exch, socket, context)| {
			trace!(target: "libp2p-secio", "sending exchange to remote");
			socket.send(local_exch)
				.from_err()
				.map(|s| (s, context))
		})

		// Receive the remote's `Exchange`.
		.and_then(move |(socket, context)| {
			socket.into_future()
				.map_err(|(e, _)| e.into())
				.and_then(move |(raw, socket)| {
					let raw = match raw {
						Some(r) => r,
						None => {
							let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
							debug!(target: "libp2p-secio", "unexpected eof while waiting for \
															remote's exchange");
							return Err(err.into())
						},
					};

					let remote_exch = match protobuf_parse_from_bytes::<Exchange>(&raw) {
						Ok(e) => e,
						Err(err) => {
							debug!(target: "libp2p-secio", "failed to parse remote's exchange \
															protobuf ; {:?}", err);
							return Err(SecioError::HandshakeParsingFailure);
						}
					};

					trace!(target: "libp2p-secio", "received and decoded the remote's exchange");
					Ok((remote_exch, socket, context))
				})
		})

		// Check the validity of the remote's `Exchange`. This verifies that the remote was really
		// the sender of its proposition, and that it is the owner of both its global and ephemeral
		// keys.
		.and_then(|(remote_exch, socket, context)| {
			let mut data_to_verify = context.remote_proposition_bytes.clone();
			data_to_verify.extend_from_slice(&context.local_proposition_bytes);
			data_to_verify.extend_from_slice(remote_exch.get_epubkey());

			// TODO: The ring library doesn't like some stuff in our DER public key, therefore
			//       we scrap the first 24 bytes of the key. A proper fix would be to write a DER
			//       parser, but that's not trivial.
			match signature_verify(&RSA_PKCS1_2048_8192_SHA256,
								UntrustedInput::from(&context.remote_public_key[24..]),
								UntrustedInput::from(&data_to_verify),
								UntrustedInput::from(remote_exch.get_signature()))
			{
				Ok(()) => (),
				Err(_) => {
					debug!(target: "libp2p-secio", "failed to verify the remote's signature");
					return Err(SecioError::SignatureVerificationFailed)
				},
			}

			trace!(target: "libp2p-secio", "successfully verified the remote's signature");
			Ok((remote_exch, socket, context))
		})

		// Generate a key from the local ephemeral private key and the remote ephemeral public key,
		// derive from it a ciper key, an iv, and a hmac key, and build the encoder/decoder.
		.and_then(|(remote_exch, socket, mut context)| {
			let local_priv_key = context.local_tmp_priv_key.take()
				.expect("we filled this Option earlier, and extract it now");
			let codec = agreement::agree_ephemeral(local_priv_key,
												   &context.chosen_exchange.clone().unwrap(),
												   UntrustedInput::from(remote_exch.get_epubkey()),
												   SecioError::SecretGenerationFailed,
												   |key_material| {
				let key = SigningKey::new(context.chosen_hash.unwrap(), key_material);

				let chosen_cipher = context.chosen_cipher.unwrap();
				let (cipher_key_size, iv_size) = match chosen_cipher {
					KeySize::KeySize128 => (16, 16),
					KeySize::KeySize256 => (32, 16),
					_ => panic!()
				};

				let mut longer_key = vec![0u8; 2 * (iv_size + cipher_key_size + 20)];
				stretch_key(&key, &mut longer_key);

				let (local_infos, remote_infos) = {
					let (first_half, second_half) = longer_key.split_at(longer_key.len() / 2);
					match context.hashes_ordering {
						Ordering::Equal => panic!(),
						Ordering::Less => (second_half, first_half),
						Ordering::Greater => (first_half, second_half),
					}
				};

				let (encoding_cipher, encoding_hmac) = {
					let (iv, rest) = local_infos.split_at(iv_size);
					let (cipher_key, mac_key) = rest.split_at(cipher_key_size);
					let hmac = SigningKey::new(&context.chosen_hash.clone().unwrap(), mac_key);
					let cipher = ctr(chosen_cipher, cipher_key, iv);
					(cipher, hmac)
				};

				let (decoding_cipher, decoding_hmac) = {
					let (iv, rest) = remote_infos.split_at(iv_size);
					let (cipher_key, mac_key) = rest.split_at(cipher_key_size);
					let hmac = VerificationKey::new(&context.chosen_hash.clone().unwrap(), mac_key);
					let cipher = ctr(chosen_cipher, cipher_key, iv);
					(cipher, hmac)
				};

				Ok(full_codec(socket, Box::new(encoding_cipher), encoding_hmac,
							  Box::new(decoding_cipher), decoding_hmac))
			});

			match codec {
				Ok(c) => Ok((c, context)),
				Err(err) => {
					debug!(target: "libp2p-secio", "failed to generate shared secret with remote");
					return Err(err);
				},
			}
		})

		// We send back their nonce to check if the connection works.
		.and_then(|(codec, mut context)| {
			let remote_nonce = mem::replace(&mut context.remote_nonce, Vec::new());
			trace!(target: "libp2p-secio", "checking encryption by sending back remote's nonce");
			codec.send(BytesMut::from(remote_nonce))
				.map(|s| (s, context))
				.from_err()
		})

		// Check that the received nonce is correct.
		.and_then(|(codec, context)| {
			codec.into_future()
				.map_err(|(e, _)| e)
				.and_then(move |(nonce, rest)| {
					match nonce {
						Some(ref n) if n == &context.local_nonce => {
							trace!(target: "libp2p-secio", "secio handshake success");
							Ok((rest, context.remote_public_key))
						},
						None => {
							debug!(target: "libp2p-secio", "unexpected eof during nonce check");
							Err(IoError::new(IoErrorKind::BrokenPipe, "unexpected eof").into())
						},
						_ => {
							debug!(target: "libp2p-secio", "failed nonce verification with remote");
							Err(SecioError::NonceVerificationFailed)
						}
					}
				})
		});

	Box::new(future)
}

// Custom algorithm translated from reference implementations. Needs to be the same algorithm
// amongst all implementations.
fn stretch_key(key: &SigningKey, result: &mut [u8]) {
	const SEED: &'static [u8] = b"key expansion";

	let mut init_ctxt = SigningContext::with_key(key);
	init_ctxt.update(SEED);
	let mut a = init_ctxt.sign();

	let mut j = 0;
	while j < result.len() {
		let mut context = SigningContext::with_key(key);
		context.update(a.as_ref());
		context.update(SEED);
		let b = context.sign();

		let todo = cmp::min(b.as_ref().len(), result.len() - j);

		result[j..j + todo].copy_from_slice(&b.as_ref()[..todo]);

		j += todo;

		let mut context = SigningContext::with_key(key);
		context.update(a.as_ref());
		a = context.sign();
	}
}

#[cfg(test)]
mod tests {
	extern crate tokio_core;
	use super::handshake;
	use super::stretch_key;
	use futures::Future;
	use futures::Stream;
	use ring::digest::SHA256;
	use ring::hmac::SigningKey;
	use ring::signature::RSAKeyPair;
	use std::sync::Arc;
	use self::tokio_core::net::TcpListener;
	use self::tokio_core::net::TcpStream;
	use self::tokio_core::reactor::Core;
	use untrusted::Input;

	#[test]
	fn handshake_with_self_succeeds() {
		let mut core = Core::new().unwrap();

		let private_key1 = {
			let pkcs8 = include_bytes!("../tests/test-private-key.pk8");
			Arc::new(RSAKeyPair::from_pkcs8(Input::from(&pkcs8[..])).unwrap())
		};
		let public_key1 = include_bytes!("../tests/test-public-key.der").to_vec();

		let private_key2 = {
			let pkcs8 = include_bytes!("../tests/test-private-key-2.pk8");
			Arc::new(RSAKeyPair::from_pkcs8(Input::from(&pkcs8[..])).unwrap())
		};
		let public_key2 = include_bytes!("../tests/test-public-key-2.der").to_vec();

		let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
		let listener_addr = listener.local_addr().unwrap();

		let server = listener.incoming()
							 .into_future()
							 .map_err(|(e, _)| e.into())
							 .and_then(move |(connec, _)| {
			handshake(connec.unwrap().0, public_key1, private_key1)
		});

		let client = TcpStream::connect(&listener_addr, &core.handle())
			.map_err(|e| e.into())
			.and_then(move |stream| handshake(stream, public_key2, private_key2));

		core.run(server.join(client)).unwrap();
	}

	#[test]
	fn stretch() {
		let mut output = [0u8; 32];

		let key1 = SigningKey::new(&SHA256, &[]);
		stretch_key(&key1, &mut output);
		assert_eq!(
			&output,
			&[
				103,
				144,
				60,
				199,
				85,
				145,
				239,
				71,
				79,
				198,
				85,
				164,
				32,
				53,
				143,
				205,
				50,
				48,
				153,
				10,
				37,
				32,
				85,
				1,
				226,
				61,
				193,
				1,
				154,
				120,
				207,
				80,
			]
		);

		let key2 = SigningKey::new(
			&SHA256,
			&[
				157,
				166,
				80,
				144,
				77,
				193,
				198,
				6,
				23,
				220,
				87,
				220,
				191,
				72,
				168,
				197,
				54,
				33,
				219,
				225,
				84,
				156,
				165,
				37,
				149,
				224,
				244,
				32,
				170,
				79,
				125,
				35,
				171,
				26,
				178,
				176,
				92,
				168,
				22,
				27,
				205,
				44,
				229,
				61,
				152,
				21,
				222,
				81,
				241,
				81,
				116,
				236,
				74,
				166,
				89,
				145,
				5,
				162,
				108,
				230,
				55,
				54,
				9,
				17,
			],
		);
		stretch_key(&key2, &mut output);
		assert_eq!(
			&output,
			&[
				39,
				151,
				182,
				63,
				180,
				175,
				224,
				139,
				42,
				131,
				130,
				116,
				55,
				146,
				62,
				31,
				157,
				95,
				217,
				15,
				73,
				81,
				10,
				83,
				243,
				141,
				64,
				227,
				103,
				144,
				99,
				121,
			]
		);

		let key3 = SigningKey::new(
			&SHA256,
			&[
				98,
				219,
				94,
				104,
				97,
				70,
				139,
				13,
				185,
				110,
				56,
				36,
				66,
				3,
				80,
				224,
				32,
				205,
				102,
				170,
				59,
				32,
				140,
				245,
				86,
				102,
				231,
				68,
				85,
				249,
				227,
				243,
				57,
				53,
				171,
				36,
				62,
				225,
				178,
				74,
				89,
				142,
				151,
				94,
				183,
				231,
				208,
				166,
				244,
				130,
				130,
				209,
				248,
				65,
				19,
				48,
				127,
				127,
				55,
				82,
				117,
				154,
				124,
				108,
			],
		);
		stretch_key(&key3, &mut output);
		assert_eq!(
			&output,
			&[
				28,
				39,
				158,
				206,
				164,
				16,
				211,
				194,
				99,
				43,
				208,
				36,
				24,
				141,
				90,
				93,
				157,
				236,
				238,
				111,
				170,
				0,
				60,
				11,
				49,
				174,
				177,
				121,
				30,
				12,
				182,
				25,
			]
		);
	}
}
