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

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::prelude::*;
use libp2p_core::ProtocolName;
use minicbor::{Encode, Decode};
use std::io;
use super::RequestResponseCodec;
use unsigned_varint::{aio, io::ReadError};

/// A protocol header.
#[derive(Debug, Default, Clone, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct Header {
    /// The type of message.
    #[n(0)] pub typ: Option<Type>,
    /// The number of additional requests the remote is willing to receive.
    #[n(1)] pub credit: Option<u16>,
    /// An identifier used for sending credit grants.
    #[n(2)] pub ident: Option<u64>
}

/// A protocol message type.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Type {
    #[n(0)] Request,
    #[n(1)] Response,
    #[n(2)] Credit,
    #[n(3)] Ack
}

/// A protocol message consisting of header and data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message<T> {
    header: Header,
    data: Option<T>
}

impl<T> Message<T> {
    /// Create a new message of some type.
    fn new(header: Header) -> Self {
        Message { header, data: None }
    }

    /// Create a request message.
    pub fn request(data: T) -> Self {
        let mut m = Message::new(Header { typ: Some(Type::Request), .. Header::default() });
        m.data = Some(data);
        m
    }

    /// Create a response message.
    pub fn response(data: T) -> Self {
        let mut m = Message::new(Header { typ: Some(Type::Response), .. Header::default() });
        m.data = Some(data);
        m
    }

    /// Create a credit grant.
    pub fn credit(credit: u16, ident: u64) -> Self {
        Message::new(Header { typ: Some(Type::Credit), credit: Some(credit), ident: Some(ident) })
    }

    /// Create an acknowledge message.
    pub fn ack(ident: u64) -> Self {
        Message::new(Header { typ: Some(Type::Ack), credit: None, ident: Some(ident) })
    }

    /// Access the message header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Access the message data.
    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    /// Consume this message and return header and data.
    pub fn into_parts(self) -> (Header, Option<T>) {
        (self.header, self.data)
    }
}

/// A wrapper around a `ProtocolName` impl which augments the protocol name.
///
/// The type implements `ProtocolName` itself and creates a name for a
/// request-response protocol based on the protocol name of the wrapped type.
#[derive(Debug, Clone)]
pub struct ProtocolWrapper<P>(P, Bytes);

impl<P: ProtocolName> ProtocolWrapper<P> {
    pub fn new(prefix: &[u8], p: P) -> Self {
        let mut full = BytesMut::from(prefix);
        full.extend_from_slice(p.protocol_name());
        ProtocolWrapper(p, full.freeze())
    }
}

impl<P> ProtocolName for ProtocolWrapper<P> {
    fn protocol_name(&self) -> &[u8] {
        self.1.as_ref()
    }
}

/// A `RequestResponseCodec` wrapper that adds headers to the payload data.
#[derive(Debug, Clone)]
pub struct Codec<C> {
    /// The wrapped codec.
    inner: C,
    /// Encoding/decoding buffer.
    buffer: Vec<u8>,
    /// Max. header length.
    max_header_len: u32
}

impl<C> Codec<C> {
    /// Create a codec by wrapping an existing one.
    pub fn new(c: C, max_header_len: u32) -> Self {
        Codec { inner: c, buffer: Vec::new(), max_header_len }
    }

    /// Read and decode a request header.
    async fn read_header<T, H>(&mut self, io: &mut T) -> io::Result<H>
    where
        T: AsyncRead + Unpin + Send,
        H: for<'a> minicbor::Decode<'a>
    {
        let header_len = aio::read_u32(&mut *io).await
            .map_err(|e| match e {
                ReadError::Io(e) => e,
                other => io::Error::new(io::ErrorKind::Other, other)
            })?;
        if header_len > self.max_header_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "header too large to read"))
        }
        self.buffer.resize(u32_to_usize(header_len), 0u8);
        io.read_exact(&mut self.buffer).await?;
        minicbor::decode(&self.buffer).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    /// Encode and write a response header.
    async fn write_header<T, H>(&mut self, hdr: &H, io: &mut T) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
        H: minicbor::Encode
    {
        self.buffer.clear();
        minicbor::encode(hdr, &mut self.buffer).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        if self.buffer.len() > u32_to_usize(self.max_header_len) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "header too large to write"))
        }
        let mut b = unsigned_varint::encode::u32_buffer();
        let header_len = unsigned_varint::encode::u32(self.buffer.len() as u32, &mut b);
        io.write_all(header_len).await?;
        io.write_all(&self.buffer).await
    }
}

#[async_trait]
impl<C> RequestResponseCodec for Codec<C>
where
    C: RequestResponseCodec + Send,
    C::Protocol: Sync
{
    type Protocol = ProtocolWrapper<C::Protocol>;
    type Request = Message<C::Request>;
    type Response = Message<C::Response>;

    async fn read_request<T>(&mut self, p: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send
    {
        let mut msg = Message::new(self.read_header(io).await?);
        match msg.header.typ {
            Some(Type::Request) => {
                msg.data = Some(self.inner.read_request(&p.0, io).await?);
                Ok(msg)
            }
            Some(Type::Credit) => Ok(msg),
            Some(Type::Response) | Some(Type::Ack) | None => {
                log::debug!("unexpected {:?} when expecting request or credit grant", msg.header.typ);
                Err(io::ErrorKind::InvalidData.into())
            }
        }
    }

    async fn read_response<T>(&mut self, p: &Self::Protocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send
    {
        let mut msg = Message::new(self.read_header(io).await?);
        match msg.header.typ {
            Some(Type::Response) => {
                msg.data = Some(self.inner.read_response(&p.0, io).await?);
                Ok(msg)
            }
            Some(Type::Ack) => Ok(msg),
            Some(Type::Request) | Some(Type::Credit) | None => {
                log::debug!("unexpected {:?} when expecting response or ack", msg.header.typ);
                Err(io::ErrorKind::InvalidData.into())
            }
        }
    }

    async fn write_request<T>(&mut self, p: &Self::Protocol, io: &mut T, r: Self::Request) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send
    {
        self.write_header(&r.header, io).await?;
        if let Some(data) = r.data {
            self.inner.write_request(&p.0, io, data).await?
        }
        Ok(())
    }

    async fn write_response<T>(&mut self, p: &Self::Protocol, io: &mut T, r: Self::Response) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send
    {
        self.write_header(&r.header, io).await?;
        if let Some(data) = r.data {
            self.inner.write_response(&p.0, io, data).await?
        }
        Ok(())
    }
}

#[cfg(any(target_pointer_width = "64", target_pointer_width = "32"))]
fn u32_to_usize(n: u32) -> usize {
    n as usize
}
