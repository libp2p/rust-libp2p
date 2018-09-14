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
use stream_cipher::{Cipher, ctr};
use error::SecioError;
use futures::future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::Future;
use libp2p_core::PublicKey;
use protobuf::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::Message as ProtobufMessage;
use ring::agreement::EphemeralPrivateKey;
use ring::hmac::{SigningContext, SigningKey, VerificationKey};
use ring::rand::SecureRandom;
use ring::signature::verify as signature_verify;
use ring::signature::{ED25519, RSASigningState, RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_SHA256};
use ring::{agreement, digest, rand};
#[cfg(feature = "secp256k1")]
use secp256k1;
use std::cmp::{self, Ordering};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use structs_proto::{Exchange, Propose};
use tokio_io::codec::length_delimited;
use tokio_io::{AsyncRead, AsyncWrite};
use untrusted::Input as UntrustedInput;
use {SecioConfig, SecioKeyPairInner};

/// Performs a handshake on the given socket.
///
/// This function expects that the remote is identified with `remote_public_key`, and the remote
/// will expect that we are identified with `local_key`.Any mismatch somewhere will produce a
/// `SecioError`.
///
/// On success, returns an object that implements the `Sink` and `Stream` trait whose items are
/// buffers of data, plus the public key of the remote, plus the ephemeral public key used during
/// negotiation.
pub fn handshake<'a, S: 'a>(
    socket: S,
    config: SecioConfig
) -> Box<Future<Item = (FullCodec<S>, PublicKey, Vec<u8>), Error = SecioError> + Send + 'a>
where
    S: AsyncRead + AsyncWrite + Send,
{
    // TODO: could be rewritten as a coroutine once coroutines land in stable Rust

    // This struct contains the whole context of a handshake, and is filled progressively
    // throughout the various parts of the handshake.
    struct HandshakeContext {
        // Filled with this function's parameter.
        config: SecioConfig,

        rng: rand::SystemRandom,
        // Locally-generated random number. The array size can be changed without any repercussion.
        local_nonce: [u8; 16],

        // Our local proposition's raw bytes.
        local_public_key_in_protobuf_bytes: Vec<u8>,
        local_proposition_bytes: Vec<u8>,

        // The remote proposition's raw bytes.
        remote_proposition_bytes: BytesMut,
        remote_public_key_in_protobuf_bytes: Vec<u8>,
        remote_public_key: Option<PublicKey>,

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
        chosen_cipher: Option<Cipher>,
        chosen_hash: Option<&'static digest::Algorithm>,

        // Ephemeral key generated for the handshake and then thrown away.
        local_tmp_priv_key: Option<EphemeralPrivateKey>,
        local_tmp_pub_key: Vec<u8>,
    }

    let context = HandshakeContext {
        config,
        rng: rand::SystemRandom::new(),
        local_nonce: Default::default(),
        local_public_key_in_protobuf_bytes: Vec::new(),
        local_proposition_bytes: Vec::new(),
        remote_proposition_bytes: BytesMut::new(),
        remote_public_key_in_protobuf_bytes: Vec::new(),
        remote_public_key: None,
        remote_nonce: Vec::new(),
        hashes_ordering: Ordering::Equal,
        chosen_exchange: None,
        chosen_cipher: None,
        chosen_hash: None,
        local_tmp_priv_key: None,
        local_tmp_pub_key: Vec::new(),
    };

    // The handshake messages all start with a 4-bytes message length prefix.
    let socket = length_delimited::Builder::new()
        .big_endian()
        .length_field_length(4)
        .new_framed(socket);

    let future = future::ok::<_, SecioError>(context)
        // Generate our nonce.
        .and_then(|mut context| {
            context.rng.fill(&mut context.local_nonce)
                .map_err(|_| SecioError::NonceGenerationFailed)?;
            trace!("starting handshake ; local nonce = {:?}", context.local_nonce);
            Ok(context)
        })

        // Send our proposition with our nonce, public key and supported protocols.
        .and_then(|mut context| {
            context.local_public_key_in_protobuf_bytes = context.config.key.to_public_key().into_protobuf_encoding();

            let mut proposition = Propose::new();
            proposition.set_rand(context.local_nonce.to_vec());
            proposition.set_pubkey(context.local_public_key_in_protobuf_bytes.clone());

            if let Some(ref p) = context.config.agreements_prop {
                trace!("agreements proposition: {}", p);
                proposition.set_exchanges(p.clone())
            } else {
                trace!("agreements proposition: {}", algo_support::DEFAULT_AGREEMENTS_PROPOSITION);
                proposition.set_exchanges(algo_support::DEFAULT_AGREEMENTS_PROPOSITION.into())
            }

            if let Some(ref p) = context.config.ciphers_prop {
                trace!("ciphers proposition: {}", p);
                proposition.set_ciphers(p.clone())
            } else {
                trace!("ciphers proposition: {}", algo_support::DEFAULT_CIPHERS_PROPOSITION);
                proposition.set_ciphers(algo_support::DEFAULT_CIPHERS_PROPOSITION.into())
            }

            if let Some(ref p) = context.config.digests_prop {
                trace!("digests proposition: {}", p);
                proposition.set_hashes(p.clone())
            } else {
                trace!("digests proposition: {}", algo_support::DEFAULT_DIGESTS_PROPOSITION);
                proposition.set_hashes(algo_support::DEFAULT_DIGESTS_PROPOSITION.into())
            }

            let proposition_bytes = proposition.write_to_bytes().unwrap();
            context.local_proposition_bytes = proposition_bytes.clone();

            trace!("sending proposition to remote");

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
                            debug!("unexpected eof while waiting for remote's proposition");
                            return Err(err.into())
                        },
                    };

                    let mut prop = match protobuf_parse_from_bytes::<Propose>(
                        &context.remote_proposition_bytes
                    ) {
                        Ok(prop) => prop,
                        Err(_) => {
                            debug!("failed to parse remote's proposition protobuf message");
                            return Err(SecioError::HandshakeParsingFailure);
                        }
                    };
                    context.remote_public_key_in_protobuf_bytes = prop.take_pubkey();
                    let pubkey = match PublicKey::from_protobuf_encoding(&context.remote_public_key_in_protobuf_bytes) {
                        Ok(p) => p,
                        Err(_) => {
                            debug!("failed to parse remote's proposition's pubkey protobuf");
                            return Err(SecioError::HandshakeParsingFailure);
                        },
                    };

                    context.remote_nonce = prop.take_rand();
                    context.remote_public_key = Some(pubkey);
                    trace!("received proposition from remote ; pubkey = {:?} ; nonce = {:?}",
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
                let ours = context.config.agreements_prop.as_ref()
                    .map(|s| s.as_ref())
                    .unwrap_or(algo_support::DEFAULT_AGREEMENTS_PROPOSITION);
                let theirs = &remote_prop.get_exchanges();
                Some(match algo_support::select_agreement(context.hashes_ordering, ours, theirs) {
                    Ok(a) => a,
                    Err(err) => {
                        debug!("failed to select an exchange protocol");
                        return Err(err);
                    }
                })
            };
            context.chosen_cipher = {
                let ours = context.config.ciphers_prop.as_ref()
                    .map(|s| s.as_ref())
                    .unwrap_or(algo_support::DEFAULT_CIPHERS_PROPOSITION);
                let theirs = &remote_prop.get_ciphers();
                Some(match algo_support::select_cipher(context.hashes_ordering, ours, theirs) {
                    Ok(a) => {
                        debug!("selected cipher: {:?}", a);
                        a
                    }
                    Err(err) => {
                        debug!("failed to select a cipher protocol");
                        return Err(err);
                    }
                })
            };
            context.chosen_hash = {
                let ours = context.config.digests_prop.as_ref()
                    .map(|s| s.as_ref())
                    .unwrap_or(algo_support::DEFAULT_DIGESTS_PROPOSITION);
                let theirs = &remote_prop.get_hashes();
                Some(match algo_support::select_digest(context.hashes_ordering, ours, theirs) {
                    Ok(a) => {
                        debug!("selected hash: {:?}", a);
                        a
                    }
                    Err(err) => {
                        debug!("failed to select a hash protocol");
                        return Err(err);
                    }
                })
            };

            Ok((socket, context))
        })

        // Generate an ephemeral key for the negotiation.
        .and_then(|(socket, context)| {
            match EphemeralPrivateKey::generate(context.chosen_exchange.as_ref().unwrap(), &context.rng) {
                Ok(tmp_priv_key) => Ok((socket, context, tmp_priv_key)),
                Err(_) => {
                    debug!("failed to generate ECDH key");
                    Err(SecioError::EphemeralKeyGenerationFailed)
                },
            }
        })

        // Send the ephemeral pub key to the remote in an `Exchange` struct. The `Exchange` also
        // contains a signature of the two propositions encoded with our static public key.
        .and_then(|(socket, mut context, tmp_priv)| {
            let exchange = {
                let mut local_tmp_pub_key: Vec<u8> = (0 .. tmp_priv.public_key_len()).map(|_| 0).collect();
                tmp_priv.compute_public_key(&mut local_tmp_pub_key).unwrap();
                context.local_tmp_priv_key = Some(tmp_priv);

                let mut data_to_sign = context.local_proposition_bytes.clone();
                data_to_sign.extend_from_slice(&context.remote_proposition_bytes);
                data_to_sign.extend_from_slice(&local_tmp_pub_key);

                let mut exchange = Exchange::new();
                exchange.set_epubkey(local_tmp_pub_key.clone());
                exchange.set_signature({
                    match context.config.key.inner {
                        SecioKeyPairInner::Rsa { ref private, .. } => {
                            let mut state = match RSASigningState::new(private.clone()) {
                                Ok(s) => s,
                                Err(_) => {
                                    debug!("failed to sign local exchange");
                                    return Err(SecioError::SigningFailure);
                                },
                            };
                            let mut signature = vec![0; private.public_modulus_len()];
                            match state.sign(&RSA_PKCS1_SHA256, &context.rng, &data_to_sign,
                                             &mut signature)
                            {
                                Ok(_) => (),
                                Err(_) => {
                                    debug!("failed to sign local exchange");
                                    return Err(SecioError::SigningFailure);
                                },
                            };

                            signature
                        },
                        SecioKeyPairInner::Ed25519 { ref key_pair } => {
                            let signature = key_pair.sign(&data_to_sign);
                            signature.as_ref().to_owned()
                        },
                        #[cfg(feature = "secp256k1")]
                        SecioKeyPairInner::Secp256k1 { ref private } => {
                            let data_to_sign = digest::digest(&digest::SHA256, &data_to_sign);
                            let message = secp256k1::Message::from_slice(data_to_sign.as_ref())
                                .expect("digest output length doesn't match secp256k1 input length");
                            let secp256k1 = secp256k1::Secp256k1::with_caps(secp256k1::ContextFlag::SignOnly);
                            secp256k1
                                .sign(&message, private)
                                .expect("failed to sign message")
                                .serialize_der(&secp256k1)
                        },
                    }
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
            trace!("sending exchange to remote");
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
                            debug!("unexpected eof while waiting for remote's exchange");
                            return Err(err.into())
                        },
                    };

                    let remote_exch = match protobuf_parse_from_bytes::<Exchange>(&raw) {
                        Ok(e) => e,
                        Err(err) => {
                            debug!("failed to parse remote's exchange protobuf ; {:?}", err);
                            return Err(SecioError::HandshakeParsingFailure);
                        }
                    };

                    trace!("received and decoded the remote's exchange");
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

            match context.remote_public_key {
                Some(PublicKey::Rsa(ref remote_public_key)) => {
                    // TODO: The ring library doesn't like some stuff in our DER public key,
                    //       therefore we scrap the first 24 bytes of the key. A proper fix would
                    //       be to write a DER parser, but that's not trivial.
                    match signature_verify(&RSA_PKCS1_2048_8192_SHA256,
                                           UntrustedInput::from(&remote_public_key[24..]),
                                           UntrustedInput::from(&data_to_verify),
                                           UntrustedInput::from(remote_exch.get_signature()))
                    {
                        Ok(()) => (),
                        Err(_) => {
                            debug!("failed to verify the remote's signature");
                            return Err(SecioError::SignatureVerificationFailed)
                        },
                    }
                },
                Some(PublicKey::Ed25519(ref remote_public_key)) => {
                    match signature_verify(&ED25519,
                                           UntrustedInput::from(remote_public_key),
                                           UntrustedInput::from(&data_to_verify),
                                           UntrustedInput::from(remote_exch.get_signature()))
                    {
                        Ok(()) => (),
                        Err(_) => {
                            debug!("failed to verify the remote's signature");
                            return Err(SecioError::SignatureVerificationFailed)
                        },
                    }
                },
                #[cfg(feature = "secp256k1")]
                Some(PublicKey::Secp256k1(ref remote_public_key)) => {
                    let data_to_verify = digest::digest(&digest::SHA256, &data_to_verify);
                    let message = secp256k1::Message::from_slice(data_to_verify.as_ref())
                        .expect("digest output length doesn't match secp256k1 input length");
                    let secp256k1 = secp256k1::Secp256k1::with_caps(secp256k1::ContextFlag::VerifyOnly);
                    let signature = secp256k1::Signature::from_der(&secp256k1, remote_exch.get_signature());
                    let remote_public_key = secp256k1::key::PublicKey::from_slice(&secp256k1, remote_public_key);
                    if let (Ok(signature), Ok(remote_public_key)) = (signature, remote_public_key) {
                        match secp256k1.verify(&message, &signature, &remote_public_key) {
                            Ok(()) => (),
                            Err(_) => {
                                debug!("failed to verify the remote's signature");
                                return Err(SecioError::SignatureVerificationFailed)
                            },
                        }
                    } else {
                        debug!("remote's secp256k1 signature has wrong format");
                        return Err(SecioError::SignatureVerificationFailed)
                    }
                },
                #[cfg(not(feature = "secp256k1"))]
                Some(PublicKey::Secp256k1(_)) => {
                    debug!("support for secp256k1 was disabled at compile-time");
                    return Err(SecioError::SignatureVerificationFailed);
                },
                None => unreachable!("we store a Some in the remote public key before reaching \
                                      this point")
            };

            trace!("successfully verified the remote's signature");
            Ok((remote_exch, socket, context))
        })

        // Generate a key from the local ephemeral private key and the remote ephemeral public key,
        // derive from it a ciper key, an iv, and a hmac key, and build the encoder/decoder.
        .and_then(|(remote_exch, socket, mut context)| {
            let local_priv_key = context.local_tmp_priv_key.take()
                .expect("we filled this Option earlier, and extract it now");
            let codec = agreement::agree_ephemeral(local_priv_key,
                                                   &context.chosen_exchange.unwrap(),
                                                   UntrustedInput::from(remote_exch.get_epubkey()),
                                                   SecioError::SecretGenerationFailed,
                                                   |key_material| {
                let key = SigningKey::new(context.chosen_hash.unwrap(), key_material);

                let chosen_cipher = context.chosen_cipher.unwrap();
                let cipher_key_size = chosen_cipher.key_size();
                let iv_size = chosen_cipher.iv_size();

                let mut longer_key = vec![0u8; 2 * (iv_size + cipher_key_size + 20)];
                stretch_key(&key, &mut longer_key);

                let (local_infos, remote_infos) = {
                    let (first_half, second_half) = longer_key.split_at(longer_key.len() / 2);
                    match context.hashes_ordering {
                        Ordering::Equal => {
                            let msg = "equal digest of public key and nonce for local and remote";
                            return Err(SecioError::InvalidProposition(msg))
                        }
                        Ordering::Less => (second_half, first_half),
                        Ordering::Greater => (first_half, second_half),
                    }
                };

                let (encoding_cipher, encoding_hmac) = {
                    let (iv, rest) = local_infos.split_at(iv_size);
                    let (cipher_key, mac_key) = rest.split_at(cipher_key_size);
                    let hmac = SigningKey::new(&context.chosen_hash.unwrap(), mac_key);
                    let cipher = ctr(chosen_cipher, cipher_key, iv);
                    (cipher, hmac)
                };

                let (decoding_cipher, decoding_hmac) = {
                    let (iv, rest) = remote_infos.split_at(iv_size);
                    let (cipher_key, mac_key) = rest.split_at(cipher_key_size);
                    let hmac = VerificationKey::new(&context.chosen_hash.unwrap(), mac_key);
                    let cipher = ctr(chosen_cipher, cipher_key, iv);
                    (cipher, hmac)
                };

                Ok(full_codec(socket, encoding_cipher, encoding_hmac, decoding_cipher, decoding_hmac))
            });

            match codec {
                Ok(c) => Ok((c, context)),
                Err(err) => {
                    debug!("failed to generate shared secret with remote");
                    Err(err)
                },
            }
        })

        // We send back their nonce to check if the connection works.
        .and_then(|(codec, mut context)| {
            let remote_nonce = mem::replace(&mut context.remote_nonce, Vec::new());
            trace!("checking encryption by sending back remote's nonce");
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
                            trace!("secio handshake success");
                            Ok((rest, context.remote_public_key.expect("we stored a Some earlier"), context.local_tmp_pub_key))
                        },
                        None => {
                            debug!("unexpected eof during nonce check");
                            Err(IoError::new(IoErrorKind::BrokenPipe, "unexpected eof").into())
                        },
                        _ => {
                            debug!("failed nonce verification with remote");
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
    const SEED: &[u8] = b"key expansion";

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
    extern crate tokio_current_thread;
    extern crate tokio_tcp;
    use self::tokio_tcp::TcpListener;
    use self::tokio_tcp::TcpStream;
    use super::handshake;
    use super::stretch_key;
    use futures::Future;
    use futures::Stream;
    use ring::digest::SHA256;
    use ring::hmac::SigningKey;
    use {SecioConfig, SecioKeyPair};

    #[test]
    fn handshake_with_self_succeeds_rsa() {
        let key1 = {
            let private = include_bytes!("../tests/test-rsa-private-key.pk8");
            let public = include_bytes!("../tests/test-rsa-public-key.der").to_vec();
            SecioKeyPair::rsa_from_pkcs8(private, public).unwrap()
        };

        let key2 = {
            let private = include_bytes!("../tests/test-rsa-private-key-2.pk8");
            let public = include_bytes!("../tests/test-rsa-public-key-2.der").to_vec();
            SecioKeyPair::rsa_from_pkcs8(private, public).unwrap()
        };

        handshake_with_self_succeeds(SecioConfig::new(key1), SecioConfig::new(key2));
    }

    #[test]
    fn handshake_with_self_succeeds_ed25519() {
        let key1 = SecioKeyPair::ed25519_generated().unwrap();
        let key2 = SecioKeyPair::ed25519_generated().unwrap();
        handshake_with_self_succeeds(SecioConfig::new(key1), SecioConfig::new(key2));
    }

    #[test]
    #[cfg(feature = "secp256k1")]
    fn handshake_with_self_succeeds_secp256k1() {
        let key1 = {
            let key = include_bytes!("../tests/test-secp256k1-private-key.der");
            SecioKeyPair::secp256k1_from_der(&key[..]).unwrap()
        };

        let key2 = {
            let key = include_bytes!("../tests/test-secp256k1-private-key-2.der");
            SecioKeyPair::secp256k1_from_der(&key[..]).unwrap()
        };

        handshake_with_self_succeeds(SecioConfig::new(key1), SecioConfig::new(key2));
    }

    fn handshake_with_self_succeeds(key1: SecioConfig, key2: SecioConfig) {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(move |(connec, _)| handshake(connec.unwrap(), key1));

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .and_then(move |stream| handshake(stream, key2));

        tokio_current_thread::block_on_all(server.join(client)).unwrap();
    }

    #[test]
    fn stretch() {
        let mut output = [0u8; 32];

        let key1 = SigningKey::new(&SHA256, &[]);
        stretch_key(&key1, &mut output);
        assert_eq!(
            &output,
            &[
                103, 144, 60, 199, 85, 145, 239, 71, 79, 198, 85, 164, 32, 53, 143, 205, 50, 48,
                153, 10, 37, 32, 85, 1, 226, 61, 193, 1, 154, 120, 207, 80,
            ]
        );

        let key2 = SigningKey::new(
            &SHA256,
            &[
                157, 166, 80, 144, 77, 193, 198, 6, 23, 220, 87, 220, 191, 72, 168, 197, 54, 33,
                219, 225, 84, 156, 165, 37, 149, 224, 244, 32, 170, 79, 125, 35, 171, 26, 178, 176,
                92, 168, 22, 27, 205, 44, 229, 61, 152, 21, 222, 81, 241, 81, 116, 236, 74, 166,
                89, 145, 5, 162, 108, 230, 55, 54, 9, 17,
            ],
        );
        stretch_key(&key2, &mut output);
        assert_eq!(
            &output,
            &[
                39, 151, 182, 63, 180, 175, 224, 139, 42, 131, 130, 116, 55, 146, 62, 31, 157, 95,
                217, 15, 73, 81, 10, 83, 243, 141, 64, 227, 103, 144, 99, 121,
            ]
        );

        let key3 = SigningKey::new(
            &SHA256,
            &[
                98, 219, 94, 104, 97, 70, 139, 13, 185, 110, 56, 36, 66, 3, 80, 224, 32, 205, 102,
                170, 59, 32, 140, 245, 86, 102, 231, 68, 85, 249, 227, 243, 57, 53, 171, 36, 62,
                225, 178, 74, 89, 142, 151, 94, 183, 231, 208, 166, 244, 130, 130, 209, 248, 65,
                19, 48, 127, 127, 55, 82, 117, 154, 124, 108,
            ],
        );
        stretch_key(&key3, &mut output);
        assert_eq!(
            &output,
            &[
                28, 39, 158, 206, 164, 16, 211, 194, 99, 43, 208, 36, 24, 141, 90, 93, 157, 236,
                238, 111, 170, 0, 60, 11, 49, 174, 177, 121, 30, 12, 182, 25,
            ]
        );
    }
}
