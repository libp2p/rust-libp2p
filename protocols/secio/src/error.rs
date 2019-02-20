// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Defines the `SecioError` enum that groups all possible errors in SECIO.

use aes_ctr::stream_cipher::LoopError;
use libp2p_core::identity::error::EncodingError;
use protobuf::error::ProtobufError;
use std::error;
use std::fmt;
use std::io::Error as IoError;

/// Error at the SECIO layer communication.
#[derive(Debug)]
pub enum SecioError {
    /// I/O error.
    IoError(IoError),

    /// Protocol buffer error.
    ProtobufError(ProtobufError),

    /// Key encoding error.
    KeyEncodingError(EncodingError),

    /// Failed to parse one of the handshake protobuf messages.
    HandshakeParsingFailure,

    /// There is no protocol supported by both the local and remote hosts.
    NoSupportIntersection,

    /// Failed to generate nonce.
    NonceGenerationFailed,

    /// Failed to generate ephemeral key.
    EphemeralKeyGenerationFailed,

    /// Failed to sign a message with our local private key.
    SigningFailure,

    /// The signature of the exchange packet doesn't verify the remote public key.
    SignatureVerificationFailed,

    /// Failed to generate the secret shared key from the ephemeral key.
    SecretGenerationFailed,

    /// The final check of the handshake failed.
    NonceVerificationFailed,

    /// Error with block cipher.
    CipherError(LoopError),

    /// The received frame was of invalid length.
    FrameTooShort,

    /// The hashes of the message didn't match.
    HmacNotMatching,

    /// We received an invalid proposition from remote.
    InvalidProposition(&'static str),

    #[doc(hidden)]
    __Nonexhaustive
}

impl error::Error for SecioError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            SecioError::IoError(ref err) => Some(err),
            SecioError::ProtobufError(ref err) => Some(err),
            SecioError::KeyEncodingError(ref err) => Some(err),
            // TODO: The type doesn't implement `Error`
            /*SecioError::CipherError(ref err) => {
                Some(err)
            },*/
            _ => None,
        }
    }
}

impl fmt::Display for SecioError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            SecioError::IoError(e) =>
                write!(f, "I/O error: {}", e),
            SecioError::ProtobufError(e) =>
                write!(f, "Protobuf error: {}", e),
            SecioError::KeyEncodingError(e) =>
                write!(f, "Key encoding error: {}", e),
            SecioError::HandshakeParsingFailure =>
                f.write_str("Failed to parse one of the handshake protobuf messages"),
            SecioError::NoSupportIntersection =>
                f.write_str("There is no protocol supported by both the local and remote hosts"),
            SecioError::NonceGenerationFailed =>
                f.write_str("Failed to generate nonce"),
            SecioError::EphemeralKeyGenerationFailed =>
                f.write_str("Failed to generate ephemeral key"),
            SecioError::SigningFailure =>
                f.write_str("Failed to sign a message with our local private key"),
            SecioError::SignatureVerificationFailed =>
                f.write_str("The signature of the exchange packet doesn't verify the remote public key"),
            SecioError::SecretGenerationFailed =>
                f.write_str("Failed to generate the secret shared key from the ephemeral key"),
            SecioError::NonceVerificationFailed =>
                f.write_str("The final check of the handshake failed"),
            SecioError::CipherError(e) =>
                write!(f, "Error while decoding/encoding data: {:?}", e),
            SecioError::FrameTooShort =>
                f.write_str("The received frame was of invalid length"),
            SecioError::HmacNotMatching =>
                f.write_str("The hashes of the message didn't match"),
            SecioError::InvalidProposition(msg) =>
                write!(f, "invalid proposition: {}", msg),
            SecioError::__Nonexhaustive =>
                f.write_str("__Nonexhaustive")
        }
    }
}

impl From<LoopError> for SecioError {
    #[inline]
    fn from(err: LoopError) -> SecioError {
        SecioError::CipherError(err)
    }
}

impl From<IoError> for SecioError {
    #[inline]
    fn from(err: IoError) -> SecioError {
        SecioError::IoError(err)
    }
}

impl From<ProtobufError> for SecioError {
    #[inline]
    fn from(err: ProtobufError) -> SecioError {
        SecioError::ProtobufError(err)
    }
}

impl From<EncodingError> for SecioError {
    fn from(err: EncodingError) -> SecioError {
        SecioError::KeyEncodingError(err)
    }
}
