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

use aes_ctr::stream_cipher::StreamCipherCore;
use ring::hmac;
use tokio_io::codec::length_delimited;
use tokio_io::{AsyncRead, AsyncWrite};

mod decode;
mod encode;

/// Type returned by `full_codec`.
pub type FullCodec<S> = DecoderMiddleware<EncoderMiddleware<length_delimited::Framed<S>>>;

pub type StreamCipher = Box<dyn StreamCipherCore + Send>;


/// Takes control of `socket`. Returns an object that implements `future::Sink` and
/// `future::Stream`. The `Stream` and `Sink` produce and accept `BytesMut` objects.
///
/// The conversion between the stream/sink items and the socket is done with the given cipher and
/// hash algorithm (which are generally decided during the handshake).
pub fn full_codec<S>(
    socket: length_delimited::Framed<S>,
    cipher_encoding: StreamCipher,
    encoding_hmac: hmac::SigningKey,
    cipher_decoder: StreamCipher,
    decoding_hmac: hmac::VerificationKey,
) -> FullCodec<S>
where
    S: AsyncRead + AsyncWrite,
{
    let hmac_num_bytes = encoding_hmac.digest_algorithm().output_len;
    let encoder = EncoderMiddleware::new(socket, cipher_encoding, encoding_hmac);
    DecoderMiddleware::new(encoder, cipher_decoder, decoding_hmac, hmac_num_bytes)
}

#[cfg(test)]
mod tests {
    extern crate tokio_current_thread;
    extern crate tokio_tcp;
    use self::tokio_tcp::TcpListener;
    use self::tokio_tcp::TcpStream;
    use stream_cipher::{ctr, Cipher};
    use super::full_codec;
    use super::DecoderMiddleware;
    use super::EncoderMiddleware;
    use bytes::BytesMut;
    use error::SecioError;
    use futures::sync::mpsc::channel;
    use futures::{Future, Sink, Stream};
    use rand;
    use ring::digest::SHA256;
    use ring::hmac::SigningKey;
    use ring::hmac::VerificationKey;
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
            SigningKey::new(&SHA256, &hmac_key),
        );
        let decoder = DecoderMiddleware::new(
            data_rx,
            ctr(Cipher::Aes256, &cipher_key, &NULL_IV[..]),
            VerificationKey::new(&SHA256, &hmac_key),
            32,
        );

        let data = b"hello world";

        let data_sent = encoder.send(BytesMut::from(data.to_vec())).from_err();
        let data_received = decoder.into_future().map(|(n, _)| n).map_err(|(e, _)| e);

        let (_, decoded) = tokio_current_thread::block_on_all(data_sent.join(data_received))
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

        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener.incoming().into_future().map_err(|(e, _)| e).map(
            move |(connec, _)| {
                let connec = Framed::new(connec.unwrap());

                full_codec(
                    connec,
                    ctr(cipher, &cipher_key[..key_size], &NULL_IV[..]),
                    SigningKey::new(&SHA256, &hmac_key),
                    ctr(cipher, &cipher_key[..key_size], &NULL_IV[..]),
                    VerificationKey::new(&SHA256, &hmac_key),
                )
            },
        );

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .map(move |stream| {
                let stream = Framed::new(stream);

                full_codec(
                    stream,
                    ctr(cipher, &cipher_key_clone[..key_size], &NULL_IV[..]),
                    SigningKey::new(&SHA256, &hmac_key_clone),
                    ctr(cipher, &cipher_key_clone[..key_size], &NULL_IV[..]),
                    VerificationKey::new(&SHA256, &hmac_key_clone),
                )
            });

        let fin = server
            .join(client)
            .from_err::<SecioError>()
            .and_then(|(server, client)| {
                client
                    .send(BytesMut::from(&data_clone[..]))
                    .map(move |_| server)
                    .from_err()
            })
            .and_then(|server| server.into_future().map_err(|(e, _)| e.into()))
            .map(|recved| recved.0.unwrap().to_vec());

        let received = tokio_current_thread::block_on_all(fin).unwrap();
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
