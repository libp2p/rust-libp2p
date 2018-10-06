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

use bytes::BytesMut;
use serde_derive::{Serialize, Deserialize};
use std::{error, fmt, io};
use tokio_codec::{Decoder, Encoder};
use unsigned_varint::codec::UviBytes;

/// Encoder and decoder for protocol messages.
pub struct Codec {
    inner: UviBytes<Vec<u8>>,
}

impl Default for Codec {
    #[inline]
    fn default() -> Self {
        let mut inner = UviBytes::default();
        inner.set_max_len(1024);
        Codec {
            inner,
        }
    }
}

impl fmt::Debug for Codec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BrahmsCodec")
    }
}

impl Encoder for Codec {
    type Item = RawMessage;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let tmp_buf = serde_cbor::to_vec(&item).expect("encoding with serde_cbor cannot fail; QED");
        self.inner.encode(tmp_buf, dst)?;
        Ok(())
    }
}

impl Decoder for Codec {
    type Item = RawMessage;
    type Error = Box<error::Error + Send + Sync>;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(item) = self.inner.decode(src)? {
            Ok(Some(serde_cbor::from_slice(&item)?))
        } else {
            Ok(None)
        }
    }
}

/// Message that can be transmitted over a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RawMessage {
    /// Pushes the sender to a remote. Contains the addresses it's listening on.
    Push(Vec<Vec<u8>>),

    /// Sender requests the remote to send back their view of the network.
    PullRequest,

    /// Response to a `PullRequest`. Contains the peers and addresses how to reach them.
    PullResponse(Vec<(Vec<u8>, Vec<Vec<u8>>)>),
}
