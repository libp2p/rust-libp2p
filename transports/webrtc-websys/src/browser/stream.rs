use std::io;

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};

use crate::browser::protocol::proto::signaling::SignalingMessage;

/// A wrapper over an async stream for reading and writing `SignalingMessage`s.
///
/// Each message is length-prefixed with an unsigned-varint, per the
/// [WebRTC signaling spec](https://github.com/libp2p/specs/blob/master/webrtc/webrtc.md#signaling-protocol).
pub struct SignalingStream<T> {
    inner: T,
}

impl<T> SignalingStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Encodes and writes a signaling message to the stream.
    pub async fn write(&mut self, message: SignalingMessage) -> io::Result<()> {
        write_message(&mut self.inner, message).await
    }

    /// Reads and decodes a signaling message from the stream.
    pub async fn read(&mut self) -> io::Result<SignalingMessage> {
        read_message(&mut self.inner).await
    }

    /// Closes the writable half of the stream, signalling EOF to the remote.
    pub async fn close(&mut self) -> io::Result<()> {
        self.inner.close().await
    }

    /// Consumes self and returns the underlying stream.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

/// Encodes a `SignalingMessage` with an unsigned-varint length prefix and writes it.
pub(crate) async fn write_message<W>(mut writer: W, message: SignalingMessage) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    let mut w = Writer::new(&mut buf);
    message
        .write_message(&mut w)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut len_buf = unsigned_varint::encode::usize_buffer();
    let len_bytes = unsigned_varint::encode::usize(buf.len(), &mut len_buf);

    writer.write_all(len_bytes).await?;
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads a length-prefixed `SignalingMessage` from the stream.
pub(crate) async fn read_message<R>(mut reader: R) -> io::Result<SignalingMessage>
where
    R: AsyncRead + Unpin,
{
    let len = unsigned_varint::aio::read_usize(&mut reader)
        .await
        .map_err(|e| match e {
            unsigned_varint::io::ReadError::Io(io) => io,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        })?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;

    let mut r = BytesReader::from_bytes(&buf);
    SignalingMessage::from_reader(&mut r, &buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
