#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::{io, marker::PhantomData};

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, BytesMut};
use prost::Message;

mod generated;

#[doc(hidden)] // NOT public API. Do not use.
pub use generated::test as proto;

/// [`Codec`] implements [`Encoder`] and [`Decoder`], uses [`unsigned_varint`]
///
/// to prefix messages with their length and uses [`prost`] and a provided
/// `struct` implementing [`prost::Message`] to do the encoding.
#[derive(Debug, Clone)]
pub struct Codec<In, Out = In> {
    max_message_len_bytes: usize,
    phantom: PhantomData<(In, Out)>,
}

impl<In, Out> Codec<In, Out> {
    /// Create new [`Codec`].
    ///
    /// Parameter `max_message_len_bytes` determines the maximum length of the
    /// Protobuf message. The limit does not include the bytes needed for the
    /// [`unsigned_varint`].
    pub fn new(max_message_len_bytes: usize) -> Self {
        Self {
            max_message_len_bytes,
            phantom: PhantomData,
        }
    }
}

impl<In: Message, Out> Encoder for Codec<In, Out> {
    type Item<'a> = In;
    type Error = Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let message_length = item.encoded_len();

        let mut uvi_buf = unsigned_varint::encode::usize_buffer();
        let encoded_length = unsigned_varint::encode::usize(message_length, &mut uvi_buf);
        dst.reserve(encoded_length.len() + message_length);
        dst.extend_from_slice(encoded_length);

        item.encode(dst).map_err(io::Error::other)?;

        Ok(())
    }
}

impl<In, Out> Decoder for Codec<In, Out>
where
    Out: Message + Default,
{
    type Item = Out;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let (message_length, remaining) = match unsigned_varint::decode::usize(src) {
            Ok((len, remaining)) => (len, remaining),
            Err(unsigned_varint::decode::Error::Insufficient) => return Ok(None),
            Err(e) => return Err(Error(io::Error::new(io::ErrorKind::InvalidData, e))),
        };

        if message_length > self.max_message_len_bytes {
            return Err(Error(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "message with {message_length}b exceeds maximum of {}b",
                    self.max_message_len_bytes
                ),
            )));
        }

        // Compute how many bytes the varint itself consumed.
        let varint_length = src.len() - remaining.len();

        // Ensure we can read an entire message.
        if src.len() < (message_length + varint_length) {
            return Ok(None);
        }

        // Safe to advance buffer now.
        src.advance(varint_length);

        let message_bytes = src.split_to(message_length);

        let message = Self::Item::decode(&message_bytes[..])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Some(message))
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Failed to encode/decode message")]
pub struct Error(#[from] io::Error);

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        e.0
    }
}

/// Extracts the message bytes from a length-prefixed buffer.
///
/// Decodes the unsigned varint length prefix and returns the remaining
/// message bytes. Returns an error if the length prefix is invalid.
pub fn extract_message_bytes(src: &[u8]) -> io::Result<&[u8]> {
    let (message_length, remaining) = unsigned_varint::decode::usize(src)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if remaining.len() < message_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "buffer too short for declared message length",
        ));
    }
    Ok(&remaining[..message_length])
}

/// Decodes a protobuf field key (tag + wire type) and returns just the tag number.
/// Advances the buffer past the key only (does NOT consume the field value).
pub fn decode_field_tag(buf: &mut &[u8]) -> io::Result<u32> {
    use prost::encoding::decode_key;
    let (tag, _wire_type) =
        decode_key(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(tag)
}

/// Consumes a complete protobuf field (tag + value) from the buffer.
///
/// Returns the field tag. Advances the buffer past the entire field.
/// This is a high-level helper that hides wire type details from the caller.
pub fn consume_field(buf: &mut &[u8]) -> io::Result<u32> {
    use prost::encoding::{WireType, decode_key, decode_varint};

    // Read tag and wire type
    let (tag, wire_type) =
        decode_key(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Consume value based on wire type
    match wire_type {
        WireType::Varint => {
            decode_varint(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }
        WireType::SixtyFourBit => {
            if buf.len() < 8 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "buffer underflow",
                ));
            }
            *buf = &buf[8..];
        }
        WireType::LengthDelimited => {
            let len = decode_varint(buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
                as usize;
            if buf.len() < len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "buffer underflow",
                ));
            }
            *buf = &buf[len..];
        }
        WireType::StartGroup | WireType::EndGroup => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "groups are not supported",
            ));
        }
        WireType::ThirtyTwoBit => {
            if buf.len() < 4 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "buffer underflow",
                ));
            }
            *buf = &buf[4..];
        }
    }

    Ok(tag)
}

/// Consumes a length-delimited protobuf message from the buffer.
///
/// Returns the message bytes as a slice. Advances the buffer past the message.
/// This is useful for extracting nested messages without copying.
pub fn consume_message<'a>(buf: &mut &'a [u8]) -> io::Result<&'a [u8]> {
    use prost::encoding::decode_varint;

    let len =
        decode_varint(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))? as usize;
    if buf.len() < len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "buffer underflow",
        ));
    }
    let msg = &buf[..len];
    *buf = &buf[len..];
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use asynchronous_codec::FramedRead;
    use futures::{FutureExt, StreamExt, io::Cursor};
    use quickcheck::{Arbitrary, Gen, QuickCheck};

    use super::*;

    #[test]
    fn honors_max_message_length() {
        let codec = Codec::<Dummy>::new(1);
        let mut src = varint_zeroes(100);

        let mut read = FramedRead::new(Cursor::new(&mut src), codec);
        let err = read.next().now_or_never().unwrap().unwrap().unwrap_err();

        assert_eq!(
            err.source().unwrap().to_string(),
            "message with 100b exceeds maximum of 1b"
        )
    }

    #[test]
    fn empty_bytes_mut_does_not_panic() {
        let mut codec = Codec::<Dummy>::new(100);

        let mut src = varint_zeroes(100);
        src.truncate(50);

        let result = codec.decode(&mut src);

        assert!(result.unwrap().is_none());
        assert_eq!(
            src.len(),
            50,
            "to not modify `src` if we cannot read a full message"
        )
    }

    #[test]
    fn only_partial_message_in_bytes_mut_does_not_panic() {
        let mut codec = Codec::<Dummy>::new(100);

        let result = codec.decode(&mut BytesMut::new());

        assert!(result.unwrap().is_none());
    }

    #[test]
    fn handles_arbitrary_initial_capacity() {
        fn prop(message: proto::Message, initial_capacity: u16) {
            let mut buffer = BytesMut::with_capacity(initial_capacity as usize);
            let mut codec = Codec::<proto::Message>::new(u32::MAX as usize);

            codec.encode(message.clone(), &mut buffer).unwrap();
            let decoded = codec.decode(&mut buffer).unwrap().unwrap();

            assert_eq!(message, decoded);
        }

        QuickCheck::new().quickcheck(prop as fn(_, _) -> _)
    }

    /// Constructs a [`BytesMut`] of the provided length where the message is all zeros.
    fn varint_zeroes(length: usize) -> BytesMut {
        let mut buf = unsigned_varint::encode::usize_buffer();
        let encoded_length = unsigned_varint::encode::usize(length, &mut buf);

        let mut src = BytesMut::new();
        src.extend_from_slice(encoded_length);
        src.extend(std::iter::repeat_n(0, length));
        src
    }

    impl Arbitrary for proto::Message {
        fn arbitrary(g: &mut Gen) -> Self {
            Self {
                data: Vec::arbitrary(g),
            }
        }
    }

    /// Dummy message type for tests that don't need real decoding.
    #[derive(Debug, Default)]
    struct Dummy;

    impl Message for Dummy {
        fn encode_raw(&self, _buf: &mut impl bytes::BufMut)
        where
            Self: Sized,
        {
            unimplemented!()
        }

        fn merge_field(
            &mut self,
            _tag: u32,
            _wire_type: prost::encoding::WireType,
            _buf: &mut impl bytes::Buf,
            _ctx: prost::encoding::DecodeContext,
        ) -> Result<(), prost::DecodeError>
        where
            Self: Sized,
        {
            unimplemented!()
        }

        fn encoded_len(&self) -> usize {
            unimplemented!()
        }

        fn clear(&mut self) {
            unimplemented!()
        }
    }
}
