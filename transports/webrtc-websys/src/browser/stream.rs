use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use prost::Message;
use tracing::info;

use crate::browser::protocol::pb::SignalingMessage;

/// A wrapper over a async stream for reading and writing SignalingMesssages.
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
    pub async fn write(&mut self, message: SignalingMessage) -> Result<(), std::io::Error> {
        info!("Writing signaling message through signaling stream");
        let mut buf = Vec::new();
        message.encode(&mut buf)?;
        let len = buf.len() as u32;

        self.inner.write_all(&len.to_be_bytes()).await?;
        self.inner.write_all(&buf).await?;
        self.inner.flush().await?;
        Ok(())
    }

    /// Reads and decodes a signaling message from the stream.
    pub async fn read(&mut self) -> Result<SignalingMessage, std::io::Error> {
        info!("Reading signaling message from signaling stream");
        let mut len_buf = [0u8; 4];
        self.inner.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf).await?;

        let message = SignalingMessage::decode(&buf[..])?;
        Ok(message)
    }
}
