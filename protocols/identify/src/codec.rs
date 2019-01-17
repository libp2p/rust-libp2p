// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::message::CircuitRelay;
use bytes::BytesMut;
use protobuf::{self, Message};
use std::{fmt, error::Error};
use tokio_codec::{Encoder, Decoder};
use unsigned_varint::codec::UviBytes;

/// Encoder and decoder for protocol messages.
//#[derive(Debug)]  // TODO:
pub struct Codec {
    inner: UviBytes<Vec<u8>>,
}

impl Codec {
    /// Creates a `Codec`.
    #[inline]
    pub fn new() -> Self {
        Codec {
            inner: UviBytes::default(),
        }
    }
}

impl fmt::Debug for Codec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO:
        write!(f, "Codec")
    }
}

impl Encoder for Codec {
    type Item = CircuitRelay;
    type Error = Box<Error>;

    fn encode(&mut self, msg: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let pkg = msg.write_to_bytes()?;
        self.inner.encode(pkg, dst)?;
        Ok(())
    }
}

impl Decoder for Codec {
    type Item = CircuitRelay;
    type Error = Box<Error>;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(p) = self.inner.decode(src)? {
            Ok(Some(protobuf::parse_from_bytes(&p)?))
        } else {
            Ok(None)
        }
    }
}
