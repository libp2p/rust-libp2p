#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::{io, marker::PhantomData};

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, BufMut, BytesMut};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer, WriterBackend};

mod generated;

#[doc(hidden)] // NOT public API. Do not use.
pub use generated::test as proto;

/// [`Codec`] implements [`Encoder`] and [`Decoder`], uses [`unsigned_varint`]
///
/// to prefix messages with their length and uses [`quick_protobuf`] and a provided
/// `struct` implementing [`MessageRead`] and [`MessageWrite`] to do the encoding.
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

impl<In: MessageWrite, Out> Encoder for Codec<In, Out> {
    type Item<'a> = In;
    type Error = Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        write_length(&item, dst);
        write_message(&item, dst)?;

        Ok(())
    }
}

/// Write the message's length (i.e. `size`) to `dst` as a variable-length integer.
fn write_length(message: &impl MessageWrite, dst: &mut BytesMut) {
    let message_length = message.get_size();

    let mut uvi_buf = unsigned_varint::encode::usize_buffer();
    let encoded_length = unsigned_varint::encode::usize(message_length, &mut uvi_buf);

    dst.extend_from_slice(encoded_length);
}

/// Write the message itself to `dst`.
fn write_message(item: &impl MessageWrite, dst: &mut BytesMut) -> io::Result<()> {
    let mut writer = Writer::new(BytesMutWriterBackend::new(dst));
    item.write_message(&mut writer)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(())
}

impl<In, Out> Decoder for Codec<In, Out>
where
    Out: for<'a> MessageRead<'a>,
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
                io::ErrorKind::PermissionDenied,
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

        let message = src.split_to(message_length);

        let mut reader = BytesReader::from_bytes(&message);
        let message = Self::Item::from_reader(&mut reader, &message)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Some(message))
    }
}

struct BytesMutWriterBackend<'a> {
    dst: &'a mut BytesMut,
}

impl<'a> BytesMutWriterBackend<'a> {
    fn new(dst: &'a mut BytesMut) -> Self {
        Self { dst }
    }
}

impl WriterBackend for BytesMutWriterBackend<'_> {
    fn pb_write_u8(&mut self, x: u8) -> quick_protobuf::Result<()> {
        self.dst.put_u8(x);

        Ok(())
    }

    fn pb_write_u32(&mut self, x: u32) -> quick_protobuf::Result<()> {
        self.dst.put_u32_le(x);

        Ok(())
    }

    fn pb_write_i32(&mut self, x: i32) -> quick_protobuf::Result<()> {
        self.dst.put_i32_le(x);

        Ok(())
    }

    fn pb_write_f32(&mut self, x: f32) -> quick_protobuf::Result<()> {
        self.dst.put_f32_le(x);

        Ok(())
    }

    fn pb_write_u64(&mut self, x: u64) -> quick_protobuf::Result<()> {
        self.dst.put_u64_le(x);

        Ok(())
    }

    fn pb_write_i64(&mut self, x: i64) -> quick_protobuf::Result<()> {
        self.dst.put_i64_le(x);

        Ok(())
    }

    fn pb_write_f64(&mut self, x: f64) -> quick_protobuf::Result<()> {
        self.dst.put_f64_le(x);

        Ok(())
    }

    fn pb_write_all(&mut self, buf: &[u8]) -> quick_protobuf::Result<()> {
        self.dst.put_slice(buf);

        Ok(())
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

#[cfg(test)]
mod tests {
    use std::error::Error;

    use asynchronous_codec::FramedRead;
    use futures::{io::Cursor, FutureExt, StreamExt};
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
        src.extend(std::iter::repeat(0).take(length));
        src
    }

    impl Arbitrary for proto::Message {
        fn arbitrary(g: &mut Gen) -> Self {
            Self {
                data: Vec::arbitrary(g),
            }
        }
    }

    #[derive(Debug)]
    struct Dummy;

    impl<'a> MessageRead<'a> for Dummy {
        fn from_reader(_: &mut BytesReader, _: &'a [u8]) -> quick_protobuf::Result<Self> {
            todo!()
        }
    }
}
