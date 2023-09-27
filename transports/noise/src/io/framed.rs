// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! Provides a [`Codec`] type implementing the [`Encoder`] and [`Decoder`] traits.
//!
//! Alongside a [`asynchronous_codec::Framed`] this provides a [`Sink`] and [`Stream`]
//! for length-delimited Noise protocol messages.

use super::handshake::proto;
use crate::{protocol::PublicKey, Error};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use log::{debug, error};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::io;

/// Max. size of a noise message.
const MAX_NOISE_MSG_LEN: usize = 65535;
/// Space given to the encryption buffer to hold key material.
const EXTRA_ENCRYPT_SPACE: usize = 1024;
/// Max. length for Noise protocol message payloads.
pub(crate) const MAX_FRAME_LEN: usize = MAX_NOISE_MSG_LEN - EXTRA_ENCRYPT_SPACE;
static_assertions::const_assert! {
    MAX_FRAME_LEN + EXTRA_ENCRYPT_SPACE <= MAX_NOISE_MSG_LEN
}

/// Codec holds the noise session state `S` and acts as a medium for
/// encoding and decoding length-delimited session messages.
pub(crate) struct Codec<S> {
    write_buffer: Vec<u8>,
    decrypt_buffer: BytesMut,
    session: S,
}

impl<S: SessionState> Codec<S> {
    pub(crate) fn new(session: S) -> Self {
        Codec {
            session,
            write_buffer: Vec::new(),
            decrypt_buffer: BytesMut::new(),
        }
    }

    fn encode_bytes(&mut self, item: &Vec<u8>, dst: &mut BytesMut) -> Result<(), io::Error> {
        self.write_buffer
            .resize(item.len() + EXTRA_ENCRYPT_SPACE, 0);
        let n = match self.session.write_message(item, &mut self.write_buffer) {
            Ok(n) => n,
            Err(e) => {
                error!("encryption error: {:?}", e);
                return Err(io::ErrorKind::InvalidData.into());
            }
        };

        let prefix = u16::to_be_bytes(n as u16);
        self.write_buffer.truncate(n);

        dst.put(&prefix[..]);
        dst.put(&self.write_buffer[..]);

        Ok(())
    }

    fn decode_bytes(&mut self, src: &mut BytesMut) -> Result<Option<Bytes>, io::Error> {
        if src.len() < 2 {
            return Ok(None);
        }

        let len = u16::from_be_bytes([src[0], src[1]]) as usize;
        if len == 0 || src.len() < len + 2 {
            return Ok(None);
        }

        src.advance(2);
        self.decrypt_buffer.resize(len, 0u8);
        let n = match self
            .session
            .read_message(&src[0..len], &mut self.decrypt_buffer)
        {
            Ok(n) => n,
            Err(e) => {
                debug!("decryption error {e}");
                return Err(io::ErrorKind::InvalidData.into());
            }
        };

        self.decrypt_buffer.truncate(n);
        src.advance(len);

        Ok(Some(self.decrypt_buffer.split().freeze()))
    }
}

impl Codec<snow::HandshakeState> {
    /// Checks if the session was started in the `initiator` role.
    pub(crate) fn is_initiator(&self) -> bool {
        self.session.is_initiator()
    }

    /// Checks if the session was started in the `responder` role.
    pub(crate) fn is_responder(&self) -> bool {
        !self.session.is_initiator()
    }

    /// Converts the underlying Noise session from the [`snow::HandshakeState`] to a
    /// [`snow::TransportState`] once the handshake is complete, including the static
    /// DH [`PublicKey`] of the remote if received.
    ///
    /// If the Noise protocol session state does not permit transitioning to
    /// transport mode because the handshake is incomplete, an error is returned.
    /// Similarly if the remote's static DH key, if present, cannot be parsed.
    pub(crate) fn into_transport(self) -> Result<(PublicKey, Codec<snow::TransportState>), Error> {
        let dh_remote_pubkey = self.session.get_remote_static().ok_or_else(|| {
            Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "expect key to always be present at end of XX session",
            ))
        })?;

        let dh_remote_pubkey = PublicKey::from_slice(dh_remote_pubkey)?;
        let codec = Codec::new(self.session.into_transport_mode()?);

        Ok((dh_remote_pubkey, codec))
    }
}

impl asynchronous_codec::Encoder for Codec<snow::HandshakeState> {
    type Error = io::Error;
    type Item<'a> = &'a proto::NoiseHandshakePayload;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut buf = Vec::with_capacity(item.get_size());

        let mut writer = Writer::new(&mut buf);
        item.write_message(&mut writer)
            .expect("Encoding to succeed");
        self.encode_bytes(&buf, dst)
    }
}
impl asynchronous_codec::Decoder for Codec<snow::HandshakeState> {
    type Error = io::Error;
    type Item = proto::NoiseHandshakePayload;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode_bytes(src)? {
            Some(bytes) => {
                let mut reader = BytesReader::from_bytes(&bytes[..]);
                let pb = proto::NoiseHandshakePayload::from_reader(&mut reader, &bytes[..])
                    .map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Failed decoding handshake payload",
                        )
                    })?;
                Ok(Some(pb))
            }
            None => Ok(None),
        }
    }
}

impl asynchronous_codec::Encoder for Codec<snow::TransportState> {
    type Error = io::Error;
    type Item<'a> = &'a Vec<u8>;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.encode_bytes(item, dst)
    }
}
impl asynchronous_codec::Decoder for Codec<snow::TransportState> {
    type Error = io::Error;
    type Item = Bytes;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decode_bytes(src)
    }
}

/// A stateful context in which Noise protocol messages can be read and written.
pub(crate) trait SessionState {
    /// Decrypt the payload `msg` with the session state and read the result to `buf`.
    fn read_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error>;

    /// Encrypt the payload `msg` with the session state and write the result to `buf`.
    fn write_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error>;
}

impl SessionState for snow::HandshakeState {
    fn read_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.read_message(msg, buf)
    }

    fn write_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.write_message(msg, buf)
    }
}

impl SessionState for snow::TransportState {
    fn read_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.read_message(msg, buf)
    }

    fn write_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.write_message(msg, buf)
    }
}
