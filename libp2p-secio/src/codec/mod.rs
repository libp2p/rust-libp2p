use self::decode::DecoderMiddleware;
use self::encode::EncoderMiddleware;

use crypto::symmetriccipher::SynchronousStreamCipher;
use ring::hmac;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited;

mod decode;
mod encode;

/// Type returned by `full_codec`.
pub type FullCodec<S> = DecoderMiddleware<EncoderMiddleware<length_delimited::Framed<S>>>;

/// Takes control of `socket`. Returns an object that implements `future::Sink` and
/// `future::Stream`. The `Stream` and `Sink` produce and accept `BytesMut` objects.
///
/// The conversion between the stream/sink items and the socket is done with the given cipher and
/// hash algorithm (which are generally decided during the handshake).
///
/// > **Note**: The encoding block size could theoretically be determined from `cipher_encoding`,
/// >           but the underlying library doesn't allow doing that.
pub fn full_codec<S>(
	socket: S,
	cipher_encoding: Box<SynchronousStreamCipher>,
	encoding_hmac: hmac::SigningKey,
	cipher_decoder: Box<SynchronousStreamCipher>,
	decoding_hmac: hmac::VerificationKey,
) -> FullCodec<S>
where
	S: AsyncRead + AsyncWrite,
{
	let framed =
		length_delimited::Builder::new().big_endian().length_field_length(4).new_framed(socket);

	let encoder = EncoderMiddleware::new(framed, cipher_encoding, encoding_hmac);
	let codec = DecoderMiddleware::new(encoder, cipher_decoder, decoding_hmac);

	codec
}

#[cfg(test)]
mod tests {
	use super::DecoderMiddleware;
	use super::EncoderMiddleware;
	use super::full_codec;
	use bytes::BytesMut;
	use crypto::aessafe::AesSafe256Encryptor;
	use crypto::blockmodes::CtrMode;
	use error::SecioError;
	use futures::{Future, Sink, Stream};
	use futures::sync::mpsc::channel;
	use rand;
	use ring::digest::SHA256;
	use ring::hmac::SigningKey;
	use ring::hmac::VerificationKey;
	use std::io::Error as IoError;
	use tokio_core::net::TcpListener;
	use tokio_core::net::TcpStream;
	use tokio_core::reactor::Core;

	#[test]
	fn raw_encode_then_decode() {
		let (data_tx, data_rx) = channel::<BytesMut>(256);
		let data_tx = data_tx.sink_map_err::<_, IoError>(|_| panic!());
		let data_rx = data_rx.map_err::<IoError, _>(|_| panic!());

		let cipher_key: [u8; 32] = rand::random();
		let hmac_key: [u8; 32] = rand::random();

		let encoder =
			EncoderMiddleware::new(
				data_tx,
				Box::new(CtrMode::new(AesSafe256Encryptor::new(&cipher_key), vec![0; 16])),
				SigningKey::new(&SHA256, &hmac_key),
			);
		let decoder =
			DecoderMiddleware::new(
				data_rx,
				Box::new(CtrMode::new(AesSafe256Encryptor::new(&cipher_key), vec![0; 16])),
				VerificationKey::new(&SHA256, &hmac_key),
			);

		let data = b"hello world";

		let data_sent = encoder.send(BytesMut::from(data.to_vec())).from_err();
		let data_received = decoder.into_future().map(|(n, _)| n).map_err(|(e, _)| e);

		let mut core = Core::new().unwrap();
		let (_, decoded) = core.run(data_sent.join(data_received)).map_err(|_| ()).unwrap();
		assert_eq!(decoded.unwrap(), data);
	}

	#[test]
	fn full_codec_encode_then_decode() {
		let mut core = Core::new().unwrap();

		let cipher_key: [u8; 32] = rand::random();
		let cipher_key_clone = cipher_key.clone();
		let hmac_key: [u8; 32] = rand::random();
		let hmac_key_clone = hmac_key.clone();
		let data = b"hello world";
		let data_clone = data.clone();

		let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
		let listener_addr = listener.local_addr().unwrap();

		let server =
			listener.incoming().into_future().map_err(|(e, _)| e).map(move |(connec, _)| {
				full_codec(
					connec.unwrap().0,
					Box::new(CtrMode::new(AesSafe256Encryptor::new(&cipher_key), vec![0; 16])),
					SigningKey::new(&SHA256, &hmac_key),
					Box::new(CtrMode::new(AesSafe256Encryptor::new(&cipher_key), vec![0; 16])),
					VerificationKey::new(&SHA256, &hmac_key),
				)
			});

		let client = TcpStream::connect(&listener_addr, &core.handle())
			.map_err(|e| e.into())
			.map(move |stream| {
				full_codec(
					stream,
					Box::new(CtrMode::new(AesSafe256Encryptor::new(&cipher_key_clone), vec![0; 16])),
					SigningKey::new(&SHA256, &hmac_key_clone),
					Box::new(CtrMode::new(AesSafe256Encryptor::new(&cipher_key_clone), vec![0; 16])),
					VerificationKey::new(&SHA256, &hmac_key_clone),
				)
			});

		let fin = server.join(client)
		                .from_err::<SecioError>()
		                .and_then(|(server, client)| {
			client.send(BytesMut::from(&data_clone[..])).map(move |_| server).from_err()
		})
		                .and_then(|server| server.into_future().map_err(|(e, _)| e.into()))
		                .map(|recved| recved.0.unwrap().to_vec());

		let received = core.run(fin).unwrap();
		assert_eq!(received, data);
	}
}
