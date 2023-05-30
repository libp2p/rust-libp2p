#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Bytes, BytesMut};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::marker::PhantomData;
use unsigned_varint::codec::UviBytes;

/// [`Codec`] implements [`Encoder`] and [`Decoder`], uses [`unsigned_varint`]
/// to prefix messages with their length and uses [`quick_protobuf`] and a provided
/// `struct` implementing [`MessageRead`] and [`MessageWrite`] to do the encoding.
pub struct Codec<In, Out = In> {
    uvi: UviBytes,
    phantom: PhantomData<(In, Out)>,
}

impl<In, Out> Codec<In, Out> {
    /// Create new [`Codec`].
    ///
    /// Parameter `max_message_len_bytes` determines the maximum length of the
    /// Protobuf message. The limit does not include the bytes needed for the
    /// [`unsigned_varint`].
    pub fn new(max_message_len_bytes: usize) -> Self {
        let mut uvi = UviBytes::default();
        uvi.set_max_len(max_message_len_bytes);
        Self {
            uvi,
            phantom: PhantomData,
        }
    }
}

impl<In: MessageWrite, Out> Encoder for Codec<In, Out> {
    type Item = In;
    type Error = Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut encoded_msg = Vec::new();
        let mut writer = Writer::new(&mut encoded_msg);
        item.write_message(&mut writer)
            .expect("Encoding to succeed");
        self.uvi.encode(Bytes::from(encoded_msg), dst)?;

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
        let msg = match self.uvi.decode(src)? {
            None => return Ok(None),
            Some(msg) => msg,
        };

        let mut reader = BytesReader::from_bytes(&msg);
        let message = Self::Item::from_reader(&mut reader, &msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(message))
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Failed to encode/decode message")]
pub struct Error(#[from] std::io::Error);

impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        e.0
    }
}
