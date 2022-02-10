// Copyright 2022 Protocol Labs.
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

use crate::message_proto;
use bytes::BytesMut;
use prost::Message;
use std::io::Cursor;
use thiserror::Error;
use unsigned_varint::codec::UviBytes;

const MAX_MESSAGE_SIZE_BYTES: usize = 4096;

pub struct Codec(UviBytes);

impl Codec {
    pub fn new() -> Self {
        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_MESSAGE_SIZE_BYTES);
        Self(codec)
    }
}

impl asynchronous_codec::Encoder for Codec {
    type Item = message_proto::HolePunch;
    type Error = Error;

    fn encode(
        &mut self,
        item: Self::Item,
        dst: &mut asynchronous_codec::BytesMut,
    ) -> Result<(), Self::Error> {
        let mut encoded_msg = BytesMut::new();
        item.encode(&mut encoded_msg)
            .expect("BytesMut to have sufficient capacity.");
        self.0
            .encode(encoded_msg.freeze(), dst)
            .map_err(|e| e.into())
    }
}

impl asynchronous_codec::Decoder for Codec {
    type Item = message_proto::HolePunch;
    type Error = Error;

    fn decode(
        &mut self,
        src: &mut asynchronous_codec::BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        Ok(self
            .0
            .decode(src)?
            .map(|msg| message_proto::HolePunch::decode(Cursor::new(msg)))
            .transpose()?)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to decode response: {0}.")]
    Decode(
        #[from]
        #[source]
        prost::DecodeError,
    ),
    #[error("Io error {0}")]
    Io(
        #[from]
        #[source]
        std::io::Error,
    ),
}
