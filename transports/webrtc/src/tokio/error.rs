// Copyright 2022 Parity Technologies (UK) Ltd.
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

use libp2p_identity::PeerId;
use thiserror::Error;

/// Error in WebRTC.
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    WebRTC(#[from] webrtc::Error),
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("failed to authenticate peer")]
    Authentication(#[from] libp2p_noise::Error),

    // Authentication errors.
    #[error("invalid peer ID (expected {expected}, got {got})")]
    InvalidPeerID { expected: PeerId, got: PeerId },

    #[error("no active listeners, can not dial without a previous listen")]
    NoListeners,

    #[error("UDP mux error: {0}")]
    UDPMux(std::io::Error),

    #[error("internal error: {0} (see debug logs)")]
    Internal(String),
}
