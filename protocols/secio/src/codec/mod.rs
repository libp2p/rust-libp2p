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

//! Individual messages encoding and decoding. Use this after the algorithms have been
//! successfully negotiated.

use self::decode::DecoderMiddleware;
use self::encode::EncoderMiddleware;

use aes_ctr::stream_cipher;
use crate::algo_support::Digest;
use hmac::{self, Mac};
use sha2::{Sha256, Sha512};
use tokio_io::codec::length_delimited;
use tokio_io::{AsyncRead, AsyncWrite};

mod decode;
mod encode;

/// Type returned by `full_codec`.
pub type FullCodec<S> = DecoderMiddleware<EncoderMiddleware<length_delimited::Framed<S>>>;

pub type StreamCipher = Box<dyn stream_cipher::StreamCipher + Send>;

#[derive(Debug, Clone)]
pub enum Hmac {
    Sha256(hmac::Hmac<Sha256>),
    Sha512(hmac::Hmac<Sha512>),
}

impl Hmac {
    /// Returns the size of the hash in bytes.
    #[inline]
    pub fn num_bytes(&self) -> usize {
        match *self {
            Hmac::Sha256(_) => 32,
            Hmac::Sha512(_) => 64,
        }
    }

    /// Builds a `Hmac` from an algorithm and key.
    pub fn from_key(algorithm: Digest, key: &[u8]) -> Self {
        // TODO: it would be nice to tweak the hmac crate to add an equivalent to new_varkey that
        //       never errors
        match algorithm {
            Digest::Sha256 => Hmac::Sha256(Mac::new_varkey(key)
                .expect("Hmac::new_varkey accepts any key length")),
            Digest::Sha512 => Hmac::Sha512(Mac::new_varkey(key)
                .expect("Hmac::new_varkey accepts any key length")),
        }
    }

    /// Signs the data.
    // TODO: better return type?
    pub fn sign(&self, crypted_data: &[u8]) -> Vec<u8> {
        match *self {
            Hmac::Sha256(ref hmac) => {
                let mut hmac = hmac.clone();
                hmac.input(crypted_data);
                hmac.result().code().to_vec()
            },
            Hmac::Sha512(ref hmac) => {
                let mut hmac = hmac.clone();
                hmac.input(crypted_data);
                hmac.result().code().to_vec()
            },
        }
    }

    /// Verifies that the data matches the expected hash.
    // TODO: better error?
    pub fn verify(&self, crypted_data: &[u8], expected_hash: &[u8]) -> Result<(), ()> {
        match *self {
            Hmac::Sha256(ref hmac) => {
                let mut hmac = hmac.clone();
                hmac.input(crypted_data);
                hmac.verify(expected_hash).map_err(|_| ())
            },
            Hmac::Sha512(ref hmac) => {
                let mut hmac = hmac.clone();
                hmac.input(crypted_data);
                hmac.verify(expected_hash).map_err(|_| ())
            },
        }
    }
}

/// Takes control of `socket`. Returns an object that implements `future::Sink` and
/// `future::Stream`. The `Stream` and `Sink` produce and accept `BytesMut` objects.
///
/// The conversion between the stream/sink items and the socket is done with the given cipher and
/// hash algorithm (which are generally decided during the handshake).
pub fn full_codec<S>(
    socket: length_delimited::Framed<S>,
    cipher_encoding: StreamCipher,
    encoding_hmac: Hmac,
    cipher_decoder: StreamCipher,
    decoding_hmac: Hmac,
    remote_nonce: Vec<u8>
) -> FullCodec<S>
where
    S: AsyncRead + AsyncWrite,
{
    let encoder = EncoderMiddleware::new(socket, cipher_encoding, encoding_hmac);
    DecoderMiddleware::new(encoder, cipher_decoder, decoding_hmac, remote_nonce)
}

#[cfg(test)]
mod tests {
    use tokio::runtime::current_thread::Runtime;
    use tokio_tcp::{TcpListener, TcpStream};
    use crate::stream_cipher::{ctr, Cipher};
    use super::full_codec;
    use super::DecoderMiddleware;
    use super::EncoderMiddleware;
    use super::Hmac;
    use crate::algo_support::Digest;
    use crate::error::SecioError;
    use bytes::BytesMut;
    use futures::sync::mpsc::channel;
    use futures::{Future, Sink, Stream, stream};
    use rand;
    use std::io::Error as IoError;
    use tokio_io::codec::length_delimited::Framed;

    const NULL_IV : [u8; 16] = [0;16];

    #[test]
    fn raw_encode_then_decode() {
        let (data_tx, data_rx) = channel::<BytesMut>(256);
        let data_tx = data_tx.sink_map_err::<_, IoError>(|_| panic!());
        let data_rx = data_rx.map_err::<IoError, _>(|_| panic!());

        let cipher_key: [u8; 32] = rand::random();
        let hmac_key: [u8; 32] = rand::random();


        let encoder = EncoderMiddleware::new(
            data_tx,
            ctr(Cipher::Aes256, &cipher_key, &NULL_IV[..]),
            Hmac::from_key(Digest::Sha256, &hmac_key),
        );
        let decoder = DecoderMiddleware::new(
            data_rx,
            ctr(Cipher::Aes256, &cipher_key, &NULL_IV[..]),
            Hmac::from_key(Digest::Sha256, &hmac_key),
            Vec::new()
        );

        let data = b"hello world";

        let data_sent = encoder.send(BytesMut::from(data.to_vec())).from_err();
        let data_received = decoder.into_future().map(|(n, _)| n).map_err(|(e, _)| e);
        let mut rt = Runtime::new().unwrap();

        let (_, decoded) = rt.block_on(data_sent.join(data_received))
            .map_err(|_| ())
            .unwrap();
        assert_eq!(&decoded.unwrap()[..], &data[..]);
    }

    fn full_codec_encode_then_decode(cipher: Cipher) {
        let cipher_key: [u8; 32] = rand::random();
        let cipher_key_clone = cipher_key.clone();
        let key_size = cipher.key_size();
        let hmac_key: [u8; 16] = rand::random();
        let hmac_key_clone = hmac_key.clone();
        let data = b"hello world";
        let data_clone = data.clone();
        let nonce = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let nonce2 = nonce.clone();
        let server = listener.incoming()
            .into_future()
            .map_err(|(e, _)| e)
            .map(move |(connec, _)| {
                full_codec(
                    Framed::new(connec.unwrap()),
                    ctr(cipher, &cipher_key[..key_size], &NULL_IV[..]),
                    Hmac::from_key(Digest::Sha256, &hmac_key),
                    ctr(cipher, &cipher_key[..key_size], &NULL_IV[..]),
                    Hmac::from_key(Digest::Sha256, &hmac_key),
                    nonce2
                )
            },
        );

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .map(move |stream| {
                full_codec(
                    Framed::new(stream),
                    ctr(cipher, &cipher_key_clone[..key_size], &NULL_IV[..]),
                    Hmac::from_key(Digest::Sha256, &hmac_key_clone),
                    ctr(cipher, &cipher_key_clone[..key_size], &NULL_IV[..]),
                    Hmac::from_key(Digest::Sha256, &hmac_key_clone),
                    Vec::new()
                )
            });

        let fin = server
            .join(client)
            .from_err::<SecioError>()
            .and_then(|(server, client)| {
                client
                    .send_all(stream::iter_ok::<_, IoError>(vec![nonce.into(), data_clone[..].into()]))
                    .map(move |_| server)
                    .from_err()
            })
            .and_then(|server| server.concat2().from_err());

        let mut rt = Runtime::new().unwrap();
        let received = rt.block_on(fin).unwrap();
        assert_eq!(received, data);
    }

    #[test]
    fn full_codec_encode_then_decode_aes128() {
        full_codec_encode_then_decode(Cipher::Aes128);
    }

    #[test]
    fn full_codec_encode_then_decode_aes256() {
        full_codec_encode_then_decode(Cipher::Aes256);
    }

    #[test]
    fn full_codec_encode_then_decode_twofish() {
        full_codec_encode_then_decode(Cipher::TwofishCtr);
    }

    #[test]
    fn full_codec_encode_then_decode_null() {
        full_codec_encode_then_decode(Cipher::Null);
    }
}
