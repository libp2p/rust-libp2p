use self::encode::EncoderMiddleware;
use self::decode::DecoderMiddleware;
use tokio_io::codec::length_delimited;
use tokio_io::{AsyncRead, AsyncWrite};

pub mod encode;
mod decode;

pub type FullCodec<S> = DecoderMiddleware<EncoderMiddleware<length_delimited::Framed<S>>>;

pub fn full_codec<S>(socket: length_delimited::Framed<S>) -> FullCodec<S>
where S: AsyncRead + AsyncWrite,
{
    FullCodec { raw_stream: EncoderMiddleware { raw_sink: socket } }
}