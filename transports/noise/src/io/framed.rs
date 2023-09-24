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

//! This module provides a `Sink` and `Stream` for length-delimited
//! Noise protocol messages in form of [`NoiseFramed`].

use crate::io::Output;
use crate::{protocol::PublicKey, Error};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::prelude::*;
use log::{debug, error};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

/// Max size of a noise message.
const MAX_NOISE_MSG_LEN: usize = 65535;
/// Space given to the encryption buffer to hold key material.
const EXTRA_ENCRYPT_SPACE: usize = 1024;
/// Max. length for Noise protocol message payloads.
pub(crate) const MAX_FRAME_LEN: usize = MAX_NOISE_MSG_LEN - EXTRA_ENCRYPT_SPACE;
static_assertions::const_assert! {
    MAX_FRAME_LEN + EXTRA_ENCRYPT_SPACE <= MAX_NOISE_MSG_LEN
}

/// A `NoiseFramed` is a `Sink` and `Stream` for length-delimited
/// Noise protocol messages.
///
/// `T` is the type of the underlying I/O resource and `S` the
/// type of the Noise session state.
#[pin_project::pin_project]
pub(crate) struct NoiseFramed<T, S> {
    #[pin]
    io: asynchronous_codec::Framed<T, Codec<S>>,
}

impl<T> NoiseFramed<T, snow::HandshakeState>
where
    T: AsyncRead + AsyncWrite,
{
    /// Creates a new `NoiseFramed` for beginning a Noise protocol handshake.
    pub(crate) fn new(io: T, state: snow::HandshakeState) -> Self {
        NoiseFramed {
            io: asynchronous_codec::Framed::new(
                io,
                Codec {
                    session: state,
                    write_buffer: Vec::new(),
                    decrypt_buffer: BytesMut::new(),
                },
            ),
        }
    }

    /// Checks if the underlying noise session was started in the `initiator` role.
    pub(crate) fn is_initiator(&self) -> bool {
        self.io.codec().session.is_initiator()
    }

    /// Checks if the underlying noise session was started in the `responder` role.
    pub(crate) fn is_responder(&self) -> bool {
        !self.io.codec().session.is_initiator()
    }

    /// Converts the `NoiseFramed` into a `NoiseOutput` encrypted data stream
    /// once the handshake is complete, including the static DH [`PublicKey`]
    /// of the remote, if received.
    ///
    /// If the underlying Noise protocol session state does not permit
    /// transitioning to transport mode because the handshake is incomplete,
    /// an error is returned. Similarly if the remote's static DH key, if
    /// present, cannot be parsed.
    pub(crate) fn into_transport(self) -> Result<(PublicKey, Output<T>), Error> {
        let dh_remote_pubkey = self.io.codec().session.get_remote_static().ok_or_else(|| {
            Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "expect key to always be present at end of XX session",
            ))
        })?;

        let dh_remote_pubkey = PublicKey::from_slice(dh_remote_pubkey)?;
        let framed_parts = self.io.into_parts();

        let io = NoiseFramed {
            io: asynchronous_codec::Framed::new(
                framed_parts.io,
                Codec {
                    session: framed_parts.codec.session.into_transport_mode()?,
                    write_buffer: Vec::new(),
                    decrypt_buffer: BytesMut::new(),
                },
            ),
        };

        Ok((dh_remote_pubkey, Output::new(io)))
    }
}

/// Codec for writing and reading length-delimited noise messages.
pub(crate) struct Codec<S> {
    write_buffer: Vec<u8>,
    decrypt_buffer: BytesMut,
    session: S,
}

impl<S> asynchronous_codec::Encoder for Codec<S>
where
    S: SessionState,
{
    type Error = io::Error;
    type Item = Vec<u8>;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.write_buffer
            .resize(item.len() + EXTRA_ENCRYPT_SPACE, 0);
        let n = match self.session.write_message(&item, &mut self.write_buffer) {
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
}

impl<S> asynchronous_codec::Decoder for Codec<S>
where
    S: SessionState,
{
    type Error = io::Error;
    type Item = Bytes;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
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
                debug!("read: decryption error {e}");
                return Err(io::ErrorKind::InvalidData.into());
            }
        };

        self.decrypt_buffer.truncate(n);
        src.advance(len);

        Ok(Some(self.decrypt_buffer.split().freeze()))
    }
}

impl<T, S> futures::stream::Stream for NoiseFramed<T, S>
where
    T: AsyncRead + Unpin,
    S: SessionState,
{
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.io.poll_next_unpin(cx)
    }
}
impl<T, S> futures::sink::Sink<Vec<u8>> for NoiseFramed<T, S>
where
    T: AsyncWrite + Unpin,
    S: SessionState,
{
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().io.poll_ready(cx)
    }
    fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        self.project().io.start_send(item)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().io.poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().io.poll_close(cx)
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
