

use algo_support;
use bytes::BytesMut;
use codec::{full_codec, FullCodec};
use crypto::aes::{ctr, KeySize};
use error::SecioError;
use futures::Future;
use futures::future;
use futures::sink::Sink;
use futures::stream::Stream;
use protobuf::Message as ProtobufMessage;
use ring::{agreement, digest, rand};
use ring::hkdf::expand as hkdf_expand;
use ring::hmac::{SigningKey, VerificationKey};
use ring::rand::SecureRandom;
use ring::signature::{Ed25519KeyPair, ED25519, verify as signature_verify};
use std::cmp::Ordering;
use std::mem;
use std::sync::Arc;
use structs_proto::{Propose, Exchange};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::io::{flush, write_all};
use untrusted::Input as UntrustedInput;
use util::read_protobuf_message;

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
	local_private_key: Arc<Ed25519KeyPair>,
) -> Box<Future<Item = (FullCodec<S>, Vec<u8>), Error = SecioError> + 'a>
where
	S: AsyncRead + AsyncWrite,
{
	// TODO: could be rewritten as a coroutine once coroutines land in stable Rust

	// This struct contains the whole context of a handshake, and is filled progressively
	// throughout the various parts of the handshake.
	struct HandshakeContext {
		// Filled with this function's parameters.
		local_public_key: Vec<u8>,
		local_private_key: Arc<Ed25519KeyPair>,

		rng: rand::SystemRandom,
		// Locally-generated random number. The array size can be changed without any repercussion.
		local_nonce: [u8; 16],

		// Our local proposition's raw bytes.
		local_proposition_bytes: Vec<u8>,

		// The remote proposition's raw bytes.
		remote_proposition_bytes: Vec<u8>,
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
		local_tmp_priv_key: Option<agreement::EphemeralPrivateKey>,
		local_tmp_pub_key: [u8; agreement::PUBLIC_KEY_MAX_LEN],
	}

	let context = HandshakeContext {
		local_public_key: local_public_key,
		local_private_key: local_private_key,
		rng: rand::SystemRandom::new(),
		local_nonce: Default::default(),
		local_proposition_bytes: Vec::new(),
		remote_proposition_bytes: Vec::new(),
		remote_public_key: Vec::new(),
		remote_nonce: Vec::new(),
		hashes_ordering: Ordering::Equal,
		chosen_exchange: None,
		chosen_cipher: None,
		chosen_hash: None,
		local_tmp_priv_key: None,
		local_tmp_pub_key: [0; agreement::PUBLIC_KEY_MAX_LEN],
	};

	let future = future::ok::<_, SecioError>(context)
        // Generate our nonce.
        .and_then(|mut context| {
            context.rng.fill(&mut context.local_nonce)
                .map_err(|_| SecioError::NonceGenerationFailed)?;
            Ok(context)
        })

        // Send our proposition with our nonce, public key and supported protocols.
        .and_then(|mut context| {
            let mut proposition = Propose::new();
            proposition.set_rand(context.local_nonce.clone().to_vec());
            proposition.set_pubkey(context.local_public_key.clone());
            proposition.set_exchanges(algo_support::exchanges::PROPOSITION_STRING.into());
            proposition.set_ciphers(algo_support::ciphers::PROPOSITION_STRING.into());
            proposition.set_hashes(algo_support::hashes::PROPOSITION_STRING.into());
            let proposition_bytes = proposition.write_to_bytes().unwrap();

            write_all(socket, proposition_bytes)
                .and_then(move |(socket, proposition_bytes)| {
                    context.local_proposition_bytes = proposition_bytes;
                    flush(socket).map(|s| (s, context))
                })
                .from_err()
        })

        // Receive the remote's proposition.
        .and_then(move |(socket, mut context)| {
            read_protobuf_message::<Propose, _>(socket)
                .map(move |(prop, prop_raw, socket)| {
                    context.remote_proposition_bytes = prop_raw;
                    (prop, socket, context)
                })
                .from_err()
        })

        // Decide which algorithms to use (thanks to the remote's proposition), then generate an
        // ephemeral key.
        .and_then(move |(mut remote_prop, socket, mut context)| {
            let oh1 = {
                let mut ctx = digest::Context::new(&digest::SHA256);
                ctx.update(&remote_prop.get_pubkey());
                ctx.update(&context.local_nonce);
                ctx.finish()
            };

            let oh2 = {
                let mut ctx = digest::Context::new(&digest::SHA256);
                ctx.update(&context.local_public_key);
                ctx.update(&remote_prop.get_rand());
                ctx.finish()
            };

            context.hashes_ordering = oh1.as_ref().cmp(&oh2.as_ref());

            context.remote_public_key = remote_prop.take_pubkey();
            context.remote_nonce = remote_prop.take_rand();
            context.chosen_exchange = Some(algo_support::exchanges::select_best(context.hashes_ordering, &remote_prop.get_exchanges())?);
            context.chosen_cipher = Some(algo_support::ciphers::select_best(context.hashes_ordering, &remote_prop.get_ciphers())?);
            context.chosen_hash = Some(algo_support::hashes::select_best(context.hashes_ordering, &remote_prop.get_hashes())?);

            let tmp_priv_key = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &context.rng).map_err(|_| SecioError::EphemeralKeyGenerationFailed)?;

            Ok((socket, context, tmp_priv_key))
        })

        // Send the ephemeral pub key to the remote in an `Exchange` struct. The `Exchange` also
        // contains a signature of the two propositions encoded with our static public key.
        .and_then(|(socket, mut context, tmp_priv_key)| {
            let exchange_bytes = {
                let local_tmp_pub_key = &mut context.local_tmp_pub_key[..tmp_priv_key.public_key_len()];
                tmp_priv_key.compute_public_key(local_tmp_pub_key).unwrap();
                context.local_tmp_priv_key = Some(tmp_priv_key);

                let mut data_to_sign = context.local_proposition_bytes.clone();
                data_to_sign.extend_from_slice(&context.remote_proposition_bytes);
                data_to_sign.extend_from_slice(local_tmp_pub_key);

                let mut exchange = Exchange::new();
                exchange.set_epubkey(local_tmp_pub_key.to_vec());
                exchange.set_signature({
                    let sign = context.local_private_key.sign(&data_to_sign);
                    sign.as_ref().to_vec()
                });
                exchange.write_to_bytes().unwrap()
            };

            write_all(socket, exchange_bytes)
                .and_then(|(socket, _)| flush(socket))
                .map(move |socket| (socket, context))
                .from_err()
        })

        // Receive the remote's `Exchange`.
        .and_then(move |(socket, context)| {
            read_protobuf_message::<Exchange, _>(socket)
                .map(|(remote_exch, _, rest)| (remote_exch, rest, context))
                .from_err()
        })

        // Check the validity of the remote's `Exchange`. This verifies that the remote was really
        // the sender of its proposition, and that it is the owner of both its global and ephemeral
        // keys. Then generate a key from the local ephemeral private key and the remote ephemeral
        // public key, derive from it a ciper key, an iv, and a hmac key, and build the
        // encoder/decoder.
        .and_then(|(remote_exch, socket, mut context)| {
            let mut data_to_verify = context.remote_proposition_bytes.clone();
            data_to_verify.extend_from_slice(&context.local_proposition_bytes);
            data_to_verify.extend_from_slice(remote_exch.get_epubkey());

            match signature_verify(&ED25519,
                                   UntrustedInput::from(&context.remote_public_key),
                                   UntrustedInput::from(&data_to_verify),
                                   UntrustedInput::from(remote_exch.get_signature()))
            {
                Ok(()) => (),
                Err(_) => return Err(SecioError::SignatureVerificationFailed),
            }

            let local_priv_key = context.local_tmp_priv_key.take().unwrap();
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
                hkdf_expand(&key, b"key expansion", &mut longer_key);

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
            })?;

            Ok((codec, context))
        })

        // We send back their nonce to check if the connection works.
        .and_then(|(codec, mut context)| {
            let remote_nonce = mem::replace(&mut context.remote_nonce, Vec::new());
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
                            Ok((rest, context.remote_public_key))
                        },
                        _ => Err(SecioError::NonceVerificationFailed)
                    }
                })
        });

	Box::new(future)
}

#[cfg(test)]
mod tests {
	use super::handshake;
	use futures::Future;
	use futures::Stream;
	use ring::rand::SystemRandom;
	use ring::signature::Ed25519KeyPair;
	use std::sync::Arc;
	use tokio_core::net::TcpListener;
	use tokio_core::net::TcpStream;
	use tokio_core::reactor::Core;
	use untrusted::Input;

	#[test]
	fn handshake_with_self_succeeds() {
		let mut core = Core::new().unwrap();

		let rng = SystemRandom::new();

		let private_key1 = {
			let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
			Arc::new(Ed25519KeyPair::from_pkcs8(Input::from(&pkcs8[..])).unwrap())
		};
		let public_key1 = private_key1.public_key_bytes().to_vec();

		let private_key2 = {
			let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
			Arc::new(Ed25519KeyPair::from_pkcs8(Input::from(&pkcs8[..])).unwrap())
		};
		let public_key2 = private_key2.public_key_bytes().to_vec();

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
}
