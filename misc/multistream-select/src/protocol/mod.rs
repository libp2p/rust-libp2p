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

//! Contains lower-level structs to handle the multistream protocol.

const MSG_MULTISTREAM_1_0: &[u8] = b"/multistream/1.0.0\n";
const MSG_PROTOCOL_NA: &[u8] = b"na\n";
const MSG_LS: &[u8] = b"ls\n";

mod dialer;
mod error;
mod listener;

pub use self::dialer::{Dialer, DialerFuture};
pub use self::error::MultistreamSelectError;
pub use self::listener::{Listener, ListenerFuture};

use bytes::{BytesMut, BufMut};
use unsigned_varint as uvi;

pub enum Header {
    Multistream10
}

impl Header {
    fn encode(&self, dest: &mut BytesMut) {
        match self {
            Header::Multistream10 => {
                dest.reserve(MSG_MULTISTREAM_1_0.len());
                dest.put(MSG_MULTISTREAM_1_0);
            }
        }
    }
}

/// Message sent from the dialer to the listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request<N> {
    /// The dialer wants us to use a protocol.
    ///
    /// If this is accepted (by receiving back a `ProtocolAck`), then we immediately start
    /// communicating in the new protocol.
    Protocol {
        /// Name of the protocol.
        name: N
    },

    /// The dialer requested the list of protocols that the listener supports.
    ListProtocols,
}

impl<N: AsRef<[u8]>> Request<N> {
    fn encode(&self, dest: &mut BytesMut) -> Result<(), MultistreamSelectError> {
        match self {
            Request::Protocol { name } => {
                if !name.as_ref().starts_with(b"/") {
                    return Err(MultistreamSelectError::InvalidProtocolName)
                }
                let len = name.as_ref().len() + 1; // + 1 for \n
                dest.reserve(len);
                dest.put(name.as_ref());
                dest.put(&b"\n"[..]);
                Ok(())
            }
            Request::ListProtocols => {
                dest.reserve(MSG_LS.len());
                dest.put(MSG_LS);
                Ok(())
            }
        }
    }
}


/// Message sent from the listener to the dialer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response<N> {
    /// The protocol requested by the dialer is accepted. The socket immediately starts using the
    /// new protocol.
    Protocol { name: N },

    /// The protocol requested by the dialer is not supported or available.
    ProtocolNotAvailable,

    /// Response to the request for the list of protocols.
    SupportedProtocols {
        /// The list of protocols.
        // TODO: use some sort of iterator
        protocols: Vec<N>,
    },
}

impl<N: AsRef<[u8]>> Response<N> {
    fn encode(&self, dest: &mut BytesMut) -> Result<(), MultistreamSelectError> {
        match self {
            Response::Protocol { name } => {
                if !name.as_ref().starts_with(b"/") {
                    return Err(MultistreamSelectError::InvalidProtocolName)
                }
                let len = name.as_ref().len() + 1; // + 1 for \n
                dest.reserve(len);
                dest.put(name.as_ref());
                dest.put(&b"\n"[..]);
                Ok(())
            }
            Response::SupportedProtocols { protocols } => {
                let mut buf = uvi::encode::usize_buffer();
                let mut out_msg = Vec::from(uvi::encode::usize(protocols.len(), &mut buf));
                for p in protocols {
                    out_msg.extend(uvi::encode::usize(p.as_ref().len() + 1, &mut buf)); // +1 for '\n'
                    out_msg.extend_from_slice(p.as_ref());
                    out_msg.push(b'\n')
                }
                dest.reserve(out_msg.len());
                dest.put(out_msg);
                Ok(())
            }
            Response::ProtocolNotAvailable => {
                dest.reserve(MSG_PROTOCOL_NA.len());
                dest.put(MSG_PROTOCOL_NA);
                Ok(())
            }
        }
    }
}


