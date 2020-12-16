//! Allows compression algorithms to be added to the gossipsub layer.

#[cfg(feature = "snappy")]
use snap::raw::{decompress_len, Decoder, Encoder};

pub trait MessageCompression {
    fn compress_message(&self, data: Vec<u8>) -> Result<Vec<u8>, CompressionError>;

    fn decompress_message(
        &self,
        data: Vec<u8>,
        max_len: usize,
    ) -> Result<Vec<u8>, CompressionError>;
}

#[derive(Debug)]
pub enum CompressionError {
    /// The decompressed contents are too large.
    DecompressionTooLarge,
    /// A custom error type.
    Custom(String),
}

#[cfg(feature = "snappy")]
impl From<snap::Error> for CompressionError {
    fn from(error: snap::Error) -> CompressionError {
        CompressionError::Custom(error.to_string())
    }
}

/// The default for gossipsub.
#[derive(Default, Clone)]
pub struct NoCompression;

impl MessageCompression for NoCompression {
    fn compress_message(&self, data: Vec<u8>) -> Result<Vec<u8>, CompressionError> {
        Ok(data)
    }

    fn decompress_message(
        &self,
        data: Vec<u8>,
        _max_len: usize,
    ) -> Result<Vec<u8>, CompressionError> {
        Ok(data)
    }
}

/// Optional Snappy compression
#[cfg(feature = "snappy")]
#[derive(Default, Clone)]
pub struct SnappyCompression;

#[cfg(feature = "snappy")]
impl MessageCompression for SnappyCompression {
    fn decompress_message(
        &self,
        data: Vec<u8>,
        max_len: usize,
    ) -> Result<Vec<u8>, CompressionError> {
        // Exit early if uncompressed data is > max_len
        match decompress_len(&data) {
            Ok(n) if n > max_len => {
                return Err(CompressionError::DecompressionTooLarge);
            }
            Ok(_) => {}
            Err(e) => {
                return Err(CompressionError::Custom(e.to_string()));
            }
        };
        let mut decoder = Decoder::new();
        decoder.decompress_vec(&data).map_err(|e| e.into())
    }

    fn compress_message(&self, data: Vec<u8>) -> Result<Vec<u8>, CompressionError> {
        let mut encoder = Encoder::new();
        encoder.compress_vec(&data).map_err(|e| e.into())
    }
}
