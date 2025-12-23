//! Simple framing layer for sending length-prefixed messages over BLE characteristics.
//!
//! BLE characteristics have limited MTU sizes (typically 20-512 bytes), so we need
//! to frame our messages properly. This module provides a simple length-prefix framing
//! where each message is prefixed with a 4-byte big-endian length.

use bytes::{Buf, BufMut, BytesMut};
use std::io;

/// Maximum frame size (1MB - prevents DoS)
const MAX_FRAME_SIZE: usize = 1024 * 1024;

/// Frame encoder/decoder for length-prefixed messages
pub(crate) struct FrameCodec {
    buffer: BytesMut,
}

impl FrameCodec {
    pub(crate) fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(4096),
        }
    }

    /// Encode a message with length prefix
    pub(crate) fn encode(&self, data: &[u8]) -> Result<Vec<u8>, io::Error> {
        if data.len() > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "frame too large",
            ));
        }

        let mut buf = Vec::with_capacity(4 + data.len());
        buf.put_u32(data.len() as u32);
        buf.extend_from_slice(data);
        Ok(buf)
    }

    /// Add incoming data to the buffer
    pub(crate) fn push_data(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Try to decode the next complete frame from the buffer
    pub(crate) fn decode_next(&mut self) -> Result<Option<Vec<u8>>, io::Error> {
        // Need at least 4 bytes for length prefix
        if self.buffer.len() < 4 {
            return Ok(None);
        }

        // Peek at the length without consuming
        let mut length_bytes = &self.buffer[..4];
        let frame_len = length_bytes.get_u32() as usize;

        // Validate frame size
        if frame_len > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "frame size {} exceeds maximum {}",
                    frame_len, MAX_FRAME_SIZE
                ),
            ));
        }

        // Check if we have the complete frame
        let total_len = 4 + frame_len;
        if self.buffer.len() < total_len {
            return Ok(None); // Need more data
        }

        // Consume the length prefix
        self.buffer.advance(4);

        // Extract the frame data
        let frame = self.buffer.split_to(frame_len).to_vec();

        Ok(Some(frame))
    }
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let codec = FrameCodec::new();
        let data = b"hello world";

        // Encode
        let encoded = codec.encode(data).unwrap();
        assert_eq!(encoded.len(), 4 + data.len());

        // Decode
        let mut decoder = FrameCodec::new();
        decoder.push_data(&encoded);
        let decoded = decoder.decode_next().unwrap().unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_partial_frames() {
        let codec = FrameCodec::new();
        let data = b"hello world";
        let encoded = codec.encode(data).unwrap();

        let mut decoder = FrameCodec::new();

        // Push only part of the data
        decoder.push_data(&encoded[..5]);
        assert!(decoder.decode_next().unwrap().is_none());

        // Push the rest
        decoder.push_data(&encoded[5..]);
        let decoded = decoder.decode_next().unwrap().unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_multiple_frames() {
        let codec = FrameCodec::new();
        let data1 = b"hello";
        let data2 = b"world";

        let encoded1 = codec.encode(data1).unwrap();
        let encoded2 = codec.encode(data2).unwrap();

        let mut decoder = FrameCodec::new();
        decoder.push_data(&encoded1);
        decoder.push_data(&encoded2);

        let decoded1 = decoder.decode_next().unwrap().unwrap();
        assert_eq!(decoded1, data1);

        let decoded2 = decoder.decode_next().unwrap().unwrap();
        assert_eq!(decoded2, data2);
    }

    #[test]
    fn test_max_frame_size() {
        let codec = FrameCodec::new();
        let data = vec![0u8; MAX_FRAME_SIZE + 1];
        assert!(codec.encode(&data).is_err());
    }
}
