// Copyright 2019 Parity Technologies (UK) Ltd.
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

use libp2p_core::identity;
use std::io;

/// libp2p_noise error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NoiseError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Noise(#[from] snow::Error),
    #[error("Invalid public key")]
    InvalidKey,
    #[error("Authentication failed")]
    AuthenticationFailed,
    #[error(transparent)]
    InvalidPayload(#[from] prost::DecodeError),
    #[error(transparent)]
    SigningError(#[from] identity::error::SigningError),
}
