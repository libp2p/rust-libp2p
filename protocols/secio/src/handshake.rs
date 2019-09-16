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

use crate::algo_support;
use crate::codec::{full_codec, FullCodec, Hmac};
use crate::stream_cipher::ctr;
use crate::error::SecioError;
use crate::exchange;
use futures::prelude::*;
use libp2p_core::PublicKey;
use log::{debug, trace};
use protobuf::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::Message as ProtobufMessage;
use rand::{self, RngCore};
use sha2::{Digest as ShaDigestTrait, Sha256};
use std::cmp::{self, Ordering};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use crate::structs_proto::{Exchange, Propose};
use crate::SecioConfig;

/// Performs a handshake on the given socket.
///
/// This function expects that the remote is identified with `remote_public_key`, and the remote
/// will expect that we are identified with `local_key`. Any mismatch somewhere will produce a
/// `SecioError`.
///
/// On success, returns an object that implements the `Sink` and `Stream` trait whose items are
/// buffers of data, plus the public key of the remote, plus the ephemeral public key used during
/// negotiation.
pub async fn handshake<'a, S: 'a>(socket: S, config: SecioConfig)
    -> Result<(FullCodec<S>, PublicKey, Vec<u8>), SecioError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    // The handshake messages all start with a variable-length integer indicating the size.
    let mut socket = futures_codec::Framed::new(
        socket,
        unsigned_varint::codec::UviBytes::<Vec<u8>>::default()
    );

    let local_nonce = {
        let mut local_nonce = [0; 16];
        rand::thread_rng()
            .try_fill_bytes(&mut local_nonce)
            .map_err(|_| SecioError::NonceGenerationFailed)?;
        local_nonce
    };

    let local_public_key_encoded = config.key.public().into_protobuf_encoding();

    // Send our proposition with our nonce, public key and supported protocols.
    let mut local_proposition = Propose::new();
    local_proposition.set_rand(local_nonce.to_vec());
    local_proposition.set_pubkey(local_public_key_encoded.clone());

    if let Some(ref p) = config.agreements_prop {
        trace!("agreements proposition: {}", p);
        local_proposition.set_exchanges(p.clone())
    } else {
        trace!("agreements proposition: {}", algo_support::DEFAULT_AGREEMENTS_PROPOSITION);
        local_proposition.set_exchanges(algo_support::DEFAULT_AGREEMENTS_PROPOSITION.into())
    }

    if let Some(ref p) = config.ciphers_prop {
        trace!("ciphers proposition: {}", p);
        local_proposition.set_ciphers(p.clone())
    } else {
        trace!("ciphers proposition: {}", algo_support::DEFAULT_CIPHERS_PROPOSITION);
        local_proposition.set_ciphers(algo_support::DEFAULT_CIPHERS_PROPOSITION.into())
    }

    if let Some(ref p) = config.digests_prop {
        trace!("digests proposition: {}", p);
        local_proposition.set_hashes(p.clone())
    } else {
        trace!("digests proposition: {}", algo_support::DEFAULT_DIGESTS_PROPOSITION);
        local_proposition.set_hashes(algo_support::DEFAULT_DIGESTS_PROPOSITION.into())
    }

    let local_proposition_bytes = local_proposition.write_to_bytes()?;
    trace!("starting handshake; local nonce = {:?}", local_nonce);

    trace!("sending proposition to remote");
    socket.send(local_proposition_bytes.clone()).await?;

    // Receive the remote's proposition.
    let remote_proposition_bytes = match socket.next().await {
        Some(b) => b?,
        None => {
            let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
            debug!("unexpected eof while waiting for remote's proposition");
            return Err(err.into())
        },
    };

    let mut remote_proposition = match protobuf_parse_from_bytes::<Propose>(&remote_proposition_bytes) {
        Ok(prop) => prop,
        Err(_) => {
            debug!("failed to parse remote's proposition protobuf message");
            return Err(SecioError::HandshakeParsingFailure);
        }
    };

    let remote_public_key_encoded = remote_proposition.take_pubkey();
    let remote_nonce = remote_proposition.take_rand();

    let remote_public_key = match PublicKey::from_protobuf_encoding(&remote_public_key_encoded) {
        Ok(p) => p,
        Err(_) => {
            debug!("failed to parse remote's proposition's pubkey protobuf");
            return Err(SecioError::HandshakeParsingFailure);
        },
    };
    trace!("received proposition from remote; pubkey = {:?}; nonce = {:?}",
        remote_public_key, remote_nonce);

    // In order to determine which protocols to use, we compute two hashes and choose
    // based on which hash is larger.
    let hashes_ordering = {
        let oh1 = {
            let mut ctx = Sha256::new();
            ctx.input(&remote_public_key_encoded);
            ctx.input(&local_nonce);
            ctx.result()
        };

        let oh2 = {
            let mut ctx = Sha256::new();
            ctx.input(&local_public_key_encoded);
            ctx.input(&remote_nonce);
            ctx.result()
        };

        oh1.as_ref().cmp(&oh2.as_ref())
    };

    let chosen_exchange = {
        let ours = config.agreements_prop.as_ref()
            .map(|s| s.as_ref())
            .unwrap_or(algo_support::DEFAULT_AGREEMENTS_PROPOSITION);
        let theirs = &remote_proposition.get_exchanges();
        match algo_support::select_agreement(hashes_ordering, ours, theirs) {
            Ok(a) => a,
            Err(err) => {
                debug!("failed to select an exchange protocol");
                return Err(err);
            }
        }
    };

    let chosen_cipher = {
        let ours = config.ciphers_prop.as_ref()
            .map(|s| s.as_ref())
            .unwrap_or(algo_support::DEFAULT_CIPHERS_PROPOSITION);
        let theirs = &remote_proposition.get_ciphers();
        match algo_support::select_cipher(hashes_ordering, ours, theirs) {
            Ok(a) => {
                debug!("selected cipher: {:?}", a);
                a
            }
            Err(err) => {
                debug!("failed to select a cipher protocol");
                return Err(err);
            }
        }
    };

    let chosen_hash = {
        let ours = config.digests_prop.as_ref()
            .map(|s| s.as_ref())
            .unwrap_or(algo_support::DEFAULT_DIGESTS_PROPOSITION);
        let theirs = &remote_proposition.get_hashes();
        match algo_support::select_digest(hashes_ordering, ours, theirs) {
            Ok(a) => {
                debug!("selected hash: {:?}", a);
                a
            }
            Err(err) => {
                debug!("failed to select a hash protocol");
                return Err(err);
            }
        }
    };

    // Generate an ephemeral key for the negotiation.
    let (tmp_priv_key, tmp_pub_key) = exchange::generate_agreement(chosen_exchange).await?;

    // Send the ephemeral pub key to the remote in an `Exchange` struct. The `Exchange` also
    // contains a signature of the two propositions encoded with our static public key.
    let local_exchange = {
        let mut data_to_sign = local_proposition_bytes.clone();
        data_to_sign.extend_from_slice(&remote_proposition_bytes);
        data_to_sign.extend_from_slice(&tmp_pub_key);

        let mut exchange = Exchange::new();
        exchange.set_epubkey(tmp_pub_key.clone());
        match config.key.sign(&data_to_sign) {
            Ok(sig) => exchange.set_signature(sig),
            Err(_) => return Err(SecioError::SigningFailure)
        }
        exchange
    };
    let local_exch = local_exchange.write_to_bytes()?;

    // Send our local `Exchange`.
    trace!("sending exchange to remote");
    socket.send(local_exch).await?;

    // Receive the remote's `Exchange`.
    let remote_exch = {
        let raw = match socket.next().await {
            Some(r) => r?,
            None => {
                let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
                debug!("unexpected eof while waiting for remote's exchange");
                return Err(err.into())
            },
        };

        match protobuf_parse_from_bytes::<Exchange>(&raw) {
            Ok(e) => {
                trace!("received and decoded the remote's exchange");
                e
            },
            Err(err) => {
                debug!("failed to parse remote's exchange protobuf; {:?}", err);
                return Err(SecioError::HandshakeParsingFailure);
            }
        }
    };

    // Check the validity of the remote's `Exchange`. This verifies that the remote was really
    // the sender of its proposition, and that it is the owner of both its global and ephemeral
    // keys.
    {
        let mut data_to_verify = remote_proposition_bytes.clone();
        data_to_verify.extend_from_slice(&local_proposition_bytes);
        data_to_verify.extend_from_slice(remote_exch.get_epubkey());

        if !remote_public_key.verify(&data_to_verify, remote_exch.get_signature()) {
            return Err(SecioError::SignatureVerificationFailed)
        }

        trace!("successfully verified the remote's signature");
    }

    // Generate a key from the local ephemeral private key and the remote ephemeral public key,
    // derive from it a cipher key, an iv, and a hmac key, and build the encoder/decoder.
    let key_material = exchange::agree(chosen_exchange, tmp_priv_key, remote_exch.get_epubkey(), chosen_hash.num_bytes()).await?;

    // Generate a key from the local ephemeral private key and the remote ephemeral public key,
    // derive from it a cipher key, an iv, and a hmac key, and build the encoder/decoder.
    let mut codec = {
        let cipher_key_size = chosen_cipher.key_size();
        let iv_size = chosen_cipher.iv_size();

        let key = Hmac::from_key(chosen_hash, &key_material);
        let mut longer_key = vec![0u8; 2 * (iv_size + cipher_key_size + 20)];
        stretch_key(key, &mut longer_key);

        let (local_infos, remote_infos) = {
            let (first_half, second_half) = longer_key.split_at(longer_key.len() / 2);
            match hashes_ordering {
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
            let hmac = Hmac::from_key(chosen_hash, mac_key);
            let cipher = ctr(chosen_cipher, cipher_key, iv);
            (cipher, hmac)
        };

        let (decoding_cipher, decoding_hmac) = {
            let (iv, rest) = remote_infos.split_at(iv_size);
            let (cipher_key, mac_key) = rest.split_at(cipher_key_size);
            let hmac = Hmac::from_key(chosen_hash, mac_key);
            let cipher = ctr(chosen_cipher, cipher_key, iv);
            (cipher, hmac)
        };

        full_codec(
            socket,
            encoding_cipher,
            encoding_hmac,
            decoding_cipher,
            decoding_hmac,
            local_nonce.to_vec()
        )
    };

    // We send back their nonce to check if the connection works.
    trace!("checking encryption by sending back remote's nonce");
    codec.send(remote_nonce).await?;

    Ok((codec, remote_public_key, tmp_pub_key))
}

/// Custom algorithm translated from reference implementations. Needs to be the same algorithm
/// amongst all implementations.
fn stretch_key(hmac: Hmac, result: &mut [u8]) {
    match hmac {
        Hmac::Sha256(hmac) => stretch_key_inner(hmac, result),
        Hmac::Sha512(hmac) => stretch_key_inner(hmac, result),
    }
}

fn stretch_key_inner<D>(hmac: ::hmac::Hmac<D>, result: &mut [u8])
where D: ::hmac::digest::Input + ::hmac::digest::BlockInput +
          ::hmac::digest::FixedOutput + ::hmac::digest::Reset + Default + Clone,
    ::hmac::Hmac<D>: Clone + ::hmac::crypto_mac::Mac
{
    use ::hmac::Mac;
    const SEED: &[u8] = b"key expansion";

    let mut init_ctxt = hmac.clone();
    init_ctxt.input(SEED);
    let mut a = init_ctxt.result().code();

    let mut j = 0;
    while j < result.len() {
        let mut context = hmac.clone();
        context.input(a.as_ref());
        context.input(SEED);
        let b = context.result().code();

        let todo = cmp::min(b.as_ref().len(), result.len() - j);

        result[j..j + todo].copy_from_slice(&b.as_ref()[..todo]);

        j += todo;

        let mut context = hmac.clone();
        context.input(a.as_ref());
        a = context.result().code();
    }
}

#[cfg(test)]
mod tests {
    use super::{handshake, stretch_key};
    use crate::{algo_support::Digest, codec::Hmac, SecioConfig};
    use libp2p_core::identity;
    use futures::{prelude::*, channel::oneshot};

    #[test]
    #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
    fn handshake_with_self_succeeds_rsa() {
        let key1 = {
            let mut private = include_bytes!("../tests/test-rsa-private-key.pk8").to_vec();
            identity::Keypair::rsa_from_pkcs8(&mut private).unwrap()
        };

        let key2 = {
            let mut private = include_bytes!("../tests/test-rsa-private-key-2.pk8").to_vec();
            identity::Keypair::rsa_from_pkcs8(&mut private).unwrap()
        };

        handshake_with_self_succeeds(SecioConfig::new(key1), SecioConfig::new(key2));
    }

    #[test]
    fn handshake_with_self_succeeds_ed25519() {
        let key1 = identity::Keypair::generate_ed25519();
        let key2 = identity::Keypair::generate_ed25519();
        handshake_with_self_succeeds(SecioConfig::new(key1), SecioConfig::new(key2));
    }

    #[test]
    #[cfg(feature = "secp256k1")]
    fn handshake_with_self_succeeds_secp256k1() {
        let key1 = {
            let mut key = include_bytes!("../tests/test-secp256k1-private-key.der").to_vec();
            identity::Keypair::secp256k1_from_der(&mut key).unwrap()
        };

        let key2 = {
            let mut key = include_bytes!("../tests/test-secp256k1-private-key-2.der").to_vec();
            identity::Keypair::secp256k1_from_der(&mut key).unwrap()
        };

        handshake_with_self_succeeds(SecioConfig::new(key1), SecioConfig::new(key2));
    }

    fn handshake_with_self_succeeds(key1: SecioConfig, key2: SecioConfig) {
        let (l_a_tx, l_a_rx) = oneshot::channel();

        async_std::task::spawn(async move {
            let listener = async_std::net::TcpListener::bind(&"127.0.0.1:0").await.unwrap();
            l_a_tx.send(listener.local_addr().unwrap()).unwrap();
            let connec = listener.accept().await.unwrap().0;
            let mut codec = handshake(connec, key1).await.unwrap().0;
            while let Some(packet) = codec.next().await {
                let packet = packet.unwrap();
                if !packet.is_empty() {
                    codec.send(packet.into()).await.unwrap();
                }
            }
        });

        futures::executor::block_on(async move {
            let listen_addr = l_a_rx.await.unwrap();
            let connec = async_std::net::TcpStream::connect(&listen_addr).await.unwrap();
            let mut codec = handshake(connec, key2).await.unwrap().0;
            codec.send(b"hello".to_vec().into()).await.unwrap();
            let mut packets_stream = codec.filter(|p| future::ready(!p.as_ref().unwrap().is_empty()));
            let packet = packets_stream.next().await.unwrap();
            assert_eq!(packet.unwrap(), b"hello");
        });
    }

    #[test]
    fn stretch() {
        let mut output = [0u8; 32];

        let key1 = Hmac::from_key(Digest::Sha256, &[]);
        stretch_key(key1, &mut output);
        assert_eq!(
            &output,
            &[
                103, 144, 60, 199, 85, 145, 239, 71, 79, 198, 85, 164, 32, 53, 143, 205, 50, 48,
                153, 10, 37, 32, 85, 1, 226, 61, 193, 1, 154, 120, 207, 80,
            ]
        );

        let key2 = Hmac::from_key(
            Digest::Sha256,
            &[
                157, 166, 80, 144, 77, 193, 198, 6, 23, 220, 87, 220, 191, 72, 168, 197, 54, 33,
                219, 225, 84, 156, 165, 37, 149, 224, 244, 32, 170, 79, 125, 35, 171, 26, 178, 176,
                92, 168, 22, 27, 205, 44, 229, 61, 152, 21, 222, 81, 241, 81, 116, 236, 74, 166,
                89, 145, 5, 162, 108, 230, 55, 54, 9, 17,
            ],
        );
        stretch_key(key2, &mut output);
        assert_eq!(
            &output,
            &[
                39, 151, 182, 63, 180, 175, 224, 139, 42, 131, 130, 116, 55, 146, 62, 31, 157, 95,
                217, 15, 73, 81, 10, 83, 243, 141, 64, 227, 103, 144, 99, 121,
            ]
        );

        let key3 = Hmac::from_key(
            Digest::Sha256,
            &[
                98, 219, 94, 104, 97, 70, 139, 13, 185, 110, 56, 36, 66, 3, 80, 224, 32, 205, 102,
                170, 59, 32, 140, 245, 86, 102, 231, 68, 85, 249, 227, 243, 57, 53, 171, 36, 62,
                225, 178, 74, 89, 142, 151, 94, 183, 231, 208, 166, 244, 130, 130, 209, 248, 65,
                19, 48, 127, 127, 55, 82, 117, 154, 124, 108,
            ],
        );
        stretch_key(key3, &mut output);
        assert_eq!(
            &output,
            &[
                28, 39, 158, 206, 164, 16, 211, 194, 99, 43, 208, 36, 24, 141, 90, 93, 157, 236,
                238, 111, 170, 0, 60, 11, 49, 174, 177, 121, 30, 12, 182, 25,
            ]
        );
    }
}
