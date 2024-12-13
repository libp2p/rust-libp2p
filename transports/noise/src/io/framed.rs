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
//! Alongside a [`asynchronous_codec::Framed`] this provides a [Sink](futures::Sink)
//! and [Stream](futures::Stream) for length-delimited Noise protocol messages.

use std::{io, mem::size_of};

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, Bytes, BytesMut};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};

use super::handshake::proto;
use crate::{protocol::PublicKey, Error};

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
    session: S,

    // We reuse write and encryption buffers across multiple messages to avoid reallocations.
    // We cannot reuse read and decryption buffers because we cannot return borrowed data.
    write_buffer: BytesMut,
    encrypt_buffer: BytesMut,
}

impl<S> Codec<S> {
    pub(crate) fn new(session: S) -> Self {
        Codec {
            session,
            write_buffer: BytesMut::default(),
            encrypt_buffer: BytesMut::default(),
        }
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
    ///
    /// An error is also returned if the remote's static DH key is not present or
    /// cannot be parsed, as that indicates a fatal handshake error for the noise
    /// `XX` pattern, which is the only handshake protocol libp2p currently supports.
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

impl Encoder for Codec<snow::HandshakeState> {
    type Error = io::Error;
    type Item<'a> = &'a proto::NoiseHandshakePayload;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let item_size = item.get_size();

        self.write_buffer.resize(item_size, 0);
        let mut writer = Writer::new(&mut self.write_buffer[..item_size]);
        item.write_message(&mut writer)
            .expect("Protobuf encoding to succeed");

        encrypt(
            &self.write_buffer[..item_size],
            dst,
            &mut self.encrypt_buffer,
            |item, buffer| self.session.write_message(item, buffer),
        )?;

        Ok(())
    }
}

impl Decoder for Codec<snow::HandshakeState> {
    type Error = io::Error;
    type Item = proto::NoiseHandshakePayload;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let cleartext = match decrypt(src, |ciphertext, decrypt_buffer| {
            self.session.read_message(ciphertext, decrypt_buffer)
        })? {
            None => return Ok(None),
            Some(cleartext) => cleartext,
        };

        let mut reader = BytesReader::from_bytes(&cleartext[..]);
        let pb =
            proto::NoiseHandshakePayload::from_reader(&mut reader, &cleartext).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Failed decoding handshake payload",
                )
            })?;

        Ok(Some(pb))
    }
}

impl Encoder for Codec<snow::TransportState> {
    type Error = io::Error;
    type Item<'a> = &'a [u8];

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        encrypt(item, dst, &mut self.encrypt_buffer, |item, buffer| {
            self.session.write_message(item, buffer)
        })
    }
}

impl Decoder for Codec<snow::TransportState> {
    type Error = io::Error;
    type Item = Bytes;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        decrypt(src, |ciphertext, decrypt_buffer| {
            self.session.read_message(ciphertext, decrypt_buffer)
        })
    }
}

/// Encrypts the given cleartext to `dst`.
///
/// This is a standalone function to allow us reusing the `encrypt_buffer` and to use to across
/// different session states of the noise protocol.
fn encrypt(
    cleartext: &[u8],
    dst: &mut BytesMut,
    encrypt_buffer: &mut BytesMut,
    encrypt_fn: impl FnOnce(&[u8], &mut [u8]) -> Result<usize, snow::Error>,
) -> io::Result<()> {
    tracing::trace!("Encrypting {} bytes", cleartext.len());

    encrypt_buffer.resize(cleartext.len() + EXTRA_ENCRYPT_SPACE, 0);
    let n = encrypt_fn(cleartext, encrypt_buffer).map_err(into_io_error)?;

    tracing::trace!("Outgoing ciphertext has {n} bytes");

    encode_length_prefixed(&encrypt_buffer[..n], dst);

    Ok(())
}

/// Encrypts the given ciphertext.
///
/// This is a standalone function so we can use it across different session states of the noise
/// protocol. In case `ciphertext` does not contain enough bytes to decrypt the entire frame,
/// `Ok(None)` is returned.
fn decrypt(
    ciphertext: &mut BytesMut,
    decrypt_fn: impl FnOnce(&[u8], &mut [u8]) -> Result<usize, snow::Error>,
) -> io::Result<Option<Bytes>> {
    let Some(ciphertext) = decode_length_prefixed(ciphertext) else {
        return Ok(None);
    };

    tracing::trace!("Incoming ciphertext has {} bytes", ciphertext.len());

    let mut decrypt_buffer = BytesMut::zeroed(ciphertext.len());
    let n = decrypt_fn(&ciphertext, &mut decrypt_buffer).map_err(into_io_error)?;

    tracing::trace!("Decrypted cleartext has {n} bytes");

    Ok(Some(decrypt_buffer.split_to(n).freeze()))
}

fn into_io_error(err: snow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

const U16_LENGTH: usize = size_of::<u16>();

fn encode_length_prefixed(src: &[u8], dst: &mut BytesMut) {
    dst.reserve(U16_LENGTH + src.len());
    dst.extend_from_slice(&(src.len() as u16).to_be_bytes());
    dst.extend_from_slice(src);
}

fn decode_length_prefixed(src: &mut BytesMut) -> Option<Bytes> {
    if src.len() < size_of::<u16>() {
        return None;
    }

    let mut len_bytes = [0u8; U16_LENGTH];
    len_bytes.copy_from_slice(&src[..U16_LENGTH]);
    let len = u16::from_be_bytes(len_bytes) as usize;

    if src.len() - U16_LENGTH >= len {
        // Skip the length header we already read.
        src.advance(U16_LENGTH);
        Some(src.split_to(len).freeze())
    } else {
        None
    }
}
