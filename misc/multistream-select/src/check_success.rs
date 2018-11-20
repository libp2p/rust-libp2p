// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::protocol::{Dialer, DialerReadOnly, ListenerToDialerMessage};
use futures::prelude::*;
use std::mem;
use std::io::{self, Read, Write};
use tokio_io::{AsyncRead, AsyncWrite};

/// Wraps around a `AsyncRead + AsyncWrite`.
///
/// Contains an optional additional first step before the first read success, which is to check
/// whether the remote successfully supports the protocol we negotiated. If this is not the case,
/// the stream errors.
pub struct CheckSuccessStream<TStream> {
    inner: CheckSuccessStreamState<TStream>
}

/// Internal state.
enum CheckSuccessStreamState<TStream> {
    CheckSuccess(DialerReadOnly<TStream>),
    Stream(TStream),
    Poisoned,
}

impl<TStream> CheckSuccessStream<TStream>
where TStream: AsyncRead + AsyncWrite,
{
    // Creates a `CheckSuccessStream` that holds a successfully negotiated stream.
    #[inline]
    pub(crate) fn finished(stream: TStream) -> Self {
        CheckSuccessStream {
            inner: CheckSuccessStreamState::Stream(stream)
        }
    }

    // Creates a `CheckSuccessStream` that holds a `Dialer` waiting for the remote's response.
    #[inline]
    pub(crate) fn negotiating(stream: Dialer<TStream>) -> Self {
        CheckSuccessStream {
            inner: CheckSuccessStreamState::CheckSuccess(stream.into_read_only())
        }
    }
}

impl<TStream> Read for CheckSuccessStream<TStream>
where TStream: AsyncRead + AsyncWrite
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match mem::replace(&mut self.inner, CheckSuccessStreamState::Poisoned) {
                CheckSuccessStreamState::Stream(mut stream) => {
                    let read = stream.read(buf);
                    self.inner = CheckSuccessStreamState::Stream(stream);
                    return read;
                },
                CheckSuccessStreamState::CheckSuccess(mut dialer) => {
                    match dialer.poll() {
                        Ok(Async::Ready(Some(ListenerToDialerMessage::ProtocolAck { .. }))) => {},
                        Ok(Async::NotReady) => {
                            return Err(io::ErrorKind::WouldBlock.into());
                        }
                        Ok(Async::Ready(Some(_))) | Ok(Async::Ready(None)) => {
                            return Err(io::Error::new(io::ErrorKind::Other,
                                "The remote doesn't support the protocol we negotiated"));
                        },
                        Err(err) => {
                            return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
                        },
                    };

                    self.inner = CheckSuccessStreamState::Stream(dialer.into_inner());
                },
                CheckSuccessStreamState::Poisoned => {
                    return Err(io::Error::new(io::ErrorKind::Other,
                                              "The stream is in a poisoned state"));
                },
            }
        }
    }
}

impl<TStream> AsyncRead for CheckSuccessStream<TStream>
where TStream: AsyncRead + AsyncWrite
{
}

impl<TStream> Write for CheckSuccessStream<TStream>
where TStream: AsyncRead + AsyncWrite
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.inner {
            CheckSuccessStreamState::CheckSuccess(ref mut inner) => {
                inner.write(buf)
            },
            CheckSuccessStreamState::Stream(ref mut inner) => {
                inner.write(buf)
            },
            CheckSuccessStreamState::Poisoned => {
                Err(io::Error::new(io::ErrorKind::Other, "Stream is in a poisoned state"))
            },
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match self.inner {
            CheckSuccessStreamState::CheckSuccess(ref mut inner) => {
                inner.flush()
            },
            CheckSuccessStreamState::Stream(ref mut inner) => {
                inner.flush()
            },
            CheckSuccessStreamState::Poisoned => {
                Err(io::Error::new(io::ErrorKind::Other, "Stream is in a poisoned state"))
            },
        }
    }
}

impl<TStream> AsyncWrite for CheckSuccessStream<TStream>
where TStream: AsyncRead + AsyncWrite
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match self.inner {
            CheckSuccessStreamState::CheckSuccess(ref mut inner) => {
                inner.shutdown()
            },
            CheckSuccessStreamState::Stream(ref mut inner) => {
                inner.shutdown()
            },
            CheckSuccessStreamState::Poisoned => {
                Err(io::Error::new(io::ErrorKind::Other, "Stream is in a poisoned state"))
            },
        }
    }
}
