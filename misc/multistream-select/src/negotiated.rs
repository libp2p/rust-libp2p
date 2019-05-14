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

use crate::protocol::{Dialer, ListenerToDialerMessage};
use futures::prelude::*;
use std::{io, mem};

/// A stream after it has been negotiated.
pub struct Negotiated<TInner>(NegotiatedInner<TInner>);

enum NegotiatedInner<TInner> {
    /// We have received confirmation that the protocol is negotiated.
    Finished(TInner),

    /// We are waiting for the remote to send back a confirmation that the negotiation has been
    /// successful.
    Negotiating {
        /// The stream of data.
        inner: Dialer<TInner, Vec<u8>>,
        /// Expected protocol name.
        expected_name: Vec<u8>,
    },

    /// Temporary state used for transitioning. Should never be observed.
    Poisoned,
}

impl<TInner> Negotiated<TInner> {
    /// Builds a `Negotiated` containing a stream that's already been successfully negotiated to
    /// a specific protocol.
    pub(crate) fn finished(inner: TInner) -> Self {
        Negotiated(NegotiatedInner::Finished(inner))
    }

    /// Builds a `Negotiated` expecting a successful protocol negotiation answer.
    pub(crate) fn negotiating<P>(inner: Dialer<TInner, P>, expected_name: Vec<u8>) -> Self
    where P: AsRef<[u8]>,
          TInner: tokio_io::AsyncRead + tokio_io::AsyncWrite,
    {
        Negotiated(NegotiatedInner::Negotiating {
            inner: inner.map_param(),
            expected_name,
        })
    }
}

impl<TInner> io::Read for Negotiated<TInner>
where
    TInner: tokio_io::AsyncRead
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.0 {
                NegotiatedInner::Finished(ref mut s) => return s.read(buf),
                NegotiatedInner::Negotiating { ref mut inner, ref expected_name } => {
                    let msg = match inner.poll() {
                        Ok(Async::Ready(Some(x))) => x,
                        Ok(Async::NotReady) => return Err(io::ErrorKind::WouldBlock.into()),
                        Err(_) | Ok(Async::Ready(None)) => return Err(io::ErrorKind::InvalidData.into()),
                    };

                    if let ListenerToDialerMessage::ProtocolAck { ref name } = msg {
                        if name.as_ref() != &expected_name[..] {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                    } else {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                },
                NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
            };

            // If we reach here, we should transition from `Negotiating` to `Finished`.
            self.0 = match mem::replace(&mut self.0, NegotiatedInner::Poisoned) {
                NegotiatedInner::Negotiating { inner, .. } => {
                    NegotiatedInner::Finished(inner.into_inner())
                },
                NegotiatedInner::Finished(_) => unreachable!(),
                NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
            };
        }
    }
}

impl<TInner> tokio_io::AsyncRead for Negotiated<TInner>
where
    TInner: tokio_io::AsyncRead
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match self.0 {
            NegotiatedInner::Finished(ref s) => s.prepare_uninitialized_buffer(buf),
            NegotiatedInner::Negotiating { ref inner, .. } =>
                inner.get_ref().prepare_uninitialized_buffer(buf),
            NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
        }
    }
}

impl<TInner> io::Write for Negotiated<TInner>
where
    TInner: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.0 {
            NegotiatedInner::Finished(ref mut s) => s.write(buf),
            NegotiatedInner::Negotiating { ref mut inner, .. } => inner.get_mut().write(buf),
            NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.0 {
            NegotiatedInner::Finished(ref mut s) => s.flush(),
            NegotiatedInner::Negotiating { ref mut inner, .. } => inner.get_mut().flush(),
            NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
        }
    }
}

impl<TInner> tokio_io::AsyncWrite for Negotiated<TInner>
where
    TInner: tokio_io::AsyncRead + tokio_io::AsyncWrite
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        loop {
            match self.0 {
                NegotiatedInner::Finished(ref mut s) => return s.shutdown(),
                NegotiatedInner::Negotiating { ref mut inner, ref expected_name } => {
                    // We need to wait for the remote to send either an approval or a refusal,
                    // otherwise for write-only streams we would never know whether anything has
                    // succeeded.
                    let msg = match inner.poll() {
                        Ok(Async::Ready(Some(x))) => x,
                        Ok(Async::NotReady) => return Err(io::ErrorKind::WouldBlock.into()),
                        Err(_) | Ok(Async::Ready(None)) => return Err(io::ErrorKind::InvalidData.into()),
                    };

                    if let ListenerToDialerMessage::ProtocolAck { ref name } = msg {
                        if name.as_ref() != &expected_name[..] {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                    } else {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                }
                NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
            };

            // If we reach here, we should transition from `Negotiating` to `Finished`.
            self.0 = match mem::replace(&mut self.0, NegotiatedInner::Poisoned) {
                NegotiatedInner::Negotiating { inner, .. } => {
                    NegotiatedInner::Finished(inner.into_inner())
                },
                NegotiatedInner::Finished(_) => unreachable!(),
                NegotiatedInner::Poisoned => panic!("Poisonned negotiated stream"),
            };
        }
    }
}
