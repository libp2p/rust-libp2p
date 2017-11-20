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

use crypto::symmetriccipher::SymmetricCipherError;
use std::error;
use std::fmt;
use std::io::Error as IoError;

/// Error at the SECIO layer communication.
#[derive(Debug)]
pub enum SecioError {
	/// I/O error.
	IoError(IoError),

	/// Failed to parse one of the handshake protobuf messages.
	HandshakeParsingFailure,

	/// There is no protocol supported by both the local and remote hosts.
	NoSupportIntersection(&'static str, String),

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

	/// Error while decoding/encoding data.
	CipherError(SymmetricCipherError),

	/// The received frame was of invalid length.
	FrameTooShort,

	/// The hashes of the message didn't match.
	HmacNotMatching,
}

impl error::Error for SecioError {
	#[inline]
	fn description(&self) -> &str {
		match *self {
			SecioError::IoError(_) => {
				"I/O error"
			}
			SecioError::HandshakeParsingFailure => {
				"Failed to parse one of the handshake protobuf messages"
			}
			SecioError::NoSupportIntersection(_, _) => {
				"There is no protocol supported by both the local and remote hosts"
			}
			SecioError::NonceGenerationFailed => {
				"Failed to generate nonce"
			}
			SecioError::EphemeralKeyGenerationFailed => {
				"Failed to generate ephemeral key"
			}
			SecioError::SigningFailure => {
				"Failed to sign a message with our local private key"
			}
			SecioError::SignatureVerificationFailed => {
				"The signature of the exchange packet doesn't verify the remote public key"
			}
			SecioError::SecretGenerationFailed => {
				"Failed to generate the secret shared key from the ephemeral key"
			}
			SecioError::NonceVerificationFailed => {
				"The final check of the handshake failed"
			}
			SecioError::CipherError(_) => {
				"Error while decoding/encoding data"
			}
			SecioError::FrameTooShort => {
				"The received frame was of invalid length"
			}
			SecioError::HmacNotMatching => {
				"The hashes of the message didn't match"
			}
		}
	}

	fn cause(&self) -> Option<&error::Error> {
		match *self {
			SecioError::IoError(ref err) => {
				Some(err)
			}
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
	fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(fmt, "{}", error::Error::description(self))
	}
}

impl From<SymmetricCipherError> for SecioError {
	#[inline]
	fn from(err: SymmetricCipherError) -> SecioError {
		SecioError::CipherError(err)
	}
}

impl From<IoError> for SecioError {
	#[inline]
	fn from(err: IoError) -> SecioError {
		SecioError::IoError(err)
	}
}
