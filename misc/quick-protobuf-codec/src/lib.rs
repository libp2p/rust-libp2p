#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, BytesMut};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer, WriterBackend};
use std::io;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

mod generated;

#[doc(hidden)] // NOT public API. Do not use.
pub use generated::test as proto;

/// [`Codec`] implements [`Encoder`] and [`Decoder`], uses [`unsigned_varint`]
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
        let message_length = item.get_size();

        let mut uvi_buf = unsigned_varint::encode::usize_buffer();
        let encoded_length = unsigned_varint::encode::usize(message_length, &mut uvi_buf);

        // Append the 'unsigned varint'-encoded length.
        dst.extend_from_slice(encoded_length);

        // Ensure we have enough capacity to encode our message.
        dst.reserve(message_length);

        let mut written = 0;

        let mut writer = Writer::new(UninitMemoryWriterBackend::new(
            dst.spare_capacity_mut(),
            &mut written,
        ));
        item.write_message(&mut writer)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        if written != message_length {
            return Err(Error(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "expected message to be {message_length} bytes long but was {written} bytes"
                ),
            )));
        }

        // SAFETY: `written` records exactly how many bytes we wrote to `dst`, thus it is safe to extend the length by `written`.
        unsafe {
            dst.set_len(dst.len() + written);
        }

        Ok(())
    }
}

impl<In, Out> Decoder for Codec<In, Out>
where
    Out: for<'a> MessageRead<'a>,
{
    type Item = Out;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let (len, remaining) = match unsigned_varint::decode::usize(src) {
            Ok((len, remaining)) => (len, remaining),
            Err(unsigned_varint::decode::Error::Insufficient) => return Ok(None),
            Err(e) => return Err(Error(io::Error::new(io::ErrorKind::InvalidData, e))),
        };
        let consumed = src.len() - remaining.len();
        src.advance(consumed);

        if len > self.max_message_len_bytes {
            return Err(Error(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "message with {len}b exceeds maximum of {}b",
                    self.max_message_len_bytes
                ),
            )));
        }

        let msg = src.split_to(len);

        let mut reader = BytesReader::from_bytes(&msg);
        let message = Self::Item::from_reader(&mut reader, &msg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Some(message))
    }
}

struct UninitMemoryWriterBackend<'a> {
    memory: &'a mut [MaybeUninit<u8>],
    written: &'a mut usize,
}

impl<'a> UninitMemoryWriterBackend<'a> {
    fn new(memory: &'a mut [MaybeUninit<u8>], written: &'a mut usize) -> Self {
        Self { memory, written }
    }
}

impl<'a> WriterBackend for UninitMemoryWriterBackend<'a> {
    fn pb_write_u8(&mut self, x: u8) -> quick_protobuf::Result<()> {
        self.pb_write_all(&[x])
    }

    fn pb_write_u32(&mut self, x: u32) -> quick_protobuf::Result<()> {
        self.pb_write_all(&x.to_le_bytes())
    }

    fn pb_write_i32(&mut self, x: i32) -> quick_protobuf::Result<()> {
        self.pb_write_all(&x.to_le_bytes())
    }

    fn pb_write_f32(&mut self, x: f32) -> quick_protobuf::Result<()> {
        self.pb_write_all(&x.to_le_bytes())
    }

    fn pb_write_u64(&mut self, x: u64) -> quick_protobuf::Result<()> {
        self.pb_write_all(&x.to_le_bytes())
    }

    fn pb_write_i64(&mut self, x: i64) -> quick_protobuf::Result<()> {
        self.pb_write_all(&x.to_le_bytes())
    }

    fn pb_write_f64(&mut self, x: f64) -> quick_protobuf::Result<()> {
        self.pb_write_all(&x.to_le_bytes())
    }

    fn pb_write_all(&mut self, buf: &[u8]) -> quick_protobuf::Result<()> {
        if self.memory.len() - *self.written < buf.len() {
            return Err(quick_protobuf::errors::Error::UnexpectedEndOfBuffer);
        }

        for b in buf {
            self.memory[*self.written].write(*b);
            *self.written += 1;
        }

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
    use super::*;
    use crate::proto;
    use asynchronous_codec::FramedRead;
    use futures::io::Cursor;
    use futures::{FutureExt, StreamExt};
    use quickcheck::{Arbitrary, Gen, QuickCheck};
    use std::error::Error;

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
